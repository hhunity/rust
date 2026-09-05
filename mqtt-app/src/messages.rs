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
//! ## トピックとメッセージ種別の対応（Sparkplug Bのmessage_type名をそのまま使用）
//!
//! このプロジェクトのトピックは`<topic>/<message_type>/<マイコンの名前>`という形で、
//! `message_type`にはSparkplug B本家の名前をそのまま使っています（詳しくは`mqtt-app`の
//! READMEを参照）。
//!
//! - `NBIRTH` … マイコンが接続した（起動）ことの通知
//! - `NDEATH` … マイコンが切断した（終了）ことの通知（Last Willで代理publishされる）
//! - `NDATA`  … マイコンからの、動作中の継続的な報告（ACK・受信結果・ジョブ完了など）
//! - `NCMD`   … ホスト（パソコン）からマイコンへの命令（ファイル送信の申し出・ジョブ配信）
//!
//! `NBIRTH`/`NDEATH`はトピック自体が意味（オンライン/オフライン）を語るので、ペイロードは
//! `seq`だけの[`BirthDeathMsg`]です。一方`NDATA`/`NCMD`は複数種類のメッセージが同じ
//! トピックに乗るので、JSON自身に`"type"`フィールドを持たせて中身を区別しています。
//! それを表すのが下の`CmdMsg`・`DataMsg`という2つの`enum`です。

use serde::{Deserialize, Serialize};

/// `NCMD`（`<topic>/NCMD/<宛先の名前>`）に乗る、ホスト（パソコン）から特定/全マイコンへの命令。
///
/// `enum`の各バリアント（`FileOffer`・`Job`）は、中に別々の型（`OfferMsg`・`JobMsg`）の
/// データを持てます。これはC++でいう`std::variant<OfferMsg, JobMsg>`にかなり近いもので、
/// 「今どちらが入っているか」を`match`で必ず全パターン確認させられる点も同じです
/// （C++の`std::variant`を`std::visit`で扱うときの「全パターン網羅を強制される」感覚です）。
///
/// これはSparkplug B本家の`NCMD`が「具体的な命令の種類ごとにトピックを分けず、
/// ペイロード側（本家ではメトリクス、本プロジェクトでは`"type"`タグ）で区別する」という
/// 設計をそのまま踏襲したものです。
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

/// `NDATA`（`<topic>/NDATA/<自分の名前>`）に乗る、マイコン自身の動作中の継続的な報告。
/// ホスト側は`<topic>/NDATA/+`のようにワイルドカード購読して、全マイコンの報告を
/// まとめて受け取る（`CmdMsg`と同様、中身の判別は`"type"`フィールドで行う）。
///
/// 接続・切断そのものの通知は`NDATA`ではなく[`BirthDeathMsg`]（`NBIRTH`/`NDEATH`）が
/// 別途担当するので、ここには含まれない。
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DataMsg {
    FileAck(AckMsg),
    FileReceived(ReceivedMsg),
    JobDone(DoneMsg),
}

/// `NBIRTH`・`NDEATH`の中身。
///
/// オンラインかオフラインかは**トピック自体**（`NBIRTH`か`NDEATH`か）が表しているので、
/// ペイロードには持たせていない（トピックとペイロードで同じ情報を重複させない、という
/// このプロジェクト全体の方針に合わせている）。残るのは欠落検知用の`seq`だけ。
#[derive(Serialize, Deserialize, Debug)]
pub struct BirthDeathMsg {
    pub seq: u64,
}

/// `CmdMsg::FileOffer`の中身。「このファイルを送りたい」という申し出。
///
/// 送り先のマイコン名はトピック（`<topic>/NCMD/<宛先>`）自体が表しているので、
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

/// `CmdMsg::Job`の中身。`<topic>/NCMD/all`に乗る、全マイコンへの一斉配信ジョブ。
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
/// （`<topic>/NDATA/<自分の名前>`）自体が表しているので、ペイロードには持たせていない。
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

/// `DataMsg::JobDone`の中身。ジョブの完了報告。
#[derive(Serialize, Deserialize, Debug)]
pub struct DoneMsg {
    pub id: String,
    pub seq: u64,
}

/// `<topic>/STATE/<パソコンの名前>`の中身（Sparkplug Bの`STATE`に相当）。
///
/// マイコンと違い、パソコン（ホストアプリケーション）はオンライン/オフラインの
/// 2状態しか無く、`NBIRTH`のような詳しい起動証明書は本家でも作らないので、
/// トピックを分けず`status`フィールド1つで表す、という本家と同じ簡易な作りにしている。
#[derive(Serialize, Deserialize, Debug)]
pub struct PresenceMsg {
    /// "online" か "offline"
    pub status: String,
    pub seq: u64,
}
