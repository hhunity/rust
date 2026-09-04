// io: 標準入力(キーボード入力)や、TCP通信のバイト列を読み書きするための機能
// BufRead: 「1行ずつ読む」ためのメソッド(.lines())を使えるようにするトレイト
// Read/Write: TcpStreamからバイト列を読み書きするためのトレイト
use std::io::{self, BufRead, Read, Write};
// TcpListener: 「ここに繋いできて」と待ち受けるための機能（サーバー役）
// TcpStream: 実際にTCPで繋がった通信路（データを送受信する）
// UdpSocket: 自分のIPアドレスを調べるための小技に使う（下のdetect_local_ip関数を参照）
use std::net::{TcpListener, TcpStream, UdpSocket};
// ファイル読み書き(open/createなど)のための機能
use std::fs;
// ファイルパスを表す型
use std::path::{Path, PathBuf};
// 複数スレッドから安全に共有するための「Arc（参照カウント付きの共有ポインタ）」と
// 「Mutex（同時に1つのスレッドしか中身を触れないようにする鍵）」
use std::sync::{Arc, Mutex};
// 送信申し出(id)ごとに送信予定のファイルパスを覚えておく辞書
use std::collections::HashMap;
// thread: 別スレッド（並行して動く処理の流れ）を作るための機能
use std::thread;
// Duration: 「5秒」のような時間の長さを表す型
// SystemTime/UNIX_EPOCH: 「今の時刻」を使って、送信申し出ごとの重複しないIDを作るために使う
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// rumqttcクレート（外部ライブラリ）が提供している、MQTTクライアントを作るための部品たち
use rumqttc::{Client, Event, MqttOptions, Packet, QoS};

/// 送信申し出(id)ごとに「これから送るファイルのパス」を覚えておく辞書。
/// 標準入力を読むスレッドと、MQTT受信を処理するメインスレッドの両方から触るので、
/// Arc<Mutex<...>> で「複数スレッドから安全に共有できる箱」にしている。
type PendingOffers = Arc<Mutex<HashMap<String, PathBuf>>>;

/// 入力行の先頭にある `/qos0 ` `/qos1 ` `/qos2 ` プレフィックスを読み取り、
/// (QoS, プレフィックスを除いた本文) を返す。プレフィックスが無ければQoS1（AtLeastOnce）扱い。
fn parse_qos_prefix(line: &str) -> (QoS, &str) {
    for (prefix, qos) in [
        ("/qos0 ", QoS::AtMostOnce),  // QoS0 = 送りっぱなし（届く保証なし、その代わり速い）
        ("/qos1 ", QoS::AtLeastOnce), // QoS1 = 最低1回は届く（重複する可能性あり）
        ("/qos2 ", QoS::ExactlyOnce), // QoS2 = ちょうど1回だけ届く（一番確実、その代わり遅い）
    ] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return (qos, rest);
        }
    }
    (QoS::AtLeastOnce, line)
}

/// 自分自身のIPアドレス（相手から接続してもらうためのアドレス）を推測する。
///
/// 仕組み: 実際には通信しないUDPソケットを作り、ブローカーのアドレスへ`connect`だけしてみる。
/// すると、OSが「そのアドレス宛てに通信するならこのネットワークインターフェースを使う」と
/// 判断してくれるので、そのローカル側アドレスを覗き見ることで自分のIPが分かる、という小技。
/// （ローカルでの動作確認用の簡易実装。インターネット越しやNAT環境では正しく動かないことがある）
fn detect_local_ip(broker_host: &str, broker_port: u16) -> String {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("UDPソケットの作成に失敗しました");
    socket
        .connect((broker_host, broker_port))
        .expect("ブローカーへの疑似接続に失敗しました");
    socket
        .local_addr()
        .expect("ローカルアドレスの取得に失敗しました")
        .ip()
        .to_string()
}

/// ファイルの中身を、繋いだ相手(TcpStream)へ送りつける。
///
/// 自前の単純な通信ルール（プロトコル）を使う:
///   1. ファイル名の長さ(u32、4バイト・ビッグエンディアン)
///   2. ファイル名の中身（UTF-8のバイト列）
///   3. ファイルサイズ(u64、8バイト・ビッグエンディアン)
///   4. ファイルの中身そのもの
/// 受け取る側(receive_file関数)はこの順番通りに読み取る。
fn send_file_to(host: &str, port: u16, path: &Path) -> io::Result<()> {
    // TcpStream::connect で相手(host:port)へTCP接続する
    let mut stream = TcpStream::connect((host, port))?;

    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let filename_bytes = filename.as_bytes();

    // to_be_bytes(): 数値を「決まった順番のバイト列」に変換する（ネットワーク越しにやり取りする定番の方法）
    stream.write_all(&(filename_bytes.len() as u32).to_be_bytes())?;
    stream.write_all(filename_bytes)?;

    let mut file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    stream.write_all(&size.to_be_bytes())?;

    // io::copy はファイルの中身を丸ごと（読みながら少しずつ）TCPストリームへ流し込んでくれる
    io::copy(&mut file, &mut stream)?;
    Ok(())
}

