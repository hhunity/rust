//! パソコン役（指示を出す側）のロジック。
//!
//! ブローカー自身とは別に、このパソコン自身も普通のMQTTクライアントとしてブローカーへ
//! 接続し、チャット・ファイル送信(`/send`)・ジョブの一斉配信(`/job`)を行う。
//! ファイルの受信やジョブの実行はマイコン役（`mqtt-client`プロジェクト）の仕事なので、
//! ここには一切出てこない。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rumqttc::{Client, Event, MqttOptions, Packet, QoS};

use crate::messages::{AckMsg, DoneMsg, JobMsg, OfferMsg, PresenceMsg, ReceivedMsg};
use crate::seq::{check_seq, next_seq, ControllerSeqState};

/// 送信申し出(id)ごとに「これから送るファイルのパス」を覚えておく辞書。
type PendingOffers = Arc<Mutex<HashMap<String, PathBuf>>>;

/// 「マイコンの名前 → 今オンラインかどうか」を覚えておく辞書（ジョブ配信先の名簿）。
type Roster = Arc<Mutex<HashMap<String, bool>>>;

/// 今まさに配信中で、全員の完了報告を待っているジョブの情報。
struct InFlightJob {
    id: String,
    tx: mpsc::Sender<String>,
}
type InFlightState = Arc<Mutex<Option<InFlightJob>>>;

/// ジョブを送ってから、完了報告が来ないマイコンを「エラー」と判断するまでの待ち時間
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

/// ファイルの中身を、繋いだ相手(TcpStream)へ送りつける。
///
/// 自前の単純な通信ルール（プロトコル）を使う:
///   1. id（OFFER/ACKと同じ申し出id）の長さ(u32)＋中身（UTF-8バイト列）
///   2. ファイル名の長さ(u32)＋中身（UTF-8のバイト列）
///   3. ファイルサイズ(u64、8バイト・ビッグエンディアン)
///   4. ファイルの中身そのもの
/// 受け取る側（マイコン役、`mqtt-client`プロジェクト）はこの順番通りに読み取る。
fn send_file_to(host: &str, port: u16, id: &str, path: &Path) -> io::Result<()> {
    let mut stream = TcpStream::connect((host, port))?;

    let id_bytes = id.as_bytes();
    stream.write_all(&(id_bytes.len() as u32).to_be_bytes())?;
    stream.write_all(id_bytes)?;

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let filename_bytes = filename.as_bytes();
    stream.write_all(&(filename_bytes.len() as u32).to_be_bytes())?;
    stream.write_all(filename_bytes)?;

    let mut file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    stream.write_all(&size.to_be_bytes())?;

    io::copy(&mut file, &mut stream)?;
    Ok(())
}

/// `AckMsg`（JSON）の返事を受け取ったときの処理。
/// 自分が送った申し出(id)に対する返事だった場合、そのidに対応するファイルを
/// 教えてもらったhost:portへ実際に送信する。
fn handle_ack(payload: &str, pending_offers: &PendingOffers, seq: &ControllerSeqState) {
    let Ok(ack) = serde_json::from_str::<AckMsg>(payload) else {
        return;
    };
    check_seq(&ack.from, ack.seq, &seq.ack_tracker, false);

    let Some(path) = pending_offers.lock().unwrap().remove(&ack.id) else {
        return;
    };

    println!("[system] {}:{} へ接続してファイルを送信します…", ack.host, ack.port);

    thread::spawn(move || match send_file_to(&ack.host, ack.port, &ack.id, &path) {
        Ok(()) => println!("[system] 送信完了: {}", path.display()),
        Err(e) => eprintln!("[system] ファイル送信エラー: {e}"),
    });
}

/// `ReceivedMsg`（JSON）の、生TCPでのファイル転送結果の報告を受け取ったときの処理。
fn handle_file_received(payload: &str, seq: &ControllerSeqState) {
    let Ok(received) = serde_json::from_str::<ReceivedMsg>(payload) else {
        return;
    };
    check_seq(&received.who, received.seq, &seq.received_tracker, false);

    if received.status == "ok" {
        println!(
            "[system] ジョブ{}: {} が受信完了しました（{} bytes）",
            received.id, received.who, received.size
        );
    } else {
        println!("[system] ジョブ{}: {} での受信に失敗しました", received.id, received.who);
    }
}

/// presenceトピック（`<topic>/presence/<名前>`）への`PresenceMsg`（JSON）通知を受け取ったときの処理。
fn handle_presence(
    publish_topic: &str,
    payload: &str,
    presence_prefix: &str,
    roster: &Roster,
    seq: &ControllerSeqState,
) {
    let Some(who) = publish_topic.strip_prefix(presence_prefix).and_then(|s| s.strip_prefix('/'))
    else {
        return;
    };
    let Ok(presence) = serde_json::from_str::<PresenceMsg>(payload) else {
        return;
    };
    check_seq(who, presence.seq, &seq.presence_tracker, presence.status == "online");

    let mut roster = roster.lock().unwrap();
    if presence.status == "online" {
        if roster.insert(who.to_string(), true) != Some(true) {
            println!("[system] {who} がオンラインになりました");
        }
    } else if roster.remove(who).is_some() {
        println!("[system] {who} がオフラインになりました");
    }
}

