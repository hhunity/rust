//! # mqtt-server（パソコン役）の起動処理
//!
//! このファイルはC++でいう`main.cpp`に相当する、実行ファイルの入り口です。
//! 中身はほとんど無く、共通ライブラリ（`mqtt_app`クレート、＝`src/lib.rs`以下）の
//! 関数を呼び出して繋ぎ合わせるだけの薄い層になっています
//! （CMakeのターゲットでいう、ロジックの詰まった静的ライブラリをリンクした、
//! ごく短い`main()`だけのソースファイル、という位置づけです）。
//!
//! やっていることは2つです。
//!   1. MQTTブローカー（[`mqtt_app::broker`]） … マイコンたちとパソコン自身が繋ぐ相手
//!   2. パソコン役の指示出しロジック（[`mqtt_app::controller`]） … チャット・`/send`・`/job`を扱う、
//!      ブローカーに対する「ただの1クライアント」
//! 同じプロセスで両方動かすことで、パソコンにブローカーと指示出しロジックを同居させています。

use std::thread;
use std::time::Duration;

// `mqtt_app::broker` のように、`ライブラリクレート名::モジュール名`で共通コードを取り込みます。
// C++でいう `#include "mqtt_app/broker.h"` に近い感覚ですが、Rustではヘッダファイルを
// 別途書く必要はなく、`src/lib.rs`で`pub mod broker;`と宣言されていれば、
// このファイルからは常にこの1行だけで見えるようになります。
use mqtt_app::{broker, controller};

/// Rustのプログラムは main関数 から実行が始まります（C++と同じです）。
/// C++の`main`と違い、コマンドライン引数は明示的な`argc`/`argv`ではなく、
/// `std::env::args()`という「イテレータ」を通じて取得します。
fn main() {
    // 1番目の引数: ブローカーの待ち受けポート（省略時は1883）。
    // `.nth(1)`が「0番目（実行ファイル名）を飛ばして1番目を取る」に相当し、
    // `.and_then(...)`はC++でいう「Optionalの値があれば変換し、無ければ無いまま」という連鎖処理、
    // `.unwrap_or(1883)`は「値が取れなければ1883を使う」というデフォルト値の指定です。
    let broker_port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1883);
    // 2番目の引数: このパソコン自身のMQTTクライアントID（省略時は"pc"）
    let name = std::env::args().nth(2).unwrap_or_else(|| "pc".to_string());
    // 3番目の引数: チャット・ファイル送信などの基点になるトピック（省略時は"chat"）
    let topic = std::env::args().nth(3).unwrap_or_else(|| "chat".to_string());

    // ブローカーを別スレッドで起動する（broker::runはブロックし続けるので別スレッド必須）。
    // `thread::spawn(move || ...)` はC++の`std::thread(lambda)`と同じですが、`move`により
    // クロージャが使う変数（ここでは`broker_port`）の所有権を新しいスレッドへ完全に渡します。
    thread::spawn(move || broker::run(broker_port));

    // ブローカーがポートの待ち受けを終えるまで少し待つ。
    // 同じプロセス内で「ブローカーを起動した直後に、そのブローカーへクライアントとして
    // 接続しにいく」という順番になるため、ブローカーの起動が終わる前に接続を試みて
    // 失敗しないよう、ごく短い時間だけ待ってから次に進む
    // （C++でいう`std::this_thread::sleep_for(300ms);`と全く同じです）。
    thread::sleep(Duration::from_millis(300));

    // パソコン役の指示出しロジックを、このメインスレッドで実行する
    // （host="127.0.0.1"は、同じプロセス内で起動したブローカー自身を指す）。
    // controller::run はプログラムが終わるまでブロックし続けるので、
    // main関数はこの行で「制御を明け渡す」形になります。
    controller::run(name, "127.0.0.1".to_string(), broker_port, topic);
}
