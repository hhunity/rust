use std::collections::HashMap;
use std::net::SocketAddr;
use std::thread;

use rumqttd::{Broker, Config, ConnectionSettings, Notification, RouterConfig, ServerSettings};

fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1883);
    let listen: SocketAddr = ([0, 0, 0, 0], port).into();

    let mut v4 = HashMap::new();
    v4.insert(
        "v4-1".to_string(),
        ServerSettings {
            name: "v4-1".to_string(),
            listen,
            tls: None,
            next_connection_delay_ms: 1,
            connections: ConnectionSettings {
                connection_timeout_ms: 60_000,
                max_payload_size: 20 * 1024,
                max_inflight_count: 100,
                auth: None,
                external_auth: None,
                dynamic_filters: true,
            },
        },
    );

    let config = Config {
        id: 0,
        router: RouterConfig {
            max_connections: 1000,
            max_outgoing_packet_count: 200,
            max_segment_size: 100 * 1024 * 1024,
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
    let (mut link_tx, mut link_rx) = broker.link("server-logger").unwrap();

    thread::spawn(move || {
        broker.start().unwrap();
    });

    link_tx.subscribe("#").unwrap();

    println!("MQTT server listening on 0.0.0.0:{port}");

    loop {
        let notification = match link_rx.recv().unwrap() {
            Some(v) => v,
            None => continue,
        };

        if let Notification::Forward(forward) = notification {
            let topic = String::from_utf8_lossy(&forward.publish.topic);
            let payload = String::from_utf8_lossy(&forward.publish.payload);
            println!("[{topic}] {payload}");
        }
    }
}
