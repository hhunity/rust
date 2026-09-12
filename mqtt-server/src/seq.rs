//! メッセージの連番(seq)を発行・検証するための仕組み。
//!
//! このファイルも`mqtt-client`プロジェクトの`seq.rs`と考え方は同じだが、パソコン役
//! （このプロジェクト）とマイコン役では「自分が送るメッセージ種類」と「受け取る
//! メッセージ種類」が違う（非対称）ので、`ControllerSeqState`/`DeviceSeqState`という
//! 別々の構造体にして、各プロジェクトが本当に使うカウンタ/トラッカーだけを持つようにしている。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 次に使うseq番号を発行するためのカウンタ。
///
/// 重要: **メッセージの種類（OFFERかJOBかなど）ごとに別々のSeqCounterを用意する**必要がある。
/// 理由は、例えば`ACK`は「申し出た本人だけ」に届く専用トピックで、他の観測者には
/// そもそも見えないメッセージだから。もし全種類で1つの共有カウンタを使うと、
/// ACKの分だけ他の観測者から見て番号が「飛んで」見えてしまい、実際は何も欠落していないのに
/// 誤って警告が出てしまう（実際に開発中にこの誤検知が起きたことがある）。
/// あるメッセージ種類を購読している人には、その種類のメッセージは必ず全部見えるはずなので、
/// 種類ごとに分ければ正しく欠落を検知できる。
pub type SeqCounter = Arc<Mutex<u64>>;

/// 相手の名前ごとに「最後に見たseq番号」を覚えておく辞書。次に来たメッセージのseqと比べて、
/// 1つ飛んでいたら「間の1通が抜けたかもしれない」と分かる。
/// SeqCounterと同様、**メッセージの種類ごとに別々のSeqTrackerを使う**。
pub type SeqTracker = Arc<Mutex<HashMap<String, u64>>>;

pub fn new_counter() -> SeqCounter {
    Arc::new(Mutex::new(0))
}

pub fn new_tracker() -> SeqTracker {
    Arc::new(Mutex::new(HashMap::new()))
}

/// SeqCounterから「次に使うseq番号」を1つ取り出し、内部のカウンタを1つ進める。
pub fn next_seq(counter: &SeqCounter) -> u64 {
    let mut n = counter.lock().unwrap();
    let seq = *n;
    *n += 1;
    seq
}

/// 受信したメッセージのseq番号を確認し、直前に見た値から1つも増えていなければ
/// （＝間の番号が抜けていれば）警告を表示する。呼び出す側は、メッセージの種類に対応する
/// 専用のSeqTrackerを渡すこと（他の種類のトラッカーと混ぜて使わない）。
///
/// - 初めて見る名前の場合は比較のしようがないので、警告なしでそのまま記録するだけにする
/// - is_birthがtrueのとき（presenceの"online"、Sparkplug BでいうBIRTH）は、再接続で
///   カウンタが0から数え直されているのが正常なので、ここでも警告なしで記録し直す
pub fn check_seq(from: &str, seq: u64, tracker: &SeqTracker, is_birth: bool) {
    let mut last_seen = tracker.lock().unwrap();
    if !is_birth {
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

/// パソコン役（このプロジェクト）が使うseq状態。
/// パソコン役は「OFFERとJOBを送る側」「ACK・RECEIVED・presence・DONEを受け取る側」なので、
/// その組み合わせだけを持つ。
#[derive(Clone)]
pub struct ControllerSeqState {
    /// 自分が送るOFFERのseqカウンタ
    pub offer_counter: SeqCounter,
    /// 受け取るACKの欠落検知用トラッカー
    pub ack_tracker: SeqTracker,
    /// 受け取るRECEIVEDの欠落検知用トラッカー
    pub received_tracker: SeqTracker,
    /// 受け取るpresenceの欠落検知用トラッカー
    pub presence_tracker: SeqTracker,
    /// 自分が送るJOBのseqカウンタ
    pub job_counter: SeqCounter,
    /// 受け取るDONEの欠落検知用トラッカー
    pub done_tracker: SeqTracker,
}

impl ControllerSeqState {
    pub fn new() -> Self {
        ControllerSeqState {
            offer_counter: new_counter(),
            ack_tracker: new_tracker(),
            received_tracker: new_tracker(),
            presence_tracker: new_tracker(),
            job_counter: new_counter(),
            done_tracker: new_tracker(),
        }
    }
}
