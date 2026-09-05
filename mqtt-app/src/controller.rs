//! # パソコン役（指示を出す側）のロジック
//!
//! ここは`mqtt-server`実行ファイルだけが使うモジュールです。
//! ブローカー自身とは別に、このパソコン自身も普通のMQTTクライアントとしてブローカーへ
//! 接続し、チャット・ファイル送信(`/send`)・ジョブの一斉配信(`/job`)を行います。
//! ファイルの受信やジョブの実行はマイコン役（[`crate::device`]・[`crate::file_transfer`]）の
//! 仕事なので、ここには一切出てきません。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rumqttc::{Client, Event, MqttOptions, Packet, QoS};

use crate::messages::{AckMsg, CmdMsg, DataMsg, DoneMsg, JobMsg, OfferMsg, PresenceMsg, ReceivedMsg};
use crate::seq::{check_seq, next_seq, ControllerSeqState};

/// 送信申し出(id)ごとに「これから送るファイルのパス」を覚えておく辞書。
///
/// `Arc<Mutex<HashMap<...>>>`は、C++でいう
/// `std::shared_ptr<std::mutex_wrapped<std::unordered_map<std::string, std::filesystem::path>>>`
/// のようなものです。複数のスレッド（標準入力を読むスレッドと、MQTT受信を処理する
/// メインスレッド）から安全に読み書きするために、この形にしています。
type PendingOffers = Arc<Mutex<HashMap<String, PathBuf>>>;

/// 「マイコンの名前 → 今オンラインかどうか」を覚えておく辞書（ジョブ配信先の名簿）。
type Roster = Arc<Mutex<HashMap<String, bool>>>;

/// 今まさに配信中で、全員の完了報告を待っているジョブの情報。
///
/// `mpsc::Sender<String>`の`mpsc`は"multi-producer, single-consumer"（送る側は複数いても
/// いいが、受け取る側は1つだけ）というチャンネルです。C++でいう、スレッドセーフな
/// キュー＋条件変数（`std::condition_variable`）をセットにしたようなもの、と考えると
/// イメージしやすいです。「別スレッドから`tx.send(値)`で投げ込み、こちら側は
/// `rx.recv()`（またはタイムアウト付きの`rx.recv_timeout()`）で待ち受ける」という使い方をします。
struct InFlightJob {
    id: String,
    tx: mpsc::Sender<String>,
}
type InFlightState = Arc<Mutex<Option<InFlightJob>>>;

/// ジョブを送ってから、完了報告が来ないマイコンを「エラー」と判断するまでの待ち時間。
/// `const`はC++の`constexpr`に近いコンパイル時定数です。
const JOB_TIMEOUT: Duration = Duration::from_secs(10);

/// 入力行の先頭にある `/qos0 ` `/qos1 ` `/qos2 ` プレフィックスを読み取り、
/// (QoS, プレフィックスを除いた本文) を返す。プレフィックスが無ければQoS1（AtLeastOnce）扱い。
///
/// 戻り値の`(QoS, &str)`はタプル型で、C++の`std::pair<QoS, std::string_view>`に近いものです。
/// `&str`（文字列スライス）はC++の`std::string_view`と同様、文字列データそのものを
/// コピーせず「どこからどこまでか」という参照だけを持つ軽量な型です。
fn parse_qos_prefix(line: &str) -> (QoS, &str) {
    for (prefix, qos) in [
        ("/qos0 ", QoS::AtMostOnce),
        ("/qos1 ", QoS::AtLeastOnce),
        ("/qos2 ", QoS::ExactlyOnce),
    ] {
        // strip_prefixは「先頭がprefixと一致していれば、それを取り除いた残りを返す」関数で、
        // 戻り値はOption<&str>（一致すればSome(残り)、しなければNone）。
        // if let Some(rest) = ... は「Someだった場合だけ中身(rest)を取り出して処理する」という、
        // C++で言えばif文の中でoptionalの値を取り出すのに近いパターンマッチです。
        if let Some(rest) = line.strip_prefix(prefix) {
            return (qos, rest);
        }
    }
    (QoS::AtLeastOnce, line)
}

