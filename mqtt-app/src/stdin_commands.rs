//! # 標準入力からのコマンド受け付け（チャット・`/send`・`/job`・`/queue`・`/cancel`）
//!
//! [`crate::controller::run`]から呼ばれる、キーボード入力を読み取る専用スレッドの
//! 中身をまとめたモジュール。MQTT受信を処理するメインスレッドとは完全に別スレッドで
//! 動くので、必要な状態（`client`・`pending_offers`など）は[`spawn`]の引数として受け取る。
//!
//! `/job`は実際の配信は行わず、[`crate::job_queue::JobQueue`]へ積むだけ。積まれたジョブを
//! 順番に配信して完了を待つのは、別スレッドの[`crate::job_worker`]の仕事。

use std::fs;
use std::io::{self, BufRead};
use std::path::PathBuf;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use rumqttc::{Client, QoS};

use crate::controller::PendingOffers;
use crate::job_queue::JobQueue;
use crate::messages::{CmdMsg, OfferMsg};
use crate::mqtt_log;
use crate::seq::{next_seq, ControllerSeqState};

/// 入力行の先頭にある `/qos0 ` `/qos1 ` `/qos2 ` プレフィックスを読み取り、
/// (QoS, プレフィックスを除いた本文) を返す。プレフィックスが無ければQoS1（AtLeastOnce）扱い。
fn parse_qos_prefix(line: &str) -> (QoS, &str) {
    for (prefix, qos) in [
        ("/qos0 ", QoS::AtMostOnce),
        ("/qos1 ", QoS::AtLeastOnce),
        ("/qos2 ", QoS::ExactlyOnce),
    ] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return (qos, rest);
        }
    }
    (QoS::AtLeastOnce, line)
}

/// 標準入力を読み取り、チャット・`/send`・`/job`を処理し続ける専用スレッドを立てる。
/// この関数自体はスレッドを立てたらすぐ返り、スレッドの終了は待たない
/// （呼び出し元の`controller::run`が、これまで通りメインループを続けられるようにするため）。
pub(crate) fn spawn(
    client: Client,
    name: String,
    topic: String,
    pending_offers: PendingOffers,
    seq: ControllerSeqState,
    queue: JobQueue,
) {
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.is_empty() {
                continue;
            }

            // "/job 内容": 印刷ジョブキューに積むだけ。実際の配信・完了待ちは
            // バックグラウンドの[`crate::job_worker`]が順番に行う。
            if line == "/job" || line.starts_with("/job ") {
                let content = line.strip_prefix("/job").unwrap().trim();
                if content.is_empty() {
                    println!("[system] 使い方: /job <内容>");
                    continue;
                }
                let id = queue.enqueue(content.to_string());
                println!("[system] ジョブ{id}をキューに追加しました（バックグラウンドで順番に配信されます。/queueで確認できます）");
                continue;
            }

            // "/queue": キューにある全ジョブと状態の一覧を表示する
            if line == "/queue" {
                let jobs = queue.list();
                if jobs.is_empty() {
                    println!("[system] キューは空です");
                } else {
                    for job in &jobs {
                        println!("[system] {} [{:?}] {}", job.id, job.status, job.content);
                    }
                }
                continue;
            }

            // "/cancel ジョブID": まだ配信されていないジョブをキューから取り消す
            if line == "/cancel" || line.starts_with("/cancel ") {
                let id = line.strip_prefix("/cancel").unwrap().trim();
                if id.is_empty() {
                    println!("[system] 使い方: /cancel <ジョブID>（/queueでIDを確認できます）");
                    continue;
                }
                if queue.cancel(id) {
                    println!("[system] ジョブ{id}を取り消しました");
                } else {
                    println!("[system] ジョブ{id}は取り消せません（存在しないか、既に配信中/完了済みです）");
                }
                continue;
            }

            // "/send 宛先の名前 ファイルパス": ファイル送信の申し出
            if line == "/send" || line.starts_with("/send ") {
                let rest = line.strip_prefix("/send").unwrap().trim();
                let Some((to, path_str)) = rest.split_once(' ') else {
                    println!("[system] 使い方: /send <宛先の名前> <ファイルパス>");
                    continue;
                };
                let path = PathBuf::from(path_str);
                let metadata = match fs::metadata(&path) {
                    Ok(m) => m,
                    Err(e) => {
                        println!("[system] ファイルが読めません: {path_str} ({e})");
                        continue;
                    }
                };
                let filename = path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| path_str.to_string());

                let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
                let id = format!("{name}-{nanos}");

                pending_offers.lock().unwrap().insert(id.clone(), path);

                let offer = OfferMsg {
                    id,
                    from: name.clone(),
                    filename: filename.clone(),
                    size: metadata.len(),
                    seq: next_seq(&seq.offer_counter),
                };
                // 宛先(to)は、ペイロードではなくトピック自体（`<topic>/NCMD/<to>`）で表す。
                let offer_topic = format!("{topic}/NCMD/{to}");
                let payload = serde_json::to_vec(&CmdMsg::FileOffer(offer)).unwrap();
                mqtt_log::log_publish(&offer_topic, &payload);
                client.publish(&offer_topic, QoS::AtLeastOnce, false, payload).unwrap();
                println!(
                    "[system] {to}へ {filename} ({} bytes) の送信を申し出ました。相手の応答を待っています…",
                    metadata.len()
                );
                continue;
            }

            // ここに来たら普通のチャットメッセージ
            let (qos, text) = parse_qos_prefix(&line);
            let message = format!("{name}: {text}");
            mqtt_log::log_publish(&topic, message.as_bytes());
            client.publish(&topic, qos, false, message.as_bytes()).unwrap();
        }
    });
}
