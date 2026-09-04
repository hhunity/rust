// HashMap: キーと値のペアを保存する辞書（連想配列）の機能を使うために読み込む
use std::collections::HashMap;
// SocketAddr: 「IPアドレス + ポート番号」をまとめて表す型
use std::net::SocketAddr;
// thread: 別スレッド（並行して動く処理の流れ）を作るための機能
use std::thread;

// rumqttdクレート（外部ライブラリ）が提供している、MQTTブローカーを作るための部品たち
use rumqttd::{Broker, Config, ConnectionSettings, Notification, RouterConfig, ServerSettings};

// Rustのプログラムは main関数 から実行が始まる
fn main() {
    // --- ① 待ち受けるポート番号を決める ---
    // std::env::args() でコマンドライン引数を1つずつ取り出せる（先頭は実行ファイル名なので.nth(1)で2番目=最初の引数を取る）
    // .and_then(...) は「値があれば中の処理をし、無ければNoneのまま」という意味（Option型の連鎖処理）
    // .parse() は文字列を数値(u16)に変換する。失敗するかもしれないので .ok() でエラーを握りつぶしてOption化している
    // .unwrap_or(1883) は「値が取れなければ1883を使う」という意味（コマンドライン引数を省略したときのデフォルトポート）
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1883);

    // 0.0.0.0（どのネットワークからの接続も受け付ける）+ 上で決めたポート番号、で待ち受けアドレスを作る
    // .into() は [0,0,0,0]とportのタプルをSocketAddr型に変換している
    let listen: SocketAddr = ([0, 0, 0, 0], port).into();

    // --- ② MQTT v4（一般的なMQTTのバージョン）用の待ち受け設定を1つ作る ---
    // rumqttdは「名前付きの待ち受け設定」を複数まとめてHashMapで持てる仕組みになっている
    // 今回は "v4-1" という名前で1つだけ登録する
    let mut v4 = HashMap::new();
    v4.insert(
        "v4-1".to_string(),
        ServerSettings {
            name: "v4-1".to_string(),
            listen,
            tls: None, // 暗号化通信(TLS)は使わない
            next_connection_delay_ms: 1,
            connections: ConnectionSettings {
                connection_timeout_ms: 60_000, // 接続タイムアウト（60秒）
                max_payload_size: 20 * 1024,   // 1メッセージの最大サイズ（20KB）
                max_inflight_count: 100,       // 応答待ちにできる最大メッセージ数
                auth: None,                    // ユーザー名/パスワード認証はしない
                external_auth: None,
                dynamic_filters: true,
            },
        },
    );

    // --- ③ ブローカー全体の設定をまとめる ---
    // Config { ... } のように書くと、構造体（複数の値をひとまとめにした箱）の値を作れる
    // 使わない機能は None（「値なし」を表すOption型の値）にしている
    let config = Config {
        id: 0,
        router: RouterConfig {
            max_connections: 1000,              // 同時接続できるクライアント数の上限
            max_outgoing_packet_count: 200,
            max_segment_size: 100 * 1024 * 1024, // メッセージを貯めておく内部バッファのサイズ
            max_segment_count: 10,
            custom_segment: None,
            initialized_filters: None,
            // Default::default() は「その型の標準的な初期値」を使うという意味
            shared_subscriptions_strategy: Default::default(),
        },
        v4: Some(v4), // 上で作った設定を使う（Someは「値あり」を表す）
        v5: None,
        ws: None,
        cluster: None,
        console: None,
        bridge: None,
        prometheus: None,
        metrics: None,
    };

    // --- ④ ブローカー（サーバー本体）を作る ---
    let mut broker = Broker::new(config);

    // ブローカーの中身を覗き見るための「連絡窓口」を作る
    // link_tx: ブローカーに指示を送る側、link_rx: ブローカーからの通知を受け取る側
    // .unwrap() は「エラーが起きていなければ中身を取り出す。エラーならそこでプログラムを異常終了させる」という意味
    let (mut link_tx, mut link_rx) = broker.link("server-logger").unwrap();

    // thread::spawn で新しいスレッドを立てて、ブローカー本体をそこで動かし始める
    // move はこのクロージャ（{ } で囲んだ無名関数）が broker の所有権をそのスレッドに持っていく、という意味
    // これをしないと、broker.start()がずっとブロックしてしまい、下のloop（受信ログ表示）が実行できなくなる
    thread::spawn(move || {
        broker.start().unwrap();
    });

    // "#" はMQTTのワイルドカードで「すべてのトピック」を意味する
    // つまりこのサーバーは、流れてくる全メッセージを監視してログに出すようにしている
    link_tx.subscribe("#").unwrap();

    println!("MQTT server listening on 0.0.0.0:{port}");

    // --- ⑤ メッセージが来るたびにログを表示し続ける（無限ループ） ---
    loop {
        // link_rx.recv() でブローカーからの通知を1つ受け取る（何も無ければここで待機する）
        // 戻り値は Result<Option<Notification>, _> なので、
        //   .unwrap() でResultのエラーを弾き（失敗ならプログラム終了）、
        //   match で中身がSome(通知あり)かNone(通知なし)かで処理を分けている
        let notification = match link_rx.recv().unwrap() {
            Some(v) => v,
            None => continue, // 通知が無ければ何もせず次のループへ
        };

        // 通知の種類が「メッセージの転送(Forward)」だった場合だけ中身を取り出して表示する
        // if let は「パターンにマッチしたときだけ中の値を取り出して処理する」書き方
        if let Notification::Forward(forward) = notification {
            // トピック名やメッセージ本文はバイト列(&[u8])で来るので、文字列として表示できるように変換する
            // from_utf8_lossy は「UTF-8として解釈できない部分があっても、エラーにせず可能な範囲で文字列にする」関数
            let topic = String::from_utf8_lossy(&forward.publish.topic);
            let payload = String::from_utf8_lossy(&forward.publish.payload);
            println!("[{topic}] {payload}");
        }
    }
}