/// 受信したpublishのトピックが `<topic>/<名前>/data` の形なら、その`<名前>`部分を取り出す。
/// 一致しなければ`None`。
///
/// `Option<&str>`を返しているのは、余計な文字列コピーをせず、元の`publish_topic`の一部を
/// そのまま指す「借用」で済ませるためです（C++の`std::string_view`を返す関数に近い発想）。
fn parse_data_topic<'a>(publish_topic: &'a str, topic: &str) -> Option<&'a str> {
    publish_topic
        .strip_prefix(topic)?
        .strip_prefix('/')?
        .strip_suffix("/data")
}

/// ファイルの中身を、繋いだ相手(TcpStream)へ送りつける。
///
/// 自前の単純な通信ルール（プロトコル）を使う:
///   1. id（OFFER/ACKと同じ申し出id）の長さ(u32)＋中身（UTF-8バイト列）
///   2. ファイル名の長さ(u32)＋中身（UTF-8のバイト列）
///   3. ファイルサイズ(u64、8バイト・ビッグエンディアン)
///   4. ファイルの中身そのもの
/// 受け取る側（マイコン役、[`crate::file_transfer`]モジュール）はこの順番通りに読み取る。
///
/// 戻り値の`io::Result<()>`は、C++でいう「戻り値の型が`void`だけど、失敗もありうる関数」を
/// 表す書き方です（`()`はC++の`void`に相当する「値を持たない型」）。
fn send_file_to(host: &str, port: u16, id: &str, path: &Path) -> io::Result<()> {
    // TcpStream::connect で相手(host:port)へTCP接続する。
    // 末尾の `?` は「エラーだったらこの関数を即座にreturnする」という意味の演算子で、
    // C++で言えば `if (auto e = connect(...); e) return e;` のような定型処理を
    // 1文字にまとめたものです（例外を投げる代わりに、エラーを戻り値として伝播させます）。
    let mut stream = TcpStream::connect((host, port))?;

    let id_bytes = id.as_bytes();
    // to_be_bytes(): 数値を「決まったバイト順（ビッグエンディアン）」に変換する。
    // C++でいう htonl()/htons() に相当する処理です。
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

    // io::copy はファイルの中身を丸ごと（読みながら少しずつ）TCPストリームへ流し込んでくれる。
    // C++で言えば、read()とwrite()を繰り返すループを1関数呼び出しにまとめたようなものです。
    io::copy(&mut file, &mut stream)?;
    Ok(())
}

/// `AckMsg`（`DataMsg::FileAck`の中身）を受け取ったときの処理。
/// 自分が送った申し出(id)に対する返事だった場合、そのidに対応するファイルを
/// 教えてもらったhost:portへ実際に送信する。
fn handle_ack(ack: AckMsg, pending_offers: &PendingOffers) {
    // remove() は「辞書から取り出して削除する」。C++のstd::unordered_map::extract()に近い。
    // 自分が送った申し出でなければNoneが返り何もしない。
    let Some(path) = pending_offers.lock().unwrap().remove(&ack.id) else {
        return;
    };

    println!("[system] {}:{} へ接続してファイルを送信します…", ack.host, ack.port);

    // thread::spawn(move || { ... }) は、C++のstd::thread(lambda)に相当します。
    // moveを付けることで、この中で使うack・pathの所有権を新しいスレッドに完全に渡します
    // （渡した後、外側のスレッドではack・pathはもう使えません。C++のstd::moveと違い、
    // 「渡した後に誤って使ってしまう」バグはコンパイルエラーとして検出されます）。
    thread::spawn(move || match send_file_to(&ack.host, ack.port, &ack.id, &path) {
        Ok(()) => println!("[system] 送信完了: {}", path.display()),
        Err(e) => eprintln!("[system] ファイル送信エラー: {e}"),
    });
}