/// TcpListenerが1件の接続を受け付け、send_file_toが送ってきたファイルを保存する。
/// 戻り値は (受け取ったファイル名, サイズ, 保存先パス)。
fn receive_file(listener: TcpListener) -> io::Result<(String, u64, PathBuf)> {
    // accept() は「誰かが繋いでくるまで待ち、繋がってきたらその通信路を返す」
    let (mut stream, _addr) = listener.accept()?;

    // ファイル名の長さ(4バイト)を読み取る
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let name_len = u32::from_be_bytes(len_buf) as usize;

    // ファイル名の中身を読み取る
    let mut name_buf = vec![0u8; name_len];
    stream.read_exact(&mut name_buf)?;
    let filename = String::from_utf8_lossy(&name_buf).to_string();

    // ファイルサイズ(8バイト)を読み取る
    let mut size_buf = [0u8; 8];
    stream.read_exact(&mut size_buf)?;
    let size = u64::from_be_bytes(size_buf);

    // 送られてきたファイル名そのままだと元のファイルを上書きしかねないので、頭に "received_" を付けて保存する
    let save_path = PathBuf::from(format!("received_{filename}"));
    let mut out_file = fs::File::create(&save_path)?;

    // stream.take(size): 「このストリームから最大size バイトだけ読む」という制限付きの読み取り口を作る
    // （これが無いと「ファイルの終わり」がどこか分からず、ずっと読み続けようとしてしまう）
    let mut limited = (&mut stream).take(size);
    io::copy(&mut limited, &mut out_file)?;

    Ok((filename, size, save_path))
}

/// 「OFFER|id|from|to|filename|size」という送信申し出メッセージを受け取ったときの処理。
/// 自分宛て(to == my_name)のときだけ反応し、TCPで待ち受けを始めて、
/// 「ここに繋いで」という返事(ACK)をackトピックへpublishする。
fn handle_offer(
    payload: &str,
    my_name: &str,
    client: &Client,
    ack_topic: &str,
    broker_host: &str,
    broker_port: u16,
) {
    // "|" で区切って各項目に分解する。件数が合わなければ壊れたメッセージとして無視する
    let parts: Vec<&str> = payload.split('|').collect();
    let [_, id, from, to, filename, size] = parts.as_slice() else {
        return;
    };
    if *to != my_name {
        return; // 自分宛てのオファーでなければ何もしない
    }

    println!("[system] {from}さんから {filename} ({size} bytes) を送りたいと連絡がありました。受信準備をします…");

    // ポート番号 0 を指定すると、OSが空いているポートを1つ選んで割り当ててくれる
    let listener = TcpListener::bind("0.0.0.0:0").expect("TCP待ち受けの開始に失敗しました");
    let port = listener.local_addr().unwrap().port();
    let host = detect_local_ip(broker_host, broker_port);

    println!("[system] {host}:{port} で待ち受けを開始します");

    // ここに繋いでほしい、という返事をMQTTで送り返す
    let ack = format!("ACK|{id}|{host}|{port}");
    client
        .publish(ack_topic, QoS::AtLeastOnce, false, ack.as_bytes())
        .unwrap();

    // 実際のファイル受信は時間がかかる（相手が繋いでくるまで待つ）ので、別スレッドに任せる
    thread::spawn(move || match receive_file(listener) {
        Ok((filename, size, path)) => {
            println!("[system] 受信完了: {filename} ({size} bytes) -> {}", path.display());
        }
        Err(e) => eprintln!("[system] ファイル受信エラー: {e}"),
    });
}

/// 「ACK|id|host|port」という返事を受け取ったときの処理。
/// 自分が送った申し出(id)に対する返事だった場合、そのidに対応するファイルを
/// 教えてもらったhost:portへ実際に送信する。
fn handle_ack(payload: &str, pending_offers: &PendingOffers) {
    let parts: Vec<&str> = payload.split('|').collect();
    let [_, id, host, port_str] = parts.as_slice() else {
        return;
    };
    let Ok(port) = port_str.parse::<u16>() else {
        return;
    };

    // remove() は「辞書から取り出して削除する」。自分が送った申し出でなければNoneが返り何もしない
    let path = pending_offers.lock().unwrap().remove(*id);
    let Some(path) = path else {
        return;
    };

    let host = host.to_string();
    println!("[system] {host}:{port} へ接続してファイルを送信します…");

    // ファイル送信も時間がかかるので別スレッドに任せ、キーボード入力の受付は止めないようにする
    thread::spawn(move || match send_file_to(&host, port, &path) {
        Ok(()) => println!("[system] 送信完了: {}", path.display()),
        Err(e) => eprintln!("[system] ファイル送信エラー: {e}"),
    });
}

