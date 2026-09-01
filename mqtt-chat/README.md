# mqtt-chat

RustでMQTTを使った簡単な文字チャットのサンプルです。

- `mqtt-server`: [`rumqttd`](https://crates.io/crates/rumqttd) を使った組み込みMQTTブローカー（サーバー）
- `mqtt-client`: [`rumqttc`](https://crates.io/crates/rumqttc) を使ったMQTTクライアント（標準入力から送信、受信メッセージを表示）

## 使い方

### 1. サーバーを起動

```sh
cargo run -p mqtt-server            # 0.0.0.0:1883 で待ち受け
cargo run -p mqtt-server -- 1884    # ポートを変えたい場合
```

### 2. クライアントを起動（複数の端末から）

```sh
cargo run -p mqtt-client -- <名前> [host] [port] [topic]

# 例
cargo run -p mqtt-client -- alice 127.0.0.1 1883 chat
cargo run -p mqtt-client -- bob   127.0.0.1 1883 chat
```

`host`・`port`・`topic` は省略可能で、それぞれ `127.0.0.1` / `1883` / `chat` がデフォルトです。

起動後、行を入力して Enter を押すと `<名前>: <入力内容>` が `chat` トピックに publish され、
同じトピックを subscribe している全クライアント（自分自身も含む）に配信されて表示されます。
`Ctrl+D` で送信スレッドを終了できます。
