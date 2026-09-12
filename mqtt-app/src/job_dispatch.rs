//! # 印刷ジョブキューを1件だけ処理する
//!
//! 以前はバックグラウンドスレッドが起動時から自動でキューを監視し続け、Pendingジョブが
//! あれば（誰もオンラインでなければ誰か来るまで待ってでも）勝手に配信していた。しかし
//! 「起動時に未処理ジョブを勝手に投げない」という要望に合わせて、常駐スレッド・自動ループは
//! 廃止し、[`run_one`]が`run`コマンドから呼ばれたときだけ、その場でキューの先頭にある
//! Pendingジョブを1件処理する形に変えた。
//!
//! 呼び出しはREPLのコマンドコールバック（同じスレッド）から行われるため、完了報告を
//! 待つ間（最大[`JOB_TIMEOUT`]）はその場でブロックする。これは`job_worker`だった頃に
//! 別スレッドで待っていたのと同じ待ち方を、呼び出し元のスレッドでそのまま行うだけの違い。

use std::collections::HashSet;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rumqttc::{Client, QoS};

use crate::controller::{InFlightJob, InFlightState, Roster};
use crate::job_queue::JobQueue;
use crate::messages::{CmdMsg, JobMsg};
use crate::mqtt_log;
use crate::seq::{next_seq, ControllerSeqState};

/// ジョブを送ってから、完了報告が来ないマイコンを「失敗」と判断するまでの待ち時間。
const JOB_TIMEOUT: Duration = Duration::from_secs(10);

/// キューの先頭にあるPendingジョブを1件処理する。結果を人間向けの1メッセージとして返す
/// （呼び出し元の`run`コマンドがそのままREPLの応答として表示する）。
///
/// - キューにPendingジョブが無ければ、何もせずその旨を返す。
/// - 宛先にできるマイコンが1台もオンラインでなければ、配信はせずその旨を返す
///   （以前のように「誰か来るまで待つ」ことはしない。`run`はブロックしない設計にしている）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_one(
    client: &Client,
    name: &str,
    all_cmd_topic: &str,
    roster: &Roster,
    inflight: &InFlightState,
    seq: &ControllerSeqState,
    queue: &JobQueue,
) -> String {
    let Some(job) = queue.peek_next_pending() else {
        return "キューに未処理のジョブはありません".to_string();
    };

    let targets: HashSet<String> = roster
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, &online)| online)
        .map(|(who, _)| who.clone())
        .collect();
    if targets.is_empty() {
        return format!(
            "ジョブ{}: 今オンラインのマイコンがいないため配信できません（ジョブはPendingのまま。後でもう一度runしてください）",
            job.id
        );
    }

    queue.mark_dispatched(&job.id);

    let (tx, rx) = mpsc::channel::<String>();
    *inflight.lock().unwrap() = Some(InFlightJob { id: job.id.clone(), tx });

    let msg = JobMsg {
        id: job.id.clone(),
        from: name.to_string(),
        content: job.content.clone(),
        seq: next_seq(&seq.job_counter),
    };
    let payload = serde_json::to_vec(&CmdMsg::Job(msg)).unwrap();
    mqtt_log::log_publish(all_cmd_topic, &payload);
    client.publish(all_cmd_topic, QoS::AtLeastOnce, false, payload).unwrap();

    let mut remaining = targets.clone();
    let deadline = Instant::now() + JOB_TIMEOUT;
    while !remaining.is_empty() {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match rx.recv_timeout(deadline - now) {
            Ok(who) => {
                remaining.remove(&who);
            }
            Err(_) => break, // タイムアウト（これ以上待っても来ない）
        }
    }
    *inflight.lock().unwrap() = None; // 待つのをやめたので、共有状態も片付ける

    if remaining.is_empty() {
        queue.mark_done(&job.id);
        format!(
            "ジョブ{}を{}台のマイコン({targets:?})へ配信し、全員完了しました",
            job.id,
            targets.len()
        )
    } else {
        queue.mark_failed(&job.id);
        format!(
            "ジョブ{}は次のマイコンから応答がありませんでした: {remaining:?}",
            job.id
        )
    }
}
