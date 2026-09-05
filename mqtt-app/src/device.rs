//! # マイコン役の、OFFER（ファイル送信の申し出）とJOB（ジョブ配信）を受け取ったときの処理
//!
//! ここは`mqtt-client`実行ファイルだけが使うモジュールです。
//! 生TCPでの実際のファイル受信は[`crate::file_transfer`]モジュールが担当します。
//!
//! `<topic>/<自分の名前>/cmd`か`<topic>/all/cmd`に届いた[`crate::messages::CmdMsg`]の
//! 中身を、呼び出し側（`mqtt-client.rs`）が既に`match`で振り分けた後、
//! `OfferMsg`/`JobMsg`それぞれに対応するのがこの2関数です。

use std::thread;
use std::time::Duration;

use rumqttc::{Client, QoS};

use crate::file_transfer::detect_local_ip;
use crate::messages::{AckMsg, DataMsg, DoneMsg, JobMsg, OfferMsg};
use crate::seq::{check_seq, next_seq, DeviceSeqState};

/// `OfferMsg`（`CmdMsg::FileOffer`の中身）を受け取ったときの処理。
///
/// このメッセージは、自分専用のトピック（`<topic>/<自分の名前>/cmd`）にしか
/// 届かないように設計してある（詳しくはREADMEのトピック構造の節を参照）ので、
/// 「これは本当に自分宛てか？」というチェックはここでは不要です（トピック自体が保証しています）。
///
/// 「ここに繋いで」という返事(ACK)を、今のIPアドレス＋固定ポートで、自分のdataトピックへ
/// publishする（待ち受け自体はもう起動時から動いているので、ここで新しく始める必要はない）。
pub fn handle_offer(
    offer: OfferMsg,
    client: &Client,
    data_topic: &str,
    broker_host: &str,
    broker_port: u16,
    listen_port: u16,
    seq: &DeviceSeqState,
) {
    check_seq(&offer.from, offer.seq, &seq.offer_tracker, false);

    // DHCPなどでIPアドレスが変わっている可能性があるので、返事のたびに毎回調べ直す（ポート番号は固定のまま）
    let host = detect_local_ip(broker_host, broker_port);
    println!(
        "[system] {}さんから {} ({} bytes) を受け取ります（{host}:{listen_port} で待ち受け中）",
        offer.from, offer.filename, offer.size
    );

    // 返事は自分自身のdataトピックへpublishする（誰から見てもこれは「自分からの報告」なので、
    // 相手の名前をトピックに含める必要はない）。
    let ack = AckMsg { id: offer.id, host, port: listen_port, seq: next_seq(&seq.data_counter) };
    let payload = serde_json::to_vec(&DataMsg::FileAck(ack)).unwrap();
    client.publish(data_topic, QoS::AtLeastOnce, false, payload).unwrap();
}

/// `JobMsg`（`CmdMsg::Job`の中身）を受け取ったときの処理。
/// 実際の機器では「内容」に応じて印刷やモーター制御などをするところだが、このサンプルでは
/// 少し待つ(sleep)ことで「処理に時間がかかる」ことだけを再現し、終わったら完了報告を返す。
pub fn handle_job(job: JobMsg, client: &Client, data_topic: &str, seq: &DeviceSeqState) {
    check_seq(&job.from, job.seq, &seq.job_tracker, false);

    println!("[system] ジョブ{}を受信: {}（処理中…）", job.id, job.content);

    let client = client.clone();
    let data_topic = data_topic.to_string();
    let data_counter = seq.data_counter.clone();

    // 処理は時間がかかりうるので別スレッドに任せ、その間もMQTTの受信ループは止めない
    // （C++でいう、重い処理をstd::threadに逃がしてメインのイベントループを止めない、という定石です）
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(1)); // ここが実際の印刷・処理にあたる部分（今はダミー）
        println!("[system] ジョブ{}の処理が完了しました", job.id);
        let done = DoneMsg { id: job.id, seq: next_seq(&data_counter) };
        let payload = serde_json::to_vec(&DataMsg::JobDone(done)).unwrap();
        client.publish(&data_topic, QoS::AtLeastOnce, false, payload).unwrap();
    });
}
