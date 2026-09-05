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
//!
//! ## トピックとメッセージ種別の対応（Sparkplug Bを参考にしたCMD/DATAの分け方）
//!
//! このプロジェクトのトピックは、`<topic>/cmd/<マイコンの名前>`と`<topic>/data/<マイコンの
//! 名前>`の2種類が中心です（詳しくは`mqtt-app`のREADMEを参照）。1つのトピックに
//! 複数種類のメッセージが乗るので、「これはどの種類のメッセージか」をJSON自身に
//! `"type"`フィールドとして持たせています。それを表すのが下の`CmdMsg`・`DataMsg`という
//! 2つの`enum`です。

use serde::{Deserialize, Serialize};

/// `<topic>/cmd/<宛先の名前>`に乗る、ホスト（パソコン）から特定/全マイコンへの命令。
///
/// `enum`の各バリアント（`FileOffer`・`Job`）は、中に別々の型（`OfferMsg`・`JobMsg`）の
/// データを持てます。これはC++でいう`std::variant<OfferMsg, JobMsg>`にかなり近いもので、
/// 「今どちらが入っているか」を`match`で必ず全パターン確認させられる点も同じです
/// （C++の`std::variant`を`std::visit`で扱うときの「全パターン網羅を強制される」感覚です）。
///
/// `#[serde(tag = "type", rename_all = "snake_case")]`は、JSONにしたときに
/// `{"type": "file_offer", ...}`や`{"type": "job", ...}`のように、バリアント名を
/// 判別用の`"type"`フィールドとして埋め込む指定です（C言語で「共用体＋種別を表す
/// enumメンバ」をタグ付きunionとして手作りするのを、コンパイラに任せるイメージです）。
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CmdMsg {
    FileOffer(OfferMsg),
    Job(JobMsg),
}

/// `<topic>/data/<自分の名前>`に乗る、マイコン自身が自分について報告するメッセージ。
/// ホスト側は`<topic>/data/+`のようにワイルドカード購読して、全マイコンの報告を
/// まとめて受け取る（`CmdMsg`と同様、中身の判別は`"type"`フィールドで行う）。
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DataMsg {
    Presence(PresenceMsg),
    FileAck(AckMsg),
    FileReceived(ReceivedMsg),
    JobDone(DoneMsg),
}

/// `CmdMsg::FileOffer`の中身。「このファイルを送りたい」という申し出。
///
/// 送り先のマイコン名はトピック（`<topic>/cmd/<宛先>`）自体が表しているので、
/// ペイロードには持たせていない（送り主である`from`＝パソコン役の名前だけを持つ）。
///
/// seqは、Sparkplug B（産業IoT向けMQTT規約）の考え方を参考にした連番。
/// このクライアントが何かをpublishするたびに1ずつ増える値で、受信側はこれを見て
/// 「間の1通が届いていない（抜けている）」ことに気付けるようにする（詳しくは[`crate::seq`]参照）。
#[derive(Serialize, Deserialize, Debug)]
pub struct OfferMsg {
    pub id: String,
    pub from: String,
    pub filename: String,
    pub size: u64,
    pub seq: u64,
}

/// `CmdMsg::Job`の中身。`<topic>/cmd/all`に乗る、全マイコンへの一斉配信ジョブ。
/// 「全員」という宛先はトピック自体（`all`という特別な名前）が表している。
#[derive(Serialize, Deserialize, Debug)]
pub struct JobMsg {
    pub id: String,
    /// このジョブを配信した（＝パソコン役の）名前
    pub from: String,
    pub content: String,
    pub seq: u64,
}

/// `DataMsg::FileAck`の中身。「ここ(host:port)に繋いで」という返事。
///
/// これを送っている（＝ファイルを受け取る）マイコンの名前はトピック
/// （`<topic>/data/<自分の名前>`）自体が表しているので、ペイロードには持たせていない。
#[derive(Serialize, Deserialize, Debug)]
pub struct AckMsg {
    pub id: String,
    pub host: String,
    pub port: u16,
    pub seq: u64,
}

/// `DataMsg::FileReceived`の中身。生TCP転送が終わった後の結果報告。
#[derive(Serialize, Deserialize, Debug)]
pub struct ReceivedMsg {
    pub id: String,
    /// "ok" か "failed"
    pub status: String,
    pub size: u64,
    pub seq: u64,
}

/// `DataMsg::Presence`の中身。
#[derive(Serialize, Deserialize, Debug)]
pub struct PresenceMsg {
    /// "online" か "offline"
    pub status: String,
    pub seq: u64,
}

/// `DataMsg::JobDone`の中身。ジョブの完了報告。
#[derive(Serialize, Deserialize, Debug)]
pub struct DoneMsg {
    pub id: String,
    pub seq: u64,
}
