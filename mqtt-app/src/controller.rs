//! # パソコン役（指示を出す側）のロジック
//!
//! ここは`mqtt-server`実行ファイルだけが使うモジュールです。
//! ブローカー自身とは別に、このパソコン自身も普通のMQTTクライアントとしてブローカーへ
//! 接続し、チャット・ファイル送信(`/send`)・ジョブの一斉配信(`/job`)を行います。
//! ファイルの受信やジョブの実行はマイコン役（[`crate::device`]・[`crate::file_transfer`]）の
//! 仕事なので、ここには一切出てきません。

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use reedline_repl_rs::reedline::ExternalPrinter;
use rumqttc::{Client, Event, LastWill, MqttOptions, Packet, QoS};

use crate::job_queue::JobQueue;
use crate::messages::{AckMsg, BirthDeathMsg, DataMsg, DoneMsg, PresenceMsg, ReceivedMsg};
use crate::mqtt_log;
use crate::seq::{check_seq, next_seq, ControllerSeqState};

/// `println!`の代わりにこれで状況を報告する。`reedline-repl-rs`のプロンプトは自分の
/// スレッドだけが端末に書き込む前提でカーソル位置を管理しているため、よそのスレッドから
/// 素の`println!`を呼ぶと入力中の行が壊れて見える。`ExternalPrinter`はただのチャネルで、
/// 実際に端末へ「消す→出す→プロンプトを描き直す」をするのは`reedline`自身のスレッドなので、
/// これ経由なら安全に差し込める。
fn say(printer: &ExternalPrinter<String>, message: impl Into<String>) {
    let _ = printer.print(message.into());
}

/// 送信申し出(id)ごとに「これから送るファイルのパス」を覚えておく辞書。
///
/// `Arc<Mutex<HashMap<...>>>`は、C++でいう
/// `std::shared_ptr<std::mutex_wrapped<std::unordered_map<std::string, std::filesystem::path>>>`
/// のようなものです。複数のスレッド（標準入力を読むスレッドと、MQTT受信を処理する
/// メインスレッド）から安全に読み書きするために、この形にしています。
pub(crate) type PendingOffers = Arc<Mutex<HashMap<String, PathBuf>>>;

/// 「マイコンの名前 → 今オンラインかどうか」を覚えておく辞書（ジョブ配信先の名簿）。
pub(crate) type Roster = Arc<Mutex<HashMap<String, bool>>>;

/// 今まさに配信中で、全員の完了報告を待っているジョブの情報。
///
/// `mpsc::Sender<String>`の`mpsc`は"multi-producer, single-consumer"（送る側は複数いても
/// いいが、受け取る側は1つだけ）というチャンネルです。C++でいう、スレッドセーフな
/// キュー＋条件変数（`std::condition_variable`）をセットにしたようなもの、と考えると
/// イメージしやすいです。「別スレッドから`tx.send(値)`で投げ込み、こちら側は
/// `rx.recv()`（またはタイムアウト付きの`rx.recv_timeout()`）で待ち受ける」という使い方をします。
pub(crate) struct InFlightJob {
    pub(crate) id: String,
    pub(crate) tx: mpsc::Sender<String>,
}
pub(crate) type InFlightState = Arc<Mutex<Option<InFlightJob>>>;

/// 受信したpublishのトピックが `<topic>/<message_type>/<名前>` の形なら、その`<名前>`部分を
/// 取り出す。`message_type`が一致しなければ`None`。
///
/// `Option<&str>`を返しているのは、余計な文字列コピーをせず、元の`publish_topic`の一部を
/// そのまま指す「借用」で済ませるためです（C++の`std::string_view`を返す関数に近い発想）。
fn parse_named_topic<'a>(
    publish_topic: &'a str,
    topic: &str,
    message_type: &str,
) -> Option<&'a str> {
    publish_topic
        .strip_prefix(topic)?
        .strip_prefix('/')?
        .strip_prefix(message_type)?
        .strip_prefix('/')
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
fn handle_ack(ack: AckMsg, pending_offers: &PendingOffers, printer: &ExternalPrinter<String>) {
    // remove() は「辞書から取り出して削除する」。C++のstd::unordered_map::extract()に近い。
    // 自分が送った申し出でなければNoneが返り何もしない。
    let Some(path) = pending_offers.lock().unwrap().remove(&ack.id) else {
        return;
    };

    say(
        printer,
        format!("[system] {}:{} へ接続してファイルを送信します…", ack.host, ack.port),
    );

    // thread::spawn(move || { ... }) は、C++のstd::thread(lambda)に相当します。
    // moveを付けることで、この中で使うack・pathの所有権を新しいスレッドに完全に渡します
    // （渡した後、外側のスレッドではack・pathはもう使えません。C++のstd::moveと違い、
    // 「渡した後に誤って使ってしまう」バグはコンパイルエラーとして検出されます）。
    let printer = printer.clone();
    thread::spawn(
        move || match send_file_to(&ack.host, ack.port, &ack.id, &path) {
            Ok(()) => say(&printer, format!("[system] 送信完了: {}", path.display())),
            Err(e) => say(&printer, format!("[system] ファイル送信エラー: {e}")),
        },
    );
}

