//! # メッセージの連番(seq)を発行・検証する仕組み
//!
//! MQTTは「届いた順番」や「1通も欠けずに届いたか」を自動では保証してくれません
//! （QoSを上げれば再送はされますが、それでも「何番目まで届いたか」はアプリ側で
//! 管理する必要があります）。そこでこのプロジェクトでは、Sparkplug B（産業IoT向けの
//! MQTT規約）を参考に、メッセージ1通ごとに連番(`seq`)を振り、受け取る側が
//! 「前回より1つも増えていない＝間の1通が抜けたかもしれない」と気付けるようにしています。
//!
//! ## `Arc<Mutex<T>>` について（C++経験者向け）
//!
//! この下で出てくる`Arc<Mutex<u64>>`という型は、C++でいうと
//! `std::shared_ptr<std::mutex_wrapped<uint64_t>>`のようなものです。分解すると:
//!
//! - `Mutex<u64>` … `u64`（符号なし64bit整数）を、排他ロックで保護した箱。
//!   C++の`std::mutex`と違い、**ロックを取って返ってくる「ガード」経由でしか中身の`u64`に
//!   触れない**ようにコンパイラが強制します（`.lock().unwrap()`でロックを取得すると、
//!   その戻り値を通してしか中身をいじれません。ロックを取り忘れて直接アクセス…という
//!   C++でありがちなミスが、Rustではそもそも構文的にできません）
//! - `Arc<T>` … `std::shared_ptr<T>`と同じ、参照カウント方式の共有ポインタ。
//!   複数のスレッドから同じ`Mutex`を指せるようにするために被せています
//!   （`Arc`の"A"はAtomic＝参照カウントの増減がアトミック操作＝スレッドセーフ、という意味）

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 次に使うseq番号を発行するためのカウンタ。
///
/// 型エイリアス（`type X = Y;`）は、C++の`using X = Y;`（またはC言語の`typedef`）と同じ、
/// 長い型に短い名前を付けるための機能です。
///
/// 重要: **同じMQTTトピックを共有するメッセージ群ごとに、1つのSeqCounterを用意する**
/// 必要があります。理由は、あるトピックを購読している人には、そこに流れるメッセージが
/// （中身の種類が違っても）必ず全部見えるはずだからです。逆に、別のトピックへ流れる
/// メッセージ（例えば他のマイコン宛てのOFFER）は見えないので、それを同じカウンタに
/// 混ぜてしまうと、番号が「飛んで」見えて誤って警告を出してしまいます（実際に開発中に
/// この誤検知が起きたことがあります）。
///
/// このプロジェクトのトピックは3種類（`<topic>/<名前>/cmd`・`<topic>/all/cmd`・
/// `<topic>/<名前>/data`）なので、それぞれに対応するカウンタ／トラッカーを用意します
/// （`<topic>/<名前>/data`には元々presence・ACK・RECEIVED・DONEの4種類のメッセージが
/// 乗りますが、全部同じトピックに乗る＝観測者からは全部見えるので、まとめて1つの
/// カウンタで構いません。これはSparkplug B本家が「ノード1つにつきseqは1系列」と
/// している設計にも合わせた形です）。
pub type SeqCounter = Arc<Mutex<u64>>;

/// 相手の名前ごとに「最後に見たseq番号」を覚えておく辞書。
///
/// `HashMap<K, V>`はC++の`std::unordered_map<K, V>`と同じ、ハッシュテーブルです。
/// 次に来たメッセージのseqと比べて、1つ飛んでいたら「間の1通が抜けたかもしれない」と分かります。
/// SeqCounterと同様、**トピックごとに別々のSeqTrackerを使う**のがポイントです。
pub type SeqTracker = Arc<Mutex<HashMap<String, u64>>>;

/// 中身が0のSeqCounterを新しく作るための、ちょっとしたヘルパー関数。
/// C++でいう「デフォルト値付きのファクトリ関数」のようなものです。
pub fn new_counter() -> SeqCounter {
    Arc::new(Mutex::new(0))
}

/// 空のSeqTrackerを新しく作るためのヘルパー関数。
pub fn new_tracker() -> SeqTracker {
    Arc::new(Mutex::new(HashMap::new()))
}

/// SeqCounterから「次に使うseq番号」を1つ取り出し、内部のカウンタを1つ進める。
///
/// `counter.lock().unwrap()`の`.lock()`は、C++の`std::mutex::lock()`に相当しますが、
/// 戻り値（ロック中しか使えない「ガード」オブジェクト）を経由してしか中の`u64`を
/// 読み書きできない点がC++と異なります。`.unwrap()`は、「ロック中に他のスレッドが
/// パニック（C++でいう`std::terminate`相当の異常終了）していたら、ここでもエラーに
/// する」という処理で、通常の運用では起きないケースなので握りつぶさずそのまま
/// 異常終了させています。
pub fn next_seq(counter: &SeqCounter) -> u64 {
    let mut n = counter.lock().unwrap();
    let seq = *n; // *n はC++の *ptr と同じ「中身を読む」操作（ガード越しにu64を読む）
    *n += 1;
    seq
} // ここでガード(n)がスコープを抜けて自動的に破棄され、ロックが解放される
  // （C++のstd::lock_guard/std::unique_lockと同じ、RAIIによる自動アンロック）