/// `ReceivedMsg`（`DataMsg::FileReceived`の中身）を受け取ったときの処理。
/// `who`は、これを送ってきたマイコンの名前（トピックの`<名前>`部分から渡される）。
fn handle_file_received(who: &str, received: ReceivedMsg) {
    if received.status == "ok" {
        println!(
            "[system] ジョブ{}: {who} が受信完了しました（{} bytes）",
            received.id, received.size
        );
    } else {
        println!("[system] ジョブ{}: {who} での受信に失敗しました", received.id);
    }
}

/// `PresenceMsg`（`DataMsg::Presence`の中身）を受け取ったときの処理。
fn handle_presence(who: &str, presence: PresenceMsg, roster: &Roster) {
    let mut roster = roster.lock().unwrap();
    if presence.status == "online" {
        // insert()の戻り値は「上書きする前にそこにあった古い値」（無ければNone）。
        // C++のstd::mapならoperator[]で代入した後、以前の値は捨てられてしまいますが、
        // Rustのinsert()は古い値を捨てずにOption<V>として返してくれるので、
        // 「新規追加だったか、既存の更新だったか」をこの1行で判定できます。
        if roster.insert(who.to_string(), true) != Some(true) {
            println!("[system] {who} がオンラインになりました");
        }
    } else if roster.remove(who).is_some() {
        println!("[system] {who} がオフラインになりました");
    }
}

/// `DoneMsg`（`DataMsg::JobDone`の中身）を受け取ったときの処理。
fn handle_job_done(who: &str, done: DoneMsg, inflight: &InFlightState) {
    let guard = inflight.lock().unwrap();
    if let Some(job) = guard.as_ref() {
        if job.id == done.id {
            // send()が失敗するのは、待っている側が既にタイムアウトして諦めた後くらいなので無視してよい
            let _ = job.tx.send(who.to_string());
        }
    }
}

