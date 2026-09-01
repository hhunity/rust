# mqtt-server

[`rumqttd`](https://crates.io/crates/rumqttd) を使った、単体で動くMQTTブローカー（サーバー）です。
`mqtt-client` プロジェクトとは独立しており、単独でビルド・実行できます。

## 使い方

```sh
cargo run            # 0.0.0.0:1883 で待ち受け
cargo run -- 1884    # ポートを変えたい場合
```

受信したメッセージはすべて標準出力にログ表示されます。
