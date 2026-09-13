//! # マイコン役の、OFFER（ファイル送信の申し出）・JOB（ジョブ配信）・ABORT（中断指示）を
//! 受け取ったときの処理
//!
//! ここは`mqtt-client`実行ファイルだけが使うモジュールです。
//! 生TCPでの実際のファイル受信は[`crate::file_transfer`]モジュールが担当します。
//!
//! `<topic>/cmd/<自分の名前>`か`<topic>/cmd/all`に届いた[`crate::messages::CmdMsg`]の
//! 中身を、呼び出し側（`mqtt-client.rs`）が既に`match`で振り分けた後、
//! `OfferMsg`/`JobMsg`/`AbortMsg`それぞれに対応するのがこの関数群です。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rumqttc::{Client, QoS};

use crate::messages::{
    AbortMsg, AbortedMsg, AckMsg, DataMsg, DeviceState, DoneMsg, JobMsg, OfferMsg, ProgressMsg,
    StatusMsg,
};
use crate::mqtt_log;
use crate::seq::{check_seq, next_seq, DeviceSeqState, SeqCounter};

/// ジョブ処理を何段階に分けて進捗報告するか（ダミー処理を均等に分割しているだけ）。
const PROGRESS_STEPS: u32 = 5;

/// 今処理中のジョブ（無ければ`None`）。`handle_abort`はこれを見て、中断対象が
/// 「今まさに処理中のジョブと同じIDか」を確認してから、`cancel`フラグを立てる。
/// フィールドは非公開のまま（`pub`なのは型自体だけ）で、`mqtt-client.rs`側は
/// [`CurrentJobState`]を中身の見えない不透明な値として持ち回るだけでよい。
pub struct CurrentJob {
    id: String,
    cancel: Arc<AtomicBool>,
}

/// [`CurrentJob`]を複数スレッド（ジョブ処理スレッドと、MQTT受信を処理するメインスレッド）
/// から安全に読み書きするための共有状態。`mqtt-client.rs`の`main`で1つだけ作り、
/// `handle_job`・`handle_abort`の両方に渡す。
pub type CurrentJobState = Arc<Mutex<Option<CurrentJob>>>;

/// 空の（何も処理していない）[`CurrentJobState`]を新しく作る。
pub fn new_current_job_state() -> CurrentJobState {
    Arc::new(Mutex::new(None))
}

/// 自分の稼働状態（`NSTATUS`）をretain付きでpublishする。
pub fn publish_status(client: &Client, status_topic: &str, state: DeviceState, counter: &SeqCounter) {
    let msg = StatusMsg { state, seq: next_seq(counter) };
    let payload = serde_json::to_vec(&msg).unwrap();
    mqtt_log::log_publish(status_topic, &payload);
    client.publish(status_topic, QoS::AtLeastOnce, true, payload).unwrap();
}

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
/// 完了報告(`JobDone`)を返す。途中で`handle_abort`から中断要求が来た場合は、そこで打ち切って
/// 中断報告(`JobAborted`)を返す。
#[allow(clippy::too_many_arguments)]
pub fn handle_job(
    job: JobMsg,
    client: &Client,
    data_topic: &str,
    status_topic: &str,
    seq: &DeviceSeqState,
    current_job: &CurrentJobState,
) {
    check_seq(&job.from, job.seq, &seq.job_tracker, false);

    println!("[system] ジョブ{}を受信: {}（処理中…）", job.id, job.content);

    let cancel = Arc::new(AtomicBool::new(false));
    *current_job.lock().unwrap() =
        Some(CurrentJob { id: job.id.clone(), cancel: Arc::clone(&cancel) });
    publish_status(
        client,
        status_topic,
        DeviceState::Printing { job_id: job.id.clone() },
        &seq.status_counter,
    );

    let client = client.clone();
    let data_topic = data_topic.to_string();
    let status_topic = status_topic.to_string();
    let data_counter = seq.data_counter.clone();
    let status_counter = seq.status_counter.clone();
    let current_job = Arc::clone(current_job);

    // 処理は時間がかかりうるので別スレッドに任せ、その間もMQTTの受信ループは止めない
    // （C++でいう、重い処理をstd::threadに逃がしてメインのイベントループを止めない、という定石です）
    thread::spawn(move || {
        let mut aborted = false;
        for step in 1..=PROGRESS_STEPS {
            thread::sleep(Duration::from_millis(1000 / PROGRESS_STEPS as u64)); // ここが実際の印刷・処理にあたる部分（今はダミー）
            if cancel.load(Ordering::SeqCst) {
                aborted = true;
                break;
            }
            let percent = (step * 100 / PROGRESS_STEPS) as u8;
            println!("[system] ジョブ{}: {percent}% 完了", job.id);
            let progress =
                ProgressMsg { id: job.id.clone(), percent, seq: next_seq(&data_counter) };
            let payload = serde_json::to_vec(&DataMsg::JobProgress(progress)).unwrap();
            mqtt_log::log_publish(&data_topic, &payload);
            client.publish(&data_topic, QoS::AtLeastOnce, false, payload).unwrap();
        }

        *current_job.lock().unwrap() = None;
        publish_status(&client, &status_topic, DeviceState::Idle, &status_counter);

        if aborted {
            println!("[system] ジョブ{}を中断しました", job.id);
            let msg = AbortedMsg { id: job.id, seq: next_seq(&data_counter) };
            let payload = serde_json::to_vec(&DataMsg::JobAborted(msg)).unwrap();
            mqtt_log::log_publish(&data_topic, &payload);
            client.publish(&data_topic, QoS::AtLeastOnce, false, payload).unwrap();
        } else {
            println!("[system] ジョブ{}の処理が完了しました", job.id);
            let done = DoneMsg { id: job.id, seq: next_seq(&data_counter) };
            let payload = serde_json::to_vec(&DataMsg::JobDone(done)).unwrap();
            mqtt_log::log_publish(&data_topic, &payload);
            client.publish(&data_topic, QoS::AtLeastOnce, false, payload).unwrap();
        }
    });
}

/// `AbortMsg`（`CmdMsg::Abort`の中身）を受け取ったときの処理。
/// 今処理中のジョブと同じIDのときだけ、中断フラグを立てる（別のジョブ宛て、または
/// 既に自分が処理していないジョブのIDなら、単に無視する）。
pub fn handle_abort(abort: AbortMsg, seq: &DeviceSeqState, current_job: &CurrentJobState) {
    // AbortはJobと同じ<topic>/NCMD/allに乗るので、seqカウンタもJobと共用する
    // （同じトピックは1系列のseq、という設計のため。詳しくはseq.rsのコメント参照）。
    check_seq(&abort.from, abort.seq, &seq.job_tracker, false);

    let guard = current_job.lock().unwrap();
    if let Some(current) = guard.as_ref() {
        if current.id == abort.id {
            current.cancel.store(true, Ordering::SeqCst);
        }
    }
}
