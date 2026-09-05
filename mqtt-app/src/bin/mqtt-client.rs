//! # mqtt-client（マイコン役）の起動処理
//!
//! こちらもC++でいう`main.cpp`に相当する入り口です。マイコン役専用で、パソコン役の
//! ロジック（[`mqtt_app::controller`]）は一切使いません。
//!
//! やっていることは、MQTTブローカーへ接続し、
//!   - 自分のオンライン状態を知らせる（presence）
//!   - 自分専用のファイル受信ポートを開けて待ち受ける（[`mqtt_app::file_transfer`]）
//!   - ファイル送信の申し出(OFFER)やジョブ配信(JOB)に反応する（[`mqtt_app::device`]）
//! ことです。

use std::io::{self, BufRead};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

// rumqttcクレート（外部ライブラリ）が提供している、MQTTクライアントを作るための部品たち。
// LastWillは、接続時に登録しておく「もし異常切断したら、この内容を代わりにpublishして」
// という遺言メッセージです（C++の世界にはこれに相当する標準機能は無く、MQTTプロトコル
// 自体が持っている仕組みです）。
use rumqttc::{Client, Event, LastWill, MqttOptions, Packet, QoS};

use mqtt_app::device::{handle_job, handle_offer};
use mqtt_app::file_transfer::run_file_listener;
use mqtt_app::messages::{BirthDeathMsg, CmdMsg, PresenceMsg};
use mqtt_app::seq::{check_seq, next_seq, DeviceSeqState};

/// 受信したpublishのトピックが `<topic>/STATE/<パソコンの名前>` の形なら、
/// その`<パソコンの名前>`部分を取り出す。一致しなければ`None`。
fn parse_state_topic<'a>(publish_topic: &'a str, topic: &str) -> Option<&'a str> {
    publish_topic.strip_prefix(topic)?.strip_prefix("/STATE/")
}

