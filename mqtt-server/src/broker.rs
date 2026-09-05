//! MQTTブローカー（rumqttd）を起動するモジュール。

use std::collections::HashMap;
use std::net::SocketAddr;

use rumqttd::{Broker, Config, ConnectionSettings, RouterConfig, ServerSettings};

/// rumqttdのMQTTブローカーを起動する。
///
/// この関数は`broker.start()`の中でずっとブロックし続ける（＝呼び出したスレッドが
/// そのまま待ち受け専用になる）ので、呼び出し側は必ず別スレッドで呼び出すこと。
pub fn run(port: u16) {
    // 0.0.0.0（どのネットワークからの接続も受け付ける）+ 指定ポートで待ち受けアドレスを作る
    let listen: SocketAddr = ([0, 0, 0, 0], port).into();

    // MQTT v4（一般的なMQTTのバージョン）用の待ち受け設定を1つ作る
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
    // ここでブロックする（=このスレッドはブローカー専用になる）
    broker.start().unwrap();
}
