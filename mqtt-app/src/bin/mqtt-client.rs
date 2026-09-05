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
use mqtt_app::messages::PresenceMsg;
use mqtt_app::seq::{next_seq, DeviceSeqState};

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

    // ファイル送信・presence・ジョブ関連のトピックを、チャットのトピックから派生させる。
    // 宛先・報告元の名前をトピックに含める構造にしている（詳しくはREADME参照）。
    let offer_prefix = format!("{topic}/file/offer");
    let my_offer_topic = format!("{offer_prefix}/{name}");
    let ack_prefix = format!("{topic}/file/ack");
    let received_prefix = format!("{topic}/file/received");
    let presence_prefix = format!("{topic}/presence");
    let my_presence_topic = format!("{presence_prefix}/{name}");
    let job_topic = format!("{topic}/job/queue");
    let job_done_prefix = format!("{topic}/job/done");

    // このマイコンが送るメッセージ全種類ぶんのseqカウンタ・トラッカーをまとめて用意する。
    let seq = DeviceSeqState::new();

    // --- ② MQTT接続の設定を作る ---
    let mut mqttoptions = MqttOptions::new(&name, host.clone(), port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    // 自分のpresenceトピックにLast Will（異常切断時に代わりにブローカーがpublishしてくれる
    // 遺言メッセージ）を登録しておく。こうしておくと、電源断や通信断など「さようなら」を
    // 言えずに落ちた場合でも、ブローカーが自動で"offline"を配ってくれる。
    // retain=trueにしているので、後からpresence topicをsubscribeしたパソコン役にも
    // 「今の状態」がすぐ届く。
    let offline = serde_json::to_vec(&PresenceMsg { status: "offline".to_string(), seq: 0 }).unwrap();
    mqttoptions.set_last_will(LastWill::new(&my_presence_topic, offline, QoS::AtLeastOnce, true));

    // --- ③ 実際にMQTTブローカー（サーバー）へ接続する ---
    let (client, mut connection) = Client::new(mqttoptions, 10);

    // チャット用トピックに加えて、自分宛てのOFFERと、全マイコン向けのJOBを購読する
    client.subscribe(&topic, QoS::AtMostOnce).unwrap();
    client.subscribe(&my_offer_topic, QoS::AtLeastOnce).unwrap();
    client.subscribe(&job_topic, QoS::AtLeastOnce).unwrap();

    // 接続できたらすぐ自分のpresenceトピックに"online"をretain付きでpublishする
    let online = serde_json::to_vec(&PresenceMsg {
        status: "online".to_string(),
        seq: next_seq(&seq.presence_counter),
    })
    .unwrap();
    client.publish(&my_presence_topic, QoS::AtLeastOnce, true, online).unwrap();

    // 起動時に一度だけ固定ポートでlistenを開始し、そのままプログラムが終わるまで
    // ファイル受信を待ち受け続ける（C++でいう、`bind()`＋`listen()`をここで1回だけ行うイメージ）。
    let listener = TcpListener::bind(("0.0.0.0", listen_port))
        .unwrap_or_else(|e| panic!("{listen_port}番ポートでのlisten開始に失敗しました: {e}"));
    println!("[system] {listen_port}番ポートで常時待ち受けを開始しました");
    {
        let client = client.clone();
        let name = name.clone();
        let received_prefix = received_prefix.clone();
        let received_counter = seq.received_counter.clone();
        thread::spawn(move || run_file_listener(listener, client, name, received_prefix, received_counter));
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

                if publish.topic == my_offer_topic {
                    handle_offer(&text, &name, &client, &ack_prefix, &host, port, listen_port, &seq);
                } else if publish.topic == job_topic {
                    handle_job(&text, &name, &client, &job_done_prefix, &seq);
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