// Rustのプログラムは main関数 から実行が始まる
fn main() {
    // --- ① コマンドライン引数（起動時に渡した文字列）を読み取る ---
    let mut args = std::env::args().skip(1);

    let name = args.next().unwrap_or_else(|| {
        eprintln!("usage: mqtt-client <name> [host] [port] [topic]");
        std::process::exit(1);
    });
    let host = args.next().unwrap_or_else(|| "127.0.0.1".to_string());
    let port: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1883);
    let topic = args.next().unwrap_or_else(|| "chat".to_string());

    // ファイル送信の申し出(offer)と、その返事(ack)専用のトピックを、チャットのトピックから派生させる
    // 例: topicが"chat"なら "chat/file/offer" と "chat/file/ack"
    let offer_topic = format!("{topic}/file/offer");
    let ack_topic = format!("{topic}/file/ack");

    // --- ② MQTT接続の設定を作る ---
    let mut mqttoptions = MqttOptions::new(&name, host.clone(), port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    // --- ③ 実際にMQTTブローカー（サーバー）へ接続する ---
    let (client, mut connection) = Client::new(mqttoptions, 10);

    // チャット用トピックに加えて、ファイル送信の申し出・返事のトピックも購読する
    client.subscribe(&topic, QoS::AtMostOnce).unwrap();
    client.subscribe(&offer_topic, QoS::AtLeastOnce).unwrap();
    client.subscribe(&ack_topic, QoS::AtLeastOnce).unwrap();

    // 自分が送信を申し出た(まだ返事を待っている)ファイルの一覧
    let pending_offers: PendingOffers = Arc::new(Mutex::new(HashMap::new()));

    // --- ④ 別スレッドを立てて「キーボード入力 → メッセージ送信」を担当させる ---
    {
        let client = client.clone();
        let name = name.clone();
        let topic = topic.clone();
        let offer_topic = offer_topic.clone();
        let pending_offers = Arc::clone(&pending_offers);

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

                // "/send 宛先の名前 ファイルパス" と入力されたら、チャットではなくファイル送信の申し出として扱う
                if let Some(rest) = line.strip_prefix("/send ") {
                    // split_once(' ') で「最初のスペースの前後」に文字列を2つに割る
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

                    // 今の時刻(ナノ秒)を使って、他の申し出とかぶらないID文字列を作る
                    let nanos = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos();
                    let id = format!("{name}-{nanos}");

                    // 「id → 送るファイルのパス」を覚えておく（相手からACKが来たときに使う）
                    pending_offers.lock().unwrap().insert(id.clone(), path);

                    let offer = format!("OFFER|{id}|{name}|{to}|{filename}|{}", metadata.len());
                    client
                        .publish(&offer_topic, QoS::AtLeastOnce, false, offer.as_bytes())
                        .unwrap();
                    println!(
                        "[system] {to}へ {filename} ({} bytes) の送信を申し出ました。相手の応答を待っています…",
                        metadata.len()
                    );
                    continue;
                }

                // ここに来たら普通のチャットメッセージ
                let (qos, text) = parse_qos_prefix(&line);
                let message = format!("{name}: {text}");
                client
                    .publish(&topic, qos, false, message.as_bytes())
                    .unwrap();
            }
        });
    }

    println!("接続しました host={host} port={port} topic={topic} name={name}");
    println!("メッセージを入力して Enter で送信します（Ctrl+D で終了）");
    println!("先頭に /qos0 /qos1 /qos2 を付けるとそのメッセージだけQoSを変更できます（省略時はQoS1）");
    println!("/send <宛先の名前> <ファイルパス> でファイルを送れます（例: /send bob ./photo.png）");

    // --- ⑤ メインスレッドでは「受信したメッセージを表示する」処理をずっと続ける ---
    for notification in connection.iter() {
        match notification {
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                let text = String::from_utf8_lossy(&publish.payload);

                if publish.topic == offer_topic {
                    handle_offer(&text, &name, &client, &ack_topic, &host, port);
                } else if publish.topic == ack_topic {
                    handle_ack(&text, &pending_offers);
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