/// パソコン役としてブローカーへ接続し、チャット・`/send`・`/job`を受け付け続ける。
/// この関数はプログラムが終わるまでブロックし続ける
/// （C++でいう、`main()`の中の`while (true) { ... }`メインループに相当する部分です）。
pub fn run(name: String, host: String, port: u16, topic: String) {
    // 全マイコンへの一斉配信(JOB)専用の、特別な名前"all"を宛先としたcmdトピック。
    let all_cmd_topic = format!("{topic}/all/cmd");
    // 各マイコンの報告(presence/ACK/RECEIVED/DONE)は、ワイルドカードでまとめて購読する。
    let data_wildcard = format!("{topic}/+/data");

    let seq = ControllerSeqState::new();

    let mut mqttoptions = MqttOptions::new(&name, host.clone(), port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));
    let (client, mut connection) = Client::new(mqttoptions, 10);

    // パソコン役はコマンド(cmd)を送る側であって受け取る側ではないので、cmdトピックの購読は不要。
    client.subscribe(&topic, QoS::AtMostOnce).unwrap();
    client.subscribe(&data_wildcard, QoS::AtLeastOnce).unwrap();

    let pending_offers: PendingOffers = Arc::new(Mutex::new(HashMap::new()));
    let roster: Roster = Arc::new(Mutex::new(HashMap::new()));
    let inflight: InFlightState = Arc::new(Mutex::new(None));

    // 別スレッドを立てて「キーボード入力 → メッセージ送信」を担当させる。
    // { } で囲んでいるのは、この中だけで使うclient・name等の複製（clone）を用意して、
    // 元の変数は後半（メインループ）でも引き続き使えるようにするためのブロックスコープです
    // （C++でいう、変数のライフタイムを絞るための`{ }`スコープと同じ使い方です）。
    {
        let client = client.clone();
        let name = name.clone();
        let topic = topic.clone();
        let pending_offers = Arc::clone(&pending_offers);
        let all_cmd_topic = all_cmd_topic.clone();
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
                    // ロックした瞬間の名簿を「今回のジョブの宛先」として写し取る
                    // （HashSet<String>はC++のstd::unordered_set<std::string>に相当）
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

                    // 「完了報告が来たら教えて」という約束を、チャンネル(tx/rx)で表現する。
                    // txはこの後publishした後にメインスレッド側のhandle_job_doneへ渡され、
                    // 完了報告のたびにtx.send()される。rxはこの下のwhileループで受け取る側。
                    let (tx, rx) = mpsc::channel::<String>();
                    *inflight.lock().unwrap() = Some(InFlightJob { id: id.clone(), tx });

                    let job = JobMsg {
                        id: id.clone(),
                        from: name.clone(),
                        content: content.to_string(),
                        seq: next_seq(&seq.job_counter),
                    };
                    let payload = serde_json::to_vec(&CmdMsg::Job(job)).unwrap();
                    client.publish(&all_cmd_topic, QoS::AtLeastOnce, false, payload).unwrap();
                    println!(
                        "[system] ジョブ{id}を{}台のマイコン({targets:?})へ配信しました。完了を待っています…",
                        targets.len()
                    );

                    // 全員分の完了報告が来るか、タイムアウトするまでここでブロックして待つ。
                    // これはC++でいう
                    //   while (!remaining.empty()) {
                    //       if (cv.wait_until(lock, deadline) == cv_status::timeout) break;
                    //       remaining.erase(received_name);
                    //   }
                    // に相当する待ち合わせ処理です。
                    let mut remaining = targets;
                    let deadline = Instant::now() + JOB_TIMEOUT;
                    while !remaining.is_empty() {
                        let now = Instant::now();
                        if now >= deadline {
                            break;
                        }
                        // recv_timeout: 「残り時間内に何か届けばそれを返す、届かなければタイムアウトを返す」
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
                if let Some(rest) = line.strip_prefix("/send ") {
                    // split_once(' ') で「最初のスペースの前後」に文字列を2つに割る
                    // （C++のstd::string::find(' ')＋substr()を1回で済ませたイメージ）
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
                    // 宛先(to)は、ペイロードではなくトピック自体（`<topic>/<to>/cmd`）で表す。
                    let offer_topic = format!("{topic}/{to}/cmd");
                    let payload = serde_json::to_vec(&CmdMsg::FileOffer(offer)).unwrap();
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

    // connection.iter() は「ブローカーから届いたイベントを1つずつ返してくれる、
    // 終わりのないイテレータ」です。C++でいう、受信用のイベントループ
    // （`while (auto event = poll()) { ... }`）に相当します。
    for notification in connection.iter() {
        match notification {
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                let text = String::from_utf8_lossy(&publish.payload);

                if let Some(who) = parse_data_topic(&publish.topic, &topic) {
                    let Ok(data) = serde_json::from_str::<DataMsg>(&text) else {
                        continue;
                    };
                    // どのバリアントでも、そのマイコンの1本のdataトピックに乗っているので、
                    // 欠落チェックは1箇所（seq.data_tracker）にまとめられる。
                    let (seq_num, is_birth) = match &data {
                        DataMsg::Presence(p) => (p.seq, p.status == "online"),
                        DataMsg::FileAck(a) => (a.seq, false),
                        DataMsg::FileReceived(r) => (r.seq, false),
                        DataMsg::JobDone(d) => (d.seq, false),
                    };
                    check_seq(who, seq_num, &seq.data_tracker, is_birth);

                    match data {
                        DataMsg::Presence(p) => handle_presence(who, p, &roster),
                        DataMsg::FileAck(a) => handle_ack(a, &pending_offers),
                        DataMsg::FileReceived(r) => handle_file_received(who, r),
                        DataMsg::JobDone(d) => handle_job_done(who, d, &inflight),
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
