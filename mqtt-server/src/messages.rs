//! パソコン役とマイコン役の間でMQTT上をやり取りするメッセージの形（JSON）を定義するモジュール。
//!
//! このファイルは`mqtt-client`プロジェクトの`messages.rs`と**全く同じ内容**にしてある。
//! 2つのプロジェクトは別々のCargoクレートであり、共有ライブラリは意図的に作っていないので、
//! 通信フォーマットを変えるときは両方のファイルを一緒に直す必要がある。

use serde::{Deserialize, Serialize};

/// `<topic>/file/offer/<宛先名>` のペイロード。「このファイルを送りたい」という申し出。
///
/// seqは、Sparkplug B（産業IoT向けMQTT規約）の考え方を参考にした連番。
/// このクライアントが何かをpublishするたびに1ずつ増える値で、受信側はこれを見て
/// 「間の1通が届いていない（抜けている）」ことに気付けるようにする。
#[derive(Serialize, Deserialize)]
pub struct OfferMsg {
    pub id: String,
    pub from: String,
    pub to: String,
    pub filename: String,
    pub size: u64,
    pub seq: u64,
}

/// `<topic>/file/ack/<申し出た人の名前>` のペイロード。「ここ(host:port)に繋いで」という返事。
#[derive(Serialize, Deserialize)]
pub struct AckMsg {
    pub id: String,
    /// この返事を送っている（＝ファイルを受け取る）側の名前
    pub from: String,
    pub host: String,
    pub port: u16,
    pub seq: u64,
}

/// `<topic>/file/received/<マイコン名>` のペイロード。生TCP転送が終わった後の結果報告。
#[derive(Serialize, Deserialize)]
pub struct ReceivedMsg {
    pub id: String,
    pub who: String,
    /// "ok" か "failed"
    pub status: String,
    pub size: u64,
    pub seq: u64,
}

/// `<topic>/presence/<名前>` のペイロード。
#[derive(Serialize, Deserialize)]
pub struct PresenceMsg {
    /// "online" か "offline"
    pub status: String,
    pub seq: u64,
}

/// `<topic>/job/queue` のペイロード。全マイコンへの一斉配信ジョブ。
#[derive(Serialize, Deserialize)]
pub struct JobMsg {
    pub id: String,
    /// このジョブを配信した（＝パソコン役の）名前
    pub from: String,
    pub content: String,
    pub seq: u64,
}

/// `<topic>/job/done/<マイコン名>` のペイロード。ジョブの完了報告。
#[derive(Serialize, Deserialize)]
pub struct DoneMsg {
    pub id: String,
    pub who: String,
    pub seq: u64,
}
