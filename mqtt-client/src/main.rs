// io: 標準入力(キーボード入力)を扱うための機能
// BufRead: 「1行ずつ読む」ためのメソッド(.lines())を使えるようにするトレイト
use std::io::{self, BufRead};
// TcpListener: 「ここに繋いできて」と待ち受けるための機能
use std::net::TcpListener;
// thread: 別スレッド（並行して動く処理の流れ）を作るための機能
use std::thread;
// Duration: 「5秒」のような時間の長さを表す型
use std::time::Duration;

// rumqttcクレート（外部ライブラリ）が提供している、MQTTクライアントを作るための部品たち
// LastWill: 接続時に登録しておく「もし異常切断したら、この内容を代わりにpublishして」という遺言メッセージ
use rumqttc::{Client, Event, LastWill, MqttOptions, Packet, QoS};

mod file_transfer;
mod messages;
mod seq;

use file_transfer::{detect_local_ip, run_file_listener};
use messages::{AckMsg, JobMsg, OfferMsg, PresenceMsg};
use seq::{check_seq, next_seq, DeviceSeqState};

/// `OfferMsg`（JSON）の送信申し出メッセージを受け取ったときの処理。
/// 自分専用のトピック（`<topic>/file/offer/<自分の名前>`）にしか届かないので、
/// 「自分宛てかどうか」のチェックは不要（トピック自体がそれを保証している）。
/// 「ここに繋いで」という返事(ACK)を、今のIPアドレス＋固定ポートでackトピックへpublishする
/// （待ち受け自体はもう起動時から動いているので、ここで新しく始める必要はない）。
fn handle_offer(
    payload: &str,
    my_name: &str,
    client: &Client,
    ack_topic_prefix: &str,
    broker_host: &str,
    broker_port: u16,
    listen_port: u16,
    seq: &DeviceSeqState,
) {
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
fn handle_job(payload: &str, my_name: &str, client: &Client, job_done_topic_prefix: &str, seq: &DeviceSeqState) {
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
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(1)); // ここが実際の印刷・処理にあたる部分（今はダミー）
        println!("[system] ジョブ{}の処理が完了しました", job.id);
        let done = messages::DoneMsg { id: job.id, who: my_name, seq: next_seq(&done_counter) };
        let payload = serde_json::to_vec(&done).unwrap();
        client.publish(&job_done_topic, QoS::AtLeastOnce, false, payload).unwrap();
    });
}

// Rustのプログラムは main関数 から実行が始まる
//
// このプログラムは「マイコン役」専用。パソコン役（ブローカー＋指示出しロジック）は
// 別プロジェクトの`mqtt-server`にまとまっている。
fn main() {
    // --- ① コマンドライン引数（起動時に渡した文字列）を読み取る ---
    let mut args = std::env::args().skip(1);

    let name = args.next().unwrap_or_else(|| {
        eprintln!("usage: mqtt-client <name> <listen_port> [host] [port] [topic]");
        std::process::exit(1);
    });
    // マイコン役は必ずTCP待ち受けを行うので、listen_portは省略できない必須引数にしている
    // （前は「省略時はパソコン役」という兼用の作りだったが、パソコン役はmqtt-server側に
    // 移したので、このプロジェクトはマイコン役専用になった）
    let listen_port: u16 = args.next().unwrap_or_else(|| {
        eprintln!("usage: mqtt-client <name> <listen_port> [host] [port] [topic]");
        std::process::exit(1);
    }).parse().unwrap_or_else(|_| {
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
    // ファイル受信を待ち受け続ける
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