/// `ReceivedMsg`（`DataMsg::FileReceived`の中身）を受け取ったときの処理。
/// `who`は、これを送ってきたマイコンの名前（トピックの`<名前>`部分から渡される）。
fn handle_file_received(who: &str, received: ReceivedMsg, printer: &ExternalPrinter<String>) {
    if received.status == "ok" {
        say(
            printer,
            format!(
                "[system] ジョブ{}: {who} が受信完了しました（{} bytes）",
                received.id, received.size
            ),
        );
    } else {
        say(
            printer,
            format!("[system] ジョブ{}: {who} での受信に失敗しました", received.id),
        );
    }
}

/// `NBIRTH`（マイコンが接続した）を受け取ったときの処理。
fn handle_birth(who: &str, roster: &Roster, printer: &ExternalPrinter<String>) {
    // insert()の戻り値は「上書きする前にそこにあった古い値」（無ければNone）。
    // C++のstd::mapならoperator[]で代入した後、以前の値は捨てられてしまいますが、
    // Rustのinsert()は古い値を捨てずにOption<V>として返してくれるので、
    // 「新規追加だったか、既存の更新だったか」をこの1行で判定できます。
    if roster.lock().unwrap().insert(who.to_string(), true) != Some(true) {
        say(printer, format!("[system] {who} がオンラインになりました"));
    }
}

