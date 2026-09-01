# mqtt-client

[`rumqttc`](https://crates.io/crates/rumqttc) を使った、単体で動くMQTTチャットクライアントです。
`mqtt-server` プロジェクトとは独立しており、単独でビルド・実行できます（接続先のMQTTブローカーは別途必要です）。

## 使い方

```sh
cargo run -- <名前> [host] [port] [topic]

# 例
cargo run -- alice 127.0.0.1 1883 chat
cargo run -- bob   127.0.0.1 1883 chat
```

`host`・`port`・`topic` は省略可能で、それぞれ `127.0.0.1` / `1883` / `chat` がデフォルトです。

起動後、行を入力して Enter を押すと `<名前>: <入力内容>` が指定トピックに publish され、
同じトピックを subscribe している全クライアント（自分自身も含む）に配信されて表示されます。
`Ctrl+D` で送信スレッドを終了できます。

同じホスト上でMQTTブローカーを動かしたい場合は、`mqtt-server` プロジェクトを使ってください。
