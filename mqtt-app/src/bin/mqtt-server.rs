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

use clap::Parser;

// コマンドライン引数の形を表す構造体。
//
// `#[derive(Parser)]`を構造体に付けるだけで、`clap`クレートが「この構造体の
// フィールド1つ1つを、`--名前 値`という形の引数に対応させるコード」を自動生成して
// くれます（C++でいう`CLI11`の`app.add_option(...)`を1つずつ手で書く代わりに、
// 構造体を1つ書けば済む、というイメージです）。
//
// 各フィールドの`///`コメント（ドキュメントコメント）は、`--help`を実行したときの
// 説明文としてそのまま使われます（普通の`//`コメントと違い、こちらはツールに
// 読み取られる特別なコメントです。ここで構造体自体に`//`を使っているのは、
// この説明文が`--help`の出力に混ざってしまわないようにするためです）。
#[derive(Parser)]
#[command(about = "パソコン役：MQTTブローカーと、チャット/ファイル送信/ジョブ配信の指示出しロジックを1つに同居させた実行ファイル")]
struct Args {
    /// ブローカーの待ち受けポート
    #[arg(short, long, default_value_t = 1883)]
    port: u16,

    /// このパソコン自身のMQTTクライアントID
    #[arg(short, long, default_value = "pc")]
    name: String,

    /// チャット・ファイル送信などの基点になるトピック
    #[arg(short, long, default_value = "chat")]
    topic: String,

    /// ログの出力先ファイル（省略時は標準エラー出力）
    #[arg(short, long)]
    log_file: Option<String>,

    /// 印刷ジョブキューの永続化先ファイル（無ければ新規作成し、あれば前回の続きから再開する）
    #[arg(short, long, default_value = "job_queue.json")]
    queue_file: String,
}

/// Rustのプログラムは main関数 から実行が始まります（C++と同じです）。
fn main() {
    // `Args::parse()`が、実際にコマンドライン引数を読み取って`Args`構造体を組み立てます。
    // 引数が足りない・型が合わない（例: `--port abc`）場合は、`clap`が分かりやすい
    // エラーメッセージを表示してプログラムを終了してくれます（自分でエラー処理を書く
    // 必要がありません）。`--help`を付けて実行すると、使い方が自動で表示されます。
    let args = Args::parse();

    // ログ出力の仕組み（`log`クレート）を初期化する。これを呼ばないと、コード中の
    // `log::info!`などは何も出力されない（C++でいう、ロガーライブラリを使う前に
    // 一度だけ`spdlog::init()`のような初期化を呼ぶのと同じ）。
    // 環境変数`RUST_LOG=mqtt_app=info`を指定して起動すると、MQTTのpublish/受信ログが
    // 見えるようになる（`rumqttd`など依存クレートの内部ログを混ぜたくないので、
    // クレート名を指定する。詳しくは[`mqtt_app::mqtt_log`]参照）。
    mqtt_app::mqtt_log::init_logger(args.log_file.as_deref());

    // ブローカーを別スレッドで起動する（broker::runはブロックし続けるので別スレッド必須）。
    // `thread::spawn(move || ...)` はC++の`std::thread(lambda)`と同じですが、`move`により
    // クロージャが使う変数の所有権を新しいスレッドへ完全に渡します。
    let broker_port = args.port;
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
    controller::run(args.name, "127.0.0.1".to_string(), args.port, args.topic, args.queue_file);
}
