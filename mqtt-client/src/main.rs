use std::io::{self, BufRead};
use std::thread;
use std::time::Duration;

use rumqttc::{Client, Event, MqttOptions, Packet, QoS};

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_else(|| {
        eprintln!("usage: mqtt-client <name> [host] [port] [topic]");
        std::process::exit(1);
    });
    let host = args.next().unwrap_or_else(|| "127.0.0.1".to_string());
    let port: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1883);
    let topic = args.next().unwrap_or_else(|| "chat".to_string());

    let mut mqttoptions = MqttOptions::new(&name, host.clone(), port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    let (client, mut connection) = Client::new(mqttoptions, 10);

    client.subscribe(&topic, QoS::AtMostOnce).unwrap();

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
                client
                    .publish(&topic, QoS::AtLeastOnce, false, message.as_bytes())
                    .unwrap();
            }
        });
    }

    println!("接続しました host={host} port={port} topic={topic} name={name}");
    println!("メッセージを入力して Enter で送信します（Ctrl+D で終了）");

    for notification in connection.iter() {
        match notification {
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                let text = String::from_utf8_lossy(&publish.payload);
                println!("{text}");
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("接続エラー: {e:?}");
                break;
            }
        }
    }
}