fn main() {
    // --- ① コマンドライン引数（起動時に渡した文字列）を読み取る ---
    // `.skip(1)`は「0番目（実行ファイル名）を読み飛ばす」という意味です。
    let mut args = std::env::args().skip(1);

    let name = args.next().unwrap_or_else(|| {
        eprintln!("usage: mqtt-client <name> <listen_port> [host] [port] [topic]");
        std::process::exit(1);
    });
    // マイコン役は必ずTCP待ち受けを行うので、listen_portは省略できない必須引数にしている。
    let listen_port: u16 = args
        .next()
        .unwrap_or_else(|| {
            eprintln!("usage: mqtt-client <name> <listen_port> [host] [port] [topic]");
            std::process::exit(1);
        })
        .parse()
        .unwrap_or_else(|_| {
            eprintln!("listen_portは数値で指定してください");
            std::process::exit(1);
        });
    let host = args.next().unwrap_or_else(|| "127.0.0.1".to_string());
    let port: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1883);
    let topic = args.next().unwrap_or_else(|| "chat".to_string());

    // このマイコンが関わるトピックは5つ。Sparkplug B本家のmessage_type名
    // （`spBv1.0/<group_id>/<message_type>/<edge_node_id>`）をそのまま使っている。
    //   - cmd_topic      … ホスト（パソコン）から自分だけに向けた命令（NCMD）。中身はOFFER
    //   - all_cmd_topic  … 全マイコンへの一斉配信命令（NCMD、宛先名は特別な"all"）。中身はJOB
    //   - birth_topic    … 自分の接続通知（NBIRTH）。起動時に1回だけretain publishする
    //   - death_topic    … 自分の切断通知（NDEATH）。Last Willとしてあらかじめ登録しておく
    //   - data_topic     … 自分からの継続的な報告（NDATA）。ACK・RECEIVED・DONEをまとめて1本
    // 名前は常に「そのトピックがどのマイコンのものか」だけを表す（詳しくはREADME参照）。
    let cmd_topic = format!("{topic}/NCMD/{name}");
    let all_cmd_topic = format!("{topic}/NCMD/all");
    let birth_topic = format!("{topic}/NBIRTH/{name}");
    let death_topic = format!("{topic}/NDEATH/{name}");
    let data_topic = format!("{topic}/NDATA/{name}");
    // パソコン（ホストアプリケーション）自身の生死を知らせるSTATEトピックは、
    // 複数のパソコンがいる可能性もあるのでワイルドカードでまとめて購読する。
    let state_wildcard = format!("{topic}/STATE/+");

    // このマイコンが送るメッセージ全種類ぶんのseqカウンタ・トラッカーをまとめて用意する。
    let seq = DeviceSeqState::new();

    // --- ② MQTT接続の設定を作る ---
    let mut mqttoptions = MqttOptions::new(&name, host.clone(), port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    // 自分のNDEATHトピックにLast Will（異常切断時に代わりにブローカーがpublishしてくれる
    // 遺言メッセージ）を登録しておく。こうしておくと、電源断や通信断など「さようなら」を
    // 言えずに落ちた場合でも、ブローカーが自動でNDEATHを配ってくれる。
    // retain=trueにしているので、後からNDEATHをワイルドカード購読したパソコン役にも
    // 「切断した」という事実がすぐ届く。
    let death = serde_json::to_vec(&BirthDeathMsg { seq: 0 }).unwrap();
    mqttoptions.set_last_will(LastWill::new(&death_topic, death, QoS::AtLeastOnce, true));

    // --- ③ 実際にMQTTブローカー（サーバー）へ接続する ---
    let (client, mut connection) = Client::new(mqttoptions, 10);

    // チャット用トピックに加えて、自分宛てのNCMDと、全マイコン向けのNCMDを購読する
    client.subscribe(&topic, QoS::AtMostOnce).unwrap();
    client.subscribe(&cmd_topic, QoS::AtLeastOnce).unwrap();
    client.subscribe(&all_cmd_topic, QoS::AtLeastOnce).unwrap();
    client.subscribe(&state_wildcard, QoS::AtLeastOnce).unwrap();

    // 接続できたらすぐ自分のNBIRTHをretain付きでpublishする
    let birth = serde_json::to_vec(&BirthDeathMsg { seq: next_seq(&seq.presence_counter) }).unwrap();
    client.publish(&birth_topic, QoS::AtLeastOnce, true, birth).unwrap();

    // 起動時に一度だけ固定ポートでlistenを開始し、そのままプログラムが終わるまで
    // ファイル受信を待ち受け続ける（C++でいう、`bind()`＋`listen()`をここで1回だけ行うイメージ）。
    let listener = TcpListener::bind(("0.0.0.0", listen_port))
        .unwrap_or_else(|e| panic!("{listen_port}番ポートでのlisten開始に失敗しました: {e}"));
    println!("[system] {listen_port}番ポートで常時待ち受けを開始しました");
    {
        let client = client.clone();
        let data_topic = data_topic.clone();
        let received_counter = seq.data_counter.clone();
        thread::spawn(move || run_file_listener(listener, client, data_topic, received_counter));
    }

    // 別スレッドを立てて「キーボード入力 → チャット送信」を担当させる（デバッグ用の簡易機能）
    {
        let client = client.clone();
        let name = name.clone();
        let topic = topic.clone();

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
                let message = format!("{name}: {line}");
                client.publish(&topic, QoS::AtLeastOnce, false, message.as_bytes()).unwrap();
            }
        });
    }

    println!("接続しました host={host} port={port} topic={topic} name={name}（マイコン役）");

    // --- ④ メインスレッドでは「受信したメッセージを表示・処理する」処理をずっと続ける ---
    for notification in connection.iter() {
        match notification {
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                let text = String::from_utf8_lossy(&publish.payload);

                if publish.topic == cmd_topic || publish.topic == all_cmd_topic {
                    let Ok(cmd) = serde_json::from_str::<CmdMsg>(&text) else {
                        continue;
                    };
                    match cmd {
                        CmdMsg::FileOffer(offer) => {
                            handle_offer(offer, &client, &data_topic, &host, port, listen_port, &seq)
                        }
                        CmdMsg::Job(job) => handle_job(job, &client, &data_topic, &seq),
                    }
                } else if let Some(host_name) = parse_state_topic(&publish.topic, &topic) {
                    // パソコン（ホストアプリケーション）の生死通知。今は表示するだけだが、
                    // 「今指示を出す人がいるか」を知る手がかりとして持たせてある。
                    let Ok(state) = serde_json::from_str::<PresenceMsg>(&text) else {
                        continue;
                    };
                    check_seq(host_name, state.seq, &seq.host_state_tracker, state.status == "online");
                    if state.status == "online" {
                        println!("[system] パソコン({host_name})がオンラインになりました");
                    } else {
                        println!("[system] パソコン({host_name})がオフラインになりました");
                    }
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
