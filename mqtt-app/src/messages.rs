//! # MQTTでやり取りするメッセージの形（＝通信プロトコルの定義）
//!
//! ここに書かれている構造体（`struct`）が、パソコン役とマイコン役の間でMQTT越しに
//! やり取りされるメッセージの中身（ペイロード）です。C++でいうと、通信用の構造体を
//! `struct`で定義して、`nlohmann::json`のようなライブラリでシリアライズ/デシリアライズ
//! するのに近いイメージです。
//!
//! `#[derive(Serialize, Deserialize)]`という行が各構造体の上に付いていますが、これは
//! 「この構造体をJSON文字列に変換する処理／JSON文字列からこの構造体を組み立てる処理を、
//! コンパイラに自動生成させる」という指示です（Rustでは"derive"＝導出、と呼びます）。
//! C++にはこの仕組みが標準では無いので、`nlohmann::json`を使うときのように
//! `NLOHMANN_DEFINE_TYPE_INTRUSIVE`マクロを書いたり、手で`to_json`/`from_json`を
//! 書いたりする代わりに、Rustでは属性を1行付けるだけで済みます。
//!
//! それぞれの構造体のフィールドには`pub`が付いています。C++の`struct`はデフォルトで
//! 全メンバがpublicですが、Rustの`struct`はC++の`class`同様デフォルトが非公開（private）
//! なので、外から読み書きしたいフィールドには明示的に`pub`を付ける必要があります。

use serde::{Deserialize, Serialize};

/// `<topic>/file/offer/<宛先名>` のペイロード。「このファイルを送りたい」という申し出。
///
/// seqは、Sparkplug B（産業IoT向けMQTT規約）の考え方を参考にした連番。
/// このクライアントが何かをpublishするたびに1ずつ増える値で、受信側はこれを見て
/// 「間の1通が届いていない（抜けている）」ことに気付けるようにする（詳しくは[`crate::seq`]参照）。
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
