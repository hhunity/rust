// io: 標準入力(キーボード入力)を扱うための機能
// BufRead: 「1行ずつ読む」ためのメソッド(.lines())を使えるようにするトレイト（機能の追加口のようなもの）
use std::io::{self, BufRead};
// thread: 別スレッド（並行して動く処理の流れ）を作るための機能
use std::thread;
// Duration: 「5秒」のような時間の長さを表す型
use std::time::Duration;

// rumqttcクレート（外部ライブラリ）が提供している、MQTTクライアントを作るための部品たち
use rumqttc::{Client, Event, MqttOptions, Packet, QoS};

/// 入力行の先頭にある `/qos0 ` `/qos1 ` `/qos2 ` プレフィックスを読み取り、
/// (QoS, プレフィックスを除いた本文) を返す。プレフィックスが無ければQoS1（AtLeastOnce）扱い。
///
/// 関数の型 `fn parse_qos_prefix(line: &str) -> (QoS, &str)` の意味:
///   - 引数 line: &str … 文字列を「借りてくる」（コピーせず参照だけもらう）
///   - 戻り値 (QoS, &str) … QoSの値と、文字列の一部（先頭を除いた部分）をタプル（複数の値の組）で返す
fn parse_qos_prefix(line: &str) -> (QoS, &str) {
    // [ ("/qos0 ", QoS::AtMostOnce), ... ] は「文字列とQoSのペアのリスト（配列）」
    // for (prefix, qos) in ... でリストの中身を1つずつ順番に取り出して調べる
    for (prefix, qos) in [
        ("/qos0 ", QoS::AtMostOnce),   // QoS0 = 送りっぱなし（届く保証なし、その代わり速い）
        ("/qos1 ", QoS::AtLeastOnce),  // QoS1 = 最低1回は届く（重複する可能性あり）
        ("/qos2 ", QoS::ExactlyOnce),  // QoS2 = ちょうど1回だけ届く（一番確実、その代わり遅い）
    ] {
        // strip_prefix: 文字列の先頭が prefix と一致していれば、その部分を取り除いた残りを返す
        // 戻り値は Option<&str>（一致すればSome(残り)、しなければNone）
        // if let Some(rest) = ... で「一致した場合だけ」中身を取り出して処理する
        if let Some(rest) = line.strip_prefix(prefix) {
            return (qos, rest); // 見つかったのでここで即座に関数を抜けて結果を返す
        }
    }
    // どのプレフィックスにも一致しなかった場合は、デフォルトのQoS1のまま元の文字列を返す
    (QoS::AtLeastOnce, line)
}