/// `DoneMsg`（JSON）の完了報告を受け取ったときの処理。
fn handle_job_done(payload: &str, inflight: &InFlightState, seq: &ControllerSeqState) {
    let Ok(done) = serde_json::from_str::<DoneMsg>(payload) else {
        return;
    };
    check_seq(&done.who, done.seq, &seq.done_tracker, false);

    let guard = inflight.lock().unwrap();
    if let Some(job) = guard.as_ref() {
        if job.id == done.id {
            let _ = job.tx.send(done.who);
        }
    }
}

/// パソコン役としてブローカーへ接続し、チャット・`/send`・`/job`を受け付け続ける。
/// この関数はプログラムが終わるまでブロックし続ける。
pub fn run(name: String, host: String, port: u16, topic: String) {
    let offer_prefix = format!("{topic}/file/offer");
    let ack_prefix = format!("{topic}/file/ack");
    let my_ack_topic = format!("{ack_prefix}/{name}");
    let received_prefix = format!("{topic}/file/received");
    let presence_prefix = format!("{topic}/presence");
    let job_topic = format!("{topic}/job/queue");
    let job_done_prefix = format!("{topic}/job/done");

    let seq = ControllerSeqState::new();

    let mut mqttoptions = MqttOptions::new(&name, host.clone(), port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));
    let (client, mut connection) = Client::new(mqttoptions, 10);

    // パソコン役はファイルを受け取らないのでOFFERトピックの購読は不要。
    // 自分が送ったOFFERへの返事(ACK)、生TCP転送の結果(RECEIVED)、マイコンの在室確認(presence)、
    // ジョブの完了報告(DONE)を購読する。
    client.subscribe(&topic, QoS::AtMostOnce).unwrap();
    client.subscribe(&my_ack_topic, QoS::AtLeastOnce).unwrap();
    client
        .subscribe(format!("{received_prefix}/+"), QoS::AtLeastOnce)
        .unwrap();
    client
        .subscribe(format!("{presence_prefix}/+"), QoS::AtLeastOnce)
        .unwrap();
    client
        .subscribe(format!("{job_done_prefix}/+"), QoS::AtLeastOnce)
        .unwrap();

    let pending_offers: PendingOffers = Arc::new(Mutex::new(HashMap::new()));
    let roster: Roster = Arc::new(Mutex::new(HashMap::new()));
    let inflight: InFlightState = Arc::new(Mutex::new(None));

    // 別スレッドを立てて「キーボード入力 → メッセージ送信」を担当させる
    {
        let client = client.clone();
        let name = name.clone();
        let topic = topic.clone();
        let offer_prefix = offer_prefix.clone();
        let pending_offers = Arc::clone(&pending_offers);
        let job_topic = job_topic.clone();
        let roster = Arc::clone(&roster);
        let inflight = Arc::clone(&inflight);
        let seq = seq.clone();

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
                if let Some(content) = line.strip_prefix("/job ") {
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
                    let payload = serde_json::to_vec(&job).unwrap();
                    client.publish(&job_topic, QoS::AtLeastOnce, false, payload).unwrap();
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
                            Err(_) => break,
                        }
                    }
                    *inflight.lock().unwrap() = None;

                    if remaining.is_empty() {
                        println!("[system] ジョブ{id}は全員完了しました");
                    } else {
                        println!("[system] エラー: ジョブ{id}は次のマイコンから応答がありませんでした: {remaining:?}");
                    }
                    continue;
                }

                // "/send 宛先の名前 ファイルパス": ファイル送信の申し出
                if let Some(rest) = line.strip_prefix("/send ") {
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
                        to: to.to_string(),
                        filename: filename.clone(),
                        size: metadata.len(),
                        seq: next_seq(&seq.offer_counter),
                    };
                    let offer_topic = format!("{offer_prefix}/{to}");
                    let payload = serde_json::to_vec(&offer).unwrap();
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
                client.publish(&topic, qos, false, message.as_bytes()).unwrap();
            }
        });
    }

    println!("接続しました host={host} port={port} topic={topic} name={name}（パソコン役）");
    println!("メッセージを入力して Enter で送信します（Ctrl+D で終了）");
    println!("先頭に /qos0 /qos1 /qos2 を付けるとそのメッセージだけQoSを変更できます（省略時はQoS1）");
    println!("/send <宛先の名前> <ファイルパス> でファイルを送れます（例: /send device1 ./photo.png）");
    println!("/job <内容> で、今オンラインの全マイコンへ一斉配信し、全員完了するまで待ちます（例: /job print A4x3）");

    for notification in connection.iter() {
        match notification {
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                let text = String::from_utf8_lossy(&publish.payload);

                if publish.topic == my_ack_topic {
                    handle_ack(&text, &pending_offers, &seq);
                } else if publish.topic.starts_with(&received_prefix) {
                    handle_file_received(&text, &seq);
                } else if publish.topic.starts_with(&presence_prefix) {
                    handle_presence(&publish.topic, &text, &presence_prefix, &roster, &seq);
                } else if publish.topic.starts_with(&job_done_prefix) {
                    handle_job_done(&text, &inflight, &seq);
                } else if publish.topic == topic {
                    println!("{text}");
                }
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("接続エラー: {e:?}");
                break;
            }
        }
    }
}
