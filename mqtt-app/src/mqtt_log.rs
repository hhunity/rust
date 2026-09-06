//! # MQTTのpublish/受信を、毎回ログに出すための共通ヘルパー
//!
//! `println!`ではなく`log`クレートのマクロ（`log::info!`など）を使っています。
//! `println!`は常に出力されてしまいますが、`log`マクロは環境変数`RUST_LOG`で
//! 「今回はどのレベル以上のログだけ見たいか」を実行時に選べます（C++でいう、
//! `spdlog::set_level()`やログレベルをコマンドライン/環境変数で切り替える仕組みと
//! 同じ発想です）。
//!
//! 使い方（実行時に環境変数を指定する）:
//! ```sh
//! RUST_LOG=mqtt_app=info cargo run --bin mqtt-server
//! ```
//! 何も指定しなければ、このプロジェクトのログは表示されません（デフォルトが
//! `error`レベルだけを表示する設定になっているため）。
//!
//! `RUST_LOG=info`のように**クレート名を付けずに**指定すると、依存クレートである
//! `rumqttd`（ブローカー本体）の内部ログまで大量に表示されてしまうので注意してください。
//! `mqtt_app=info`のように**自分のクレート名だけを指定する**のがおすすめです。
//!
//! ## ログをファイルへ書き出す
//!
//! `mqtt-server`・`mqtt-client`はどちらも、最後の引数として**ログの出力先ファイル**を
//! 受け付けます（省略すれば、これまで通り標準エラー出力に表示されます）。詳しくは
//! `mqtt-app`のREADMEを参照してください。

/// ログ出力の仕組み（`log`クレート）を初期化する。
///
/// `log_file`に`Some(path)`が渡されたら、そのファイルへログを書き出す（無ければ
/// 新規作成する。既にある場合は中身を空にしてから書き込む）。`None`ならこれまで通り
/// 標準エラー出力（stderr）に書き出す。
///
/// `env_logger::Builder`はC++でいう「ロガーの設定を組み立てるビルダーオブジェクト」に
/// 近いもので、`.target(...)`のような「メソッドチェーン」で設定を1つずつ足していき、
/// 最後に`.init()`を呼んで確定させる、という使い方をします。
pub fn init_logger(log_file: Option<&str>) {
    let mut builder = env_logger::Builder::from_default_env();
    if let Some(path) = log_file {
        let file = std::fs::File::create(path)
            .unwrap_or_else(|e| panic!("ログファイル{path}の作成に失敗しました: {e}"));
        builder.target(env_logger::Target::Pipe(Box::new(file)));
    }
    builder.init();
}

/// MQTTへpublishするときに、送信内容を1行ログに出す。
///
/// `payload`はバイト列（`&[u8]`）で渡ってきますが、今回のプロジェクトの中身は
/// 全部JSON（またはプレーンテキスト）なので、そのまま文字列として表示しています。
/// `String::from_utf8_lossy`は「UTF-8として正しくない部分があっても、エラーにせず
/// 代わりの文字（`�`）に置き換えて表示する」という安全な変換です（C++で言うところの、
/// 不正なバイト列を無理やり`std::string`にキャストして未定義動作を起こす、という
/// ことが起きないようになっています）。
pub fn log_publish(topic: &str, payload: &[u8]) {
    log::info!("[MQTT送信] topic={topic} payload={}", String::from_utf8_lossy(payload));
}

/// MQTTから受信したときに、受信内容を1行ログに出す。
pub fn log_receive(topic: &str, payload: &[u8]) {
    log::info!("[MQTT受信] topic={topic} payload={}", String::from_utf8_lossy(payload));
}
