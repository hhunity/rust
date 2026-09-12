//! # mqtt-server（パソコン役）
//!
//! MQTTブローカー（[`rumqttd`](https://docs.rs/rumqttd)）と、パソコン役の指示出しロジックを
//! 1つのバイナリに同居させたプログラム。マイコン役とやり取りするメッセージの一覧（API仕様に
//! あたるもの）は [`messages`] モジュールを参照。seq番号による欠落検知の仕組みは [`seq`] を参照。
//!
//! - [`broker`] — MQTTブローカーの起動
//! - [`controller`] — パソコン役の指示出しロジック（チャット・`/send`・`/job`）
//! - [`messages`] — MQTT上でやり取りするメッセージの形（JSON）＝メッセージ一覧
//! - [`seq`] — メッセージ欠落検知用のseq番号の仕組み

// thread: 別スレッド（並行して動く処理の流れ）を作るための機能
use std::thread;
// Duration: 「0.3秒」のような時間の長さを表す型
use std::time::Duration;

mod broker;
mod controller;
mod messages;
mod seq;

// Rustのプログラムは main関数 から実行が始まる
//
// このプログラムは「パソコン役」を1つのバイナリにまとめたもので、中で2つのことをする。
//   1. MQTTブローカー（broker.rs） … マイコンたちとパソコン自身が繋ぐ相手
//   2. パソコン役の指示出しロジック（controller.rs） … チャット・/send・/job を扱う、
//      ブローカーに対する「ただの1クライアント」
// 同じプロセスで両方動かすことで、パソコンにブローカーと指示出しロジックを同居させている。
fn main() {
    // 1番目の引数: ブローカーの待ち受けポート（省略時は1883）
    let broker_port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1883);
    // 2番目の引数: このパソコン自身のMQTTクライアントID（省略時は"pc"）
    let name = std::env::args().nth(2).unwrap_or_else(|| "pc".to_string());
    // 3番目の引数: チャット・ファイル送信などの基点になるトピック（省略時は"chat"）
    let topic = std::env::args().nth(3).unwrap_or_else(|| "chat".to_string());

    // ブローカーを別スレッドで起動する（broker::runはブロックし続けるので別スレッド必須）
    thread::spawn(move || broker::run(broker_port));

    // ブローカーがポートの待ち受けを終えるまで少し待つ。
    // 同じプロセス内で「ブローカーを起動した直後に、そのブローカーへクライアントとして
    // 接続しにいく」という順番になるため、ブローカーの起動が終わる前に接続を試みて
    // 失敗しないよう、ごく短い時間だけ待ってから次に進む。
    thread::sleep(Duration::from_millis(300));

    // パソコン役の指示出しロジックを、このメインスレッドで実行する
    // （host="127.0.0.1"は、同じプロセス内で起動したブローカー自身を指す）
    controller::run(name, "127.0.0.1".to_string(), broker_port, topic);
}
