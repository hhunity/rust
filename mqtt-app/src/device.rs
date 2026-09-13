//! # マイコン役の、OFFER（ファイル送信の申し出）とJOB（ジョブ配信）を受け取ったときの処理
//!
//! ここは`mqtt-client`実行ファイルだけが使うモジュールです。
//! 生TCPでの実際のファイル受信は[`crate::file_transfer`]モジュールが担当します。
//!
//! `<topic>/cmd/<自分の名前>`か`<topic>/cmd/all`に届いた[`crate::messages::CmdMsg`]の
//! 中身を、呼び出し側（`mqtt-client.rs`）が既に`match`で振り分けた後、
//! `OfferMsg`/`JobMsg`それぞれに対応するのがこの2関数です。

use std::thread;
use std::time::Duration;

use rumqttc::{Client, QoS};

use crate::messages::{AckMsg, DataMsg, DoneMsg, JobMsg, OfferMsg, ProgressMsg};
use crate::mqtt_log;
use crate::seq::{check_seq, next_seq, DeviceSeqState};

/// ジョブ処理を何段階に分けて進捗報告するか（ダミー処理を均等に分割しているだけ）。
const PROGRESS_STEPS: u32 = 5;

/// `OfferMsg`（`CmdMsg::FileOffer`の中身）を受け取ったときの処理。
///
/// このメッセージは、自分専用のトピック（`<topic>/cmd/<自分の名前>`）にしか
/// 届かないように設計してある（詳しくはREADMEのトピック構造の節を参照）ので、
/// 「これは本当に自分宛てか？」というチェックはここでは不要です（トピック自体が保証しています）。
///
/// 「ここに繋いで」という返事(ACK)を、自分のIPアドレス＋固定ポートで、自分のdataトピックへ
/// publishする（待ち受け自体はもう起動時から動いているので、ここで新しく始める必要はない）。
/// `my_host`は起動時に1回だけ調べたもの（[`crate::file_transfer::detect_local_ip`]参照）を
/// そのまま渡してもらう想定（DHCPで配布された後に起動する運用を前提に、OFFERのたびに
/// 調べ直すことはしていない）。
pub fn handle_offer(
    offer: OfferMsg,
    client: &Client,
    data_topic: &str,
    my_host: &str,
    listen_port: u16,
    seq: &DeviceSeqState,
) {
    check_seq(&offer.from, offer.seq, &seq.offer_tracker, false);

    println!(
        "[system] {}さんから {} ({} bytes) を受け取ります（{my_host}:{listen_port} で待ち受け中）",
        offer.from, offer.filename, offer.size
    );

    // 返事は自分自身のdataトピックへpublishする（誰から見てもこれは「自分からの報告」なので、
    // 相手の名前をトピックに含める必要はない）。
    let ack = AckMsg {
        id: offer.id,
        host: my_host.to_string(),
        port: listen_port,
        seq: next_seq(&seq.data_counter),
    };
    let payload = serde_json::to_vec(&DataMsg::FileAck(ack)).unwrap();
    mqtt_log::log_publish(data_topic, &payload);
    client.publish(data_topic, QoS::AtLeastOnce, false, payload).unwrap();
}

/// `JobMsg`（`CmdMsg::Job`の中身）を受け取ったときの処理。
/// 実際の機器では「内容」に応じて印刷やモーター制御などをするところだが、このサンプルでは
/// 少し待つ(sleep)ことで「処理に時間がかかる」ことだけを再現する。処理を[`PROGRESS_STEPS`]
/// 段階に分け、1段階終わるたびに`JobProgress`（進捗報告）をpublishし、全段階終わったら
/// 完了報告(`JobDone`)を返す。
pub fn handle_job(job: JobMsg, client: &Client, data_topic: &str, seq: &DeviceSeqState) {
    check_seq(&job.from, job.seq, &seq.job_tracker, false);

    println!("[system] ジョブ{}を受信: {}（処理中…）", job.id, job.content);

    let client = client.clone();
    let data_topic = data_topic.to_string();
    let data_counter = seq.data_counter.clone();

    // 処理は時間がかかりうるので別スレッドに任せ、その間もMQTTの受信ループは止めない
    // （C++でいう、重い処理をstd::threadに逃がしてメインのイベントループを止めない、という定石です）
    thread::spawn(move || {
        for step in 1..=PROGRESS_STEPS {
            thread::sleep(Duration::from_millis(1000 / PROGRESS_STEPS as u64)); // ここが実際の印刷・処理にあたる部分（今はダミー）
            let percent = (step * 100 / PROGRESS_STEPS) as u8;
            println!("[system] ジョブ{}: {percent}% 完了", job.id);
            let progress =
                ProgressMsg { id: job.id.clone(), percent, seq: next_seq(&data_counter) };
            let payload = serde_json::to_vec(&DataMsg::JobProgress(progress)).unwrap();
            mqtt_log::log_publish(&data_topic, &payload);
            client.publish(&data_topic, QoS::AtLeastOnce, false, payload).unwrap();
        }

        println!("[system] ジョブ{}の処理が完了しました", job.id);
        let done = DoneMsg { id: job.id, seq: next_seq(&data_counter) };
        let payload = serde_json::to_vec(&DataMsg::JobDone(done)).unwrap();
        mqtt_log::log_publish(&data_topic, &payload);
        client.publish(&data_topic, QoS::AtLeastOnce, false, payload).unwrap();
    });
}