// Rustのプログラムは main関数 から実行が始まる
fn main() {
    // --- ① コマンドライン引数（起動時に渡した文字列）を読み取る ---
    // std::env::args() で「実行ファイル名, 引数1, 引数2, ...」が順番に取れる
    // .skip(1) で先頭の実行ファイル名を読み飛ばし、引数だけを扱えるようにする
    let mut args = std::env::args().skip(1);

    // args.next() は「次の引数を1つ取り出す」。無ければNoneが返る（Option型）
    // .unwrap_or_else(|| { ... }) は「値があればそれを使い、無ければ { } の中身を実行してその結果を使う」という意味
    // 名前(クライアントID)は必須引数なので、指定が無ければ使い方を表示してプログラムを終了させる
    let name = args.next().unwrap_or_else(|| {
        eprintln!("usage: mqtt-client <name> [host] [port] [topic]"); // eprintln! はエラー出力（標準エラー）に表示する
        std::process::exit(1); // 終了コード1でプログラムを終わらせる
    });
    // host・port・topicは省略可能な引数。指定が無ければデフォルト値を使う
    let host = args.next().unwrap_or_else(|| "127.0.0.1".to_string());
    // .and_then(|s| s.parse().ok()) は「文字列を数値に変換できたらその値、できなければNone」という処理
    let port: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1883);
    let topic = args.next().unwrap_or_else(|| "chat".to_string());

    // --- ② MQTT接続の設定を作る ---
    // MqttOptions::new(クライアントID, 接続先ホスト, ポート番号) で接続設定オブジェクトを作る
    // &name は「nameの値そのものではなく、参照（借用）を渡す」という意味（後でnameをまだ使いたいため）
    let mut mqttoptions = MqttOptions::new(&name, host.clone(), port);
    // set_keep_alive: 5秒ごとに「生きてます」という信号をサーバーに送るよう設定（接続維持のため）
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    // --- ③ 実際にMQTTブローカー（サーバー）へ接続する ---
    // Client::new は (操作用のclient, 受信通知を取り出すためのconnection) の組を返す
    // 数値の10は「一度に処理待ちできるメッセージ数（内部バッファのサイズ）」
    let (client, mut connection) = Client::new(mqttoptions, 10);

    // 指定したトピックを購読(subscribe)する = そのトピックに届いたメッセージを受け取れるようにする
    // ここのQoS::AtMostOnce は「受信側としてのQoS」（届く保証は求めない代わりに軽量）
    client.subscribe(&topic, QoS::AtMostOnce).unwrap();

    // --- ④ 別スレッドを立てて「キーボード入力 → メッセージ送信」を担当させる ---
    // { } で囲んでいるのは、この中だけで使うclient・name・topicの複製（clone）を用意して、
    // 元の変数は後半(⑤)のループでも引き続き使えるようにするため
    {
        // clone() で複製を作る。Rustでは1つの値を複数の場所で同時に使うにはこうして複製するか、
        // 参照(&)を借りるかする必要がある（メモリの安全性をコンパイラがチェックするための仕組み）
        let client = client.clone();
        let name = name.clone();
        let topic = topic.clone();

        // thread::spawn(move || { ... }) で新しいスレッドを作り、その中で { } の処理を並行して実行する
        // move は「client・name・topicの所有権をこのスレッドに渡す（このスレッド専用にする）」という意味
        thread::spawn(move || {
            let stdin = io::stdin(); // キーボード入力（標準入力）のハンドルを取得
            // stdin.lock().lines() で「入力された行を1行ずつ取り出すイテレータ」を作る
            // for line in ... で、Enterが押されるたびに1行ずつ処理していく（Ctrl+Dが押されるまで続く）
            for line in stdin.lock().lines() {
                // 1行の読み取りは失敗する可能性があるので Result型で返ってくる
                // match で「成功(Ok)なら中身を使う、失敗(Err)ならループを抜ける(break)」と処理を分ける
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };
                if line.is_empty() {
                    continue; // 空行（何も入力せずEnterだけ押した）は無視して次の入力を待つ
                }
                // 入力行の先頭にQoS指定プレフィックスが無いか調べる（無ければQoS1がそのまま返る）
                let (qos, text) = parse_qos_prefix(&line);
                // 送信するメッセージを「名前: 本文」という形式の文字列にする
                let message = format!("{name}: {text}");
                // 指定したトピックへメッセージを送信(publish)する
                // 引数は (トピック名, QoS, retainフラグ, 送信するバイト列) の順
                // retain(3番目の引数)はfalse固定＝ブローカーに最新メッセージを保持させない設定
                // .unwrap() は送信に失敗したらそこでスレッドごと異常終了させる、という意味
                client
                    .publish(&topic, qos, false, message.as_bytes())
                    .unwrap();
            }
        });
    }

    // 接続情報と使い方を画面に表示する
    println!("接続しました host={host} port={port} topic={topic} name={name}");
    println!("メッセージを入力して Enter で送信します（Ctrl+D で終了）");
    println!("先頭に /qos0 /qos1 /qos2 を付けるとそのメッセージだけQoSを変更できます（省略時はQoS1）");

    // --- ⑤ メインスレッドでは「受信したメッセージを表示する」処理をずっと続ける ---
    // connection.iter() で「ブローカーから届いたイベント」を1つずつ順番に取り出せる
    // このfor文はプログラムが終わるまでずっとイベントを待ち続ける（無限ループ相当）
    for notification in connection.iter() {
        // matchで、届いたイベントの種類によって処理を分ける
        match notification {
            // 「メッセージが届いた(Publish)」イベントだった場合
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                // メッセージ本文はバイト列(&[u8])なので、画面表示できる文字列に変換する
                // from_utf8_lossy は「文字化けする部分があってもエラーにせず表示する」関数
                let text = String::from_utf8_lossy(&publish.payload);
                println!("{text}");
            }
            // それ以外の種類のイベント（接続完了など）は何もせず無視する
            Ok(_) => {}
            // 通信エラーが起きた場合はエラー内容を表示してループを抜け、プログラムを終了する
            Err(e) => {
                eprintln!("接続エラー: {e:?}");
                break;
            }
        }
    }
}
