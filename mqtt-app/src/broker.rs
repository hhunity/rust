//! # MQTTブローカー（サーバー本体）の起動
//!
//! ここは`mqtt-server`実行ファイルだけが使うモジュールです。中身は外部ライブラリ
//! `rumqttd`（既製のMQTTブローカー実装）の設定を組み立てて起動するだけで、
//! 自分たちで通信プロトコルを実装しているわけではありません
//! （C++でいうと、boost.asioで自前のTCPサーバーを書くのではなく、
//! 既製のMQTTブローカーライブラリをリンクして呼び出すだけ、という位置づけです）。

use std::collections::HashMap;
use std::net::SocketAddr;

use rumqttd::{Broker, Config, ConnectionSettings, RouterConfig, ServerSettings};

/// rumqttdのMQTTブローカーを起動する。
///
/// この関数は`broker.start()`の中でずっとブロックし続ける（＝呼び出したスレッドが
/// そのまま待ち受け専用になる）ので、呼び出し側は必ず別スレッドで呼び出すこと
/// （C++でいう、`while (true) { accept(); ... }`のような無限ループを持つ関数を
/// `std::thread`に渡して動かすのと同じ考え方です）。
pub fn run(port: u16) {
    // 0.0.0.0（どのネットワークインターフェースからの接続も受け付ける）+ 指定ポートで
    // 待ち受けアドレスを作る。C++の`sockaddr_in`を組み立てる処理に相当します。
    let listen: SocketAddr = ([0, 0, 0, 0], port).into();

    // rumqttdは「名前付きの待ち受け設定」をHashMap（C++のstd::unordered_map相当）で
    // 複数持てる作りになっている。今回は"v4-1"という名前で1つだけ登録する。
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

    // Config { ... } のように波括弧でフィールドを埋めていく書き方は、C++でいう
    // 集成体初期化（aggregate initialization、`Config cfg = { .id = 0, ... };`に近いもの、
    // C++20の指示付き初期化子と似た見た目）です。使わない機能はNone
    // （C++でいう std::nullopt、Option型の「値なし」）にしています。
    let config = Config {
        id: 0,
        router: RouterConfig {
            max_connections: 1000,               // 同時接続できるクライアント数の上限
            max_outgoing_packet_count: 200,
            max_segment_size: 100 * 1024 * 1024, // メッセージを貯めておく内部バッファのサイズ
            max_segment_count: 10,
            custom_segment: None,
            initialized_filters: None,
            shared_subscriptions_strategy: Default::default(),
        },
        v4: Some(v4),
        v5: None,
        ws: None,
        cluster: None,
        console: None,
        bridge: None,
        prometheus: None,
        metrics: None,
    };

    let mut broker = Broker::new(config);
    println!("[system] MQTTブローカーを 0.0.0.0:{port} で起動しました");
    // ここでブロックする（＝このスレッドはブローカー専用になる）。
    // .unwrap()は「エラーが返ってきたらその場でパニック（異常終了）させる」という意味で、
    // C++でいう「エラーコードを無視せず、失敗したら即座にabort()する」ような書き方です。
    broker.start().unwrap();
}
