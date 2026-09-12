//! # 標準入力からのコマンド受け付け（チャット・`/send`・`/job`）
//!
//! [`crate::controller::run`]から呼ばれる、キーボード入力を読み取る専用スレッドの
//! 中身をまとめたモジュール。MQTT受信を処理するメインスレッドとは完全に別スレッドで
//! 動くので、必要な状態（`client`・`roster`など）は[`spawn`]の引数として受け取る。

use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rumqttc::{Client, QoS};

use crate::controller::{InFlightJob, InFlightState, PendingOffers, Roster};
use crate::messages::{CmdMsg, JobMsg, OfferMsg};
use crate::mqtt_log;
use crate::seq::{next_seq, ControllerSeqState};

/// ジョブを送ってから、完了報告が来ないマイコンを「エラー」と判断するまでの待ち時間。
const JOB_TIMEOUT: Duration = Duration::from_secs(10);

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
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn(
    client: Client,
    name: String,
    topic: String,
    all_cmd_topic: String,
    pending_offers: PendingOffers,
    roster: Roster,
    inflight: InFlightState,
    seq: ControllerSeqState,
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

            // "/job 内容": 今オンラインの全マイコンへ一斉配信し、全員完了するまで待つ
            if line == "/job" || line.starts_with("/job ") {
                let content = line.strip_prefix("/job").unwrap().trim();
                if content.is_empty() {
                    println!("[system] 使い方: /job <内容>");
                    continue;
                }
                let targets: HashSet<String> = roster
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, &online)| online)
                    .map(|(who, _)| who.clone())
                    .collect();

                if targets.is_empty() {
                    println!("[system] 今オンラインのマイコンがいないため、ジョブを送信できません");
                    continue;
                }

                let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
                let id = format!("{name}-{nanos}");

                let (tx, rx) = mpsc::channel::<String>();
                *inflight.lock().unwrap() = Some(InFlightJob { id: id.clone(), tx });

                let job = JobMsg {
                    id: id.clone(),
                    from: name.clone(),
                    content: content.to_string(),
                    seq: next_seq(&seq.job_counter),
                };
                let payload = serde_json::to_vec(&CmdMsg::Job(job)).unwrap();
                mqtt_log::log_publish(&all_cmd_topic, &payload);
                client.publish(&all_cmd_topic, QoS::AtLeastOnce, false, payload).unwrap();
                println!(
                    "[system] ジョブ{id}を{}台のマイコン({targets:?})へ配信しました。完了を待っています…",
                    targets.len()
                );

                let mut remaining = targets;
                let deadline = Instant::now() + JOB_TIMEOUT;
                while !remaining.is_empty() {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    match rx.recv_timeout(deadline - now) {
                        Ok(who) => {
                            remaining.remove(&who);
                        }
                        Err(_) => break, // タイムアウト（これ以上待っても来ない）
                    }
                }
                *inflight.lock().unwrap() = None; // 待つのをやめたので、共有状態も片付ける

                if remaining.is_empty() {
                    println!("[system] ジョブ{id}は全員完了しました");
                } else {
                    println!("[system] エラー: ジョブ{id}は次のマイコンから応答がありませんでした: {remaining:?}");
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
