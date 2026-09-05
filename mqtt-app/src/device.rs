//! # マイコン役の、OFFER（ファイル送信の申し出）とJOB（ジョブ配信）を受け取ったときの処理
//!
//! ここは`mqtt-client`実行ファイルだけが使うモジュールです。
//! 生TCPでの実際のファイル受信は[`crate::file_transfer`]モジュールが担当します。

use std::thread;
use std::time::Duration;

use rumqttc::{Client, QoS};

use crate::file_transfer::detect_local_ip;
use crate::messages::{AckMsg, DoneMsg, JobMsg, OfferMsg};
use crate::seq::{check_seq, next_seq, DeviceSeqState};

/// `OfferMsg`（JSON）の送信申し出メッセージを受け取ったときの処理。
///
/// このメッセージは、自分専用のトピック（`<topic>/file/offer/<自分の名前>`）にしか
/// 届かないように設計してある（詳しくはREADMEのトピック構造の節を参照）ので、
/// 「これは本当に自分宛てか？」というチェックはここでは不要です（トピック自体が保証しています）。
///
/// 「ここに繋いで」という返事(ACK)を、今のIPアドレス＋固定ポートでackトピックへpublishする
/// （待ち受け自体はもう起動時から動いているので、ここで新しく始める必要はない）。
pub fn handle_offer(
    payload: &str,
    my_name: &str,
    client: &Client,
    ack_topic_prefix: &str,
    broker_host: &str,
    broker_port: u16,
    listen_port: u16,
    seq: &DeviceSeqState,
) {
    // JSON文字列をOfferMsg構造体に変換する。壊れたJSONならErrになるので、
    // let-else（`let Ok(x) = ... else { return; };`）で「失敗したら黙って無視する」処理にしている。
    let Ok(offer) = serde_json::from_str::<OfferMsg>(payload) else {
        return;
    };
    check_seq(&offer.from, offer.seq, &seq.offer_tracker, false);

    // DHCPなどでIPアドレスが変わっている可能性があるので、返事のたびに毎回調べ直す（ポート番号は固定のまま）
    let host = detect_local_ip(broker_host, broker_port);
    println!(
        "[system] {}さんから {} ({} bytes) を受け取ります（{host}:{listen_port} で待ち受け中）",
        offer.from, offer.filename, offer.size
    );

    // 返事は申し出てきた相手(offer.from)専用のトピックへ送り返す
    // （例: ack_topic_prefixが"chat/file/ack"、offer.fromが"pc"なら"chat/file/ack/pc"）
    let ack_topic = format!("{ack_topic_prefix}/{}", offer.from);
    let ack = AckMsg {
        id: offer.id,
        from: my_name.to_string(),
        host,
        port: listen_port,
        seq: next_seq(&seq.ack_counter),
    };
    let payload = serde_json::to_vec(&ack).unwrap();
    client.publish(&ack_topic, QoS::AtLeastOnce, false, payload).unwrap();
}

/// `JobMsg`（JSON）のジョブ配信メッセージを受け取ったときの処理。
/// 実際の機器では「内容」に応じて印刷やモーター制御などをするところだが、このサンプルでは
/// 少し待つ(sleep)ことで「処理に時間がかかる」ことだけを再現し、終わったら完了報告を返す。
pub fn handle_job(payload: &str, my_name: &str, client: &Client, job_done_topic_prefix: &str, seq: &DeviceSeqState) {
    let Ok(job) = serde_json::from_str::<JobMsg>(payload) else {
        return;
    };
    check_seq(&job.from, job.seq, &seq.job_tracker, false);

    println!("[system] ジョブ{}を受信: {}（処理中…）", job.id, job.content);

    let my_name = my_name.to_string();
    let client = client.clone();
    // 完了報告は自分専用のトピックへ送る（例: "chat/job/done/device1"）
    let job_done_topic = format!("{job_done_topic_prefix}/{my_name}");
    let done_counter = seq.done_counter.clone();

    // 処理は時間がかかりうるので別スレッドに任せ、その間もMQTTの受信ループは止めない
    // （C++でいう、重い処理をstd::threadに逃がしてメインのイベントループを止めない、という定石です）
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(1)); // ここが実際の印刷・処理にあたる部分（今はダミー）
        println!("[system] ジョブ{}の処理が完了しました", job.id);
        let done = DoneMsg { id: job.id, who: my_name, seq: next_seq(&done_counter) };
        let payload = serde_json::to_vec(&done).unwrap();
        client.publish(&job_done_topic, QoS::AtLeastOnce, false, payload).unwrap();
    });
}