/// 受信したメッセージのseq番号を確認し、直前に見た値から1つも増えていなければ
/// （＝間の番号が抜けていれば）警告を表示する。呼び出す側は、そのトピックに対応する
/// 専用のSeqTrackerを渡すこと（他のトピックのトラッカーと混ぜて使わない）。
///
/// - 初めて見る名前の場合は比較のしようがないので、警告なしでそのまま記録するだけにする
/// - is_birthがtrueのとき（presenceの"online"、Sparkplug BでいうBIRTH）は、再接続で
///   カウンタが0から数え直されているのが正常なので、ここでも警告なしで記録し直す
pub fn check_seq(from: &str, seq: u64, tracker: &SeqTracker, is_birth: bool) {
    let mut last_seen = tracker.lock().unwrap();
    if !is_birth {
        // if let Some(&last) = ... は、C++でいう
        //   auto it = map.find(from);
        //   if (it != map.end()) { uint64_t last = it->second; ... }
        // を1行にまとめたような書き方（パターンマッチ）です。
        if let Some(&last) = last_seen.get(from) {
            if seq != last + 1 {
                println!(
                    "[system] 警告: {from}からのメッセージが抜けている可能性があります\
                     （前回seq={last}, 今回seq={seq}）"
                );
            }
        }
    }
    last_seen.insert(from.to_string(), seq);
}

/// パソコン役（`mqtt-server`）が使うseq状態。
/// パソコン役は「OFFERとJOBを送る側」「各マイコンのdataトピック（presence・ACK・RECEIVED・
/// DONEをまとめたもの）を受け取る側」なので、その組み合わせだけを持つ。
///
/// `#[derive(Clone)]`は、C++でいうコピーコンストラクタを自動生成する指示に近いですが、
/// 中身が全部`Arc`（＝shared_ptr相当）なので、実際に複製されるのは「参照カウンタの
/// 参照先を指す矢印」だけで、カウンタやトラッカーの中身そのものは複製されず、
/// 複数のスレッドが同じ実体を共有し続けます（shared_ptrをコピーする感覚と同じです）。
#[derive(Clone)]
pub struct ControllerSeqState {
    /// 自分が送るOFFER（`<topic>/<宛先>/cmd`、宛先ごとに別トピック）のseqカウンタ
    pub offer_counter: SeqCounter,
    /// 自分が送るJOB（`<topic>/all/cmd`）のseqカウンタ
    pub job_counter: SeqCounter,
    /// 各マイコンの`<topic>/<名前>/data`の欠落検知用トラッカー
    /// （マイコンの名前ごとに、最後に見たseqを覚えておく）
    pub data_tracker: SeqTracker,
}

impl ControllerSeqState {
    /// `impl 型名 { ... }` は、C++でいう「クラスのメンバ関数をまとめて書く場所」です。
    /// Rustでは構造体の「データの形」（`struct`）と「振る舞い」（`impl`）が分けて
    /// 書かれるのが特徴です。この`new()`は、C++でいうコンストラクタに相当します
    /// （ただしRustには本物のコンストラクタ構文が無く、`new`という名前の普通の
    /// 関連関数（staticメンバ関数）を作るのが単なる「お作法」として定着しています）。
    pub fn new() -> Self {
        ControllerSeqState {
            offer_counter: new_counter(),
            job_counter: new_counter(),
            data_tracker: new_tracker(),
        }
    }
}

/// マイコン役（`mqtt-client`）が使うseq状態。
/// マイコン役は「OFFER・JOBを受け取る側」「自分のdataトピック（presence・ACK・RECEIVED・
/// DONEをまとめたもの）を送る側」なので、パソコン役とはちょうど逆の組み合わせを持つ。
#[derive(Clone)]
pub struct DeviceSeqState {
    /// 受け取るOFFER（`<topic>/<自分の名前>/cmd`）の欠落検知用トラッカー
    pub offer_tracker: SeqTracker,
    /// 受け取るJOB（`<topic>/all/cmd`）の欠落検知用トラッカー
    pub job_tracker: SeqTracker,
    /// 自分が送る`<topic>/<自分の名前>/data`のseqカウンタ
    /// （presence・ACK・RECEIVED・DONEをまとめて、この1本のカウンタを使う）
    pub data_counter: SeqCounter,
}

impl DeviceSeqState {
    pub fn new() -> Self {
        DeviceSeqState {
            offer_tracker: new_tracker(),
            job_tracker: new_tracker(),
            data_counter: new_counter(),
        }
    }
}