/// `NDEATH`（マイコンが切断した）を受け取ったときの処理。
fn handle_death(who: &str, roster: &Roster, printer: &ExternalPrinter<String>) {
    if roster.lock().unwrap().remove(who).is_some() {
        say(printer, format!("[system] {who} がオフラインになりました"));
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
pub fn run(name: String, host: String, port: u16, topic: String, queue_file: String) {
    // 各マイコンの接続・切断・継続報告は、ワイルドカードでまとめて購読する。
    let birth_wildcard = format!("{topic}/NBIRTH/+");
    let death_wildcard = format!("{topic}/NDEATH/+");
    let data_wildcard = format!("{topic}/NDATA/+");
    // パソコン自身の生死を知らせるSTATEトピック（Sparkplug Bの`STATE`そのもの）。
    let state_topic = format!("{topic}/STATE/{name}");

    let seq = ControllerSeqState::new();

    let mut mqttoptions = MqttOptions::new(&name, host.clone(), port);
    mqttoptions.set_keep_alive(Duration::from_secs(30));

    // マイコン役と同様に、自分のstateトピックにLast Willを登録しておく。
    // パソコンが異常終了しても、ブローカーが自動で"offline"を配ってくれるので、
    // マイコン側は「今指示を出す人がいるかどうか」を知ることができる。
    let offline = serde_json::to_vec(&PresenceMsg {
        status: "offline".to_string(),
        seq: 0,
    })
    .unwrap();
    mqttoptions.set_last_will(LastWill::new(&state_topic, offline, QoS::AtLeastOnce, true));

    let (client, mut connection) = Client::new(mqttoptions, 10);

    // パソコン役はコマンド(NCMD)を送る側であって受け取る側ではないので、NCMDトピックの購読は不要。
    client.subscribe(&topic, QoS::AtMostOnce).unwrap();
    client.subscribe(&birth_wildcard, QoS::AtLeastOnce).unwrap();
    client.subscribe(&death_wildcard, QoS::AtLeastOnce).unwrap();
    client.subscribe(&data_wildcard, QoS::AtLeastOnce).unwrap();

    // 接続できたらすぐ自分のstateトピックに"online"をretain付きでpublishする
    let online = serde_json::to_vec(&PresenceMsg {
        status: "online".to_string(),
        seq: next_seq(&seq.state_counter),
    })
    .unwrap();
    mqtt_log::log_publish(&state_topic, &online);
    client
        .publish(&state_topic, QoS::AtLeastOnce, true, online)
        .unwrap();

    let pending_offers: PendingOffers = Arc::new(Mutex::new(HashMap::new()));
    let roster: Roster = Arc::new(Mutex::new(HashMap::new()));
    let inflight: InFlightState = Arc::new(Mutex::new(None));

    // 印刷ジョブの永続化キュー。ファイルに前回までの未処理ジョブが残っていれば、
    // ここで読み込んだ時点でそれらを引き継ぐ（load_or_create内でDispatched→Pendingに戻す）。
    // 引き継いだジョブも含めて、配信はrunコマンドが呼ばれるまで一切行わない
    // （詳しくは[`crate::job_dispatch`]参照。起動時に勝手に配信されると困る、という
    // 要望から、常駐の配信スレッドは廃止した）。
    let queue = JobQueue::load_or_create(PathBuf::from(&queue_file));
    let pending_count = queue.list().len();

    // 標準入力（キーボード入力）を読み取り、チャット・send・job・run・queueなどの
    // コマンドを処理する専用スレッドを立てる。現在は`reedline-repl-rs`版
    // （[`crate::repl_commands`]）を使っている。元の自作パーサ版に戻したい場合は、
    // 下の呼び出しを`crate::stdin_commands::spawn(...)`に差し替えるだけでよい
    // （ただし`stdin_commands`は`run`コマンドに未対応）。
    //
    // `repl_commands::spawn`はプロンプトを描画する`Repl`をこの場で組み立てて、内部の
    // `ExternalPrinter`（プロンプトと衝突せずに端末へ差し込める仕組み）を返してくれる。
    // これをこの関数自身の`println!`代わりに使い回す（詳しくは[`say`]参照）。
    let printer = crate::repl_commands::spawn(
        client.clone(),
        name.clone(),
        topic.clone(),
        Arc::clone(&pending_offers),
        Arc::clone(&roster),
        Arc::clone(&inflight),
        seq.clone(),
        queue,
    );

    if pending_count > 0 {
        say(
            &printer,
            format!(
                "[system] ジョブキュー({queue_file})から{pending_count}件の未処理ジョブを引き継ぎました（runで配信してください）"
            ),
        );
    }

    say(
        &printer,
        format!("接続しました host={host} port={port} topic={topic} name={name}（パソコン役）"),
    );
    say(&printer, "chat <文章> でメッセージを送れます（例: chat こんにちは）");
    say(&printer, "send <宛先の名前> <ファイルパス> でファイルを送れます（例: send device1 ./photo.png）");
    say(&printer, "job <内容> で印刷ジョブをキューに追加します（例: job print A4x3）。自動では配信されません");
    say(&printer, "run でキューの先頭にあるジョブを1件だけ配信します。queue/status/cancel/retry/clearで管理できます");
    say(&printer, "help で使えるコマンドの一覧を表示します");

    // connection.iter() は「ブローカーから届いたイベントを1つずつ返してくれる、
    // 終わりのないイテレータ」です。C++でいう、受信用のイベントループ
    // （`while (auto event = poll()) { ... }`）に相当します。
    for notification in connection.iter() {
        match notification {
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                let text = String::from_utf8_lossy(&publish.payload);
                mqtt_log::log_receive(&publish.topic, &publish.payload);

                if let Some(who) = parse_named_topic(&publish.topic, &topic, "NBIRTH") {
                    let Ok(msg) = serde_json::from_str::<BirthDeathMsg>(&text) else {
                        continue;
                    };
                    // NBIRTHは接続のたびにseqが0から数え直される（再起動すればカウンタは
                    // リセットされる）のが正常な動きなので、is_birth=trueで警告を抑える。
                    check_seq(who, msg.seq, &seq.presence_tracker, true);
                    handle_birth(who, &roster, &printer);
                } else if let Some(who) = parse_named_topic(&publish.topic, &topic, "NDEATH") {
                    let Ok(msg) = serde_json::from_str::<BirthDeathMsg>(&text) else {
                        continue;
                    };
                    check_seq(who, msg.seq, &seq.presence_tracker, false);
                    handle_death(who, &roster, &printer);
                } else if let Some(who) = parse_named_topic(&publish.topic, &topic, "NDATA") {
                    let Ok(data) = serde_json::from_str::<DataMsg>(&text) else {
                        continue;
                    };
                    // どのバリアントでも、そのマイコンの1本のNDATAトピックに乗っているので、
                    // 欠落チェックは1箇所（seq.data_tracker）にまとめられる。
                    let seq_num = match &data {
                        DataMsg::FileAck(a) => a.seq,
                        DataMsg::FileReceived(r) => r.seq,
                        DataMsg::JobDone(d) => d.seq,
                    };
                    check_seq(who, seq_num, &seq.data_tracker, false);

                    match data {
                        DataMsg::FileAck(a) => handle_ack(a, &pending_offers, &printer),
                        DataMsg::FileReceived(r) => handle_file_received(who, r, &printer),
                        DataMsg::JobDone(d) => handle_job_done(who, d, &inflight),
                    }
                } else if publish.topic == topic {
                    say(&printer, text.into_owned());
                }
            }
            Ok(_) => {}
            Err(e) => {
                say(&printer, format!("接続エラー: {e:?}"));
                break;
            }
        }
    }
}
