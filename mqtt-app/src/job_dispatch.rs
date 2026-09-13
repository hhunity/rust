//! # 印刷ジョブキューを1件だけ処理する
//!
//! 以前はバックグラウンドスレッドが起動時から自動でキューを監視し続け、Pendingジョブが
//! あれば（誰もオンラインでなければ誰か来るまで待ってでも）勝手に配信していた。しかし
//! 「起動時に未処理ジョブを勝手に投げない」という要望に合わせて、常駐スレッド・自動ループは
//! 廃止した。[`run_one`]は次の2箇所から呼ばれる。
//!
//! - `job`コマンド（[`crate::repl_commands::cmd_job`]）: キューに積んだ直後、その場で
//!   1回だけ配信を試みる。宛先が誰もオンラインでない等の理由で配信できなければ、
//!   ジョブはPendingのまま「止まった」状態になる。
//! - `run`コマンド（[`crate::repl_commands::cmd_run`]）: 止まっている（＝配信できず
//!   Pendingのままの）ジョブを、後から手動で再開する。IDを指定すればそのジョブだけを
//!   名指しで（[`run_one`]）、省略すればPendingジョブを先頭から順に進められるだけ
//!   全部（[`run_all`]）処理する。
//!
//! 起動時にファイルから引き継いだ未処理ジョブは、誰かが`job`か`run`を打つまで
//! 配信されない（勝手に印刷が始まると困る、という要望に合わせている）。
//!
//! [`abort`]は処理中(Dispatched)のジョブを中断させる。中断の完了を待つ必要はなく
//! （実際に止まったかどうかは、待っている側＝`run_one`の完了待ちループに、後から
//! [`crate::controller::JobSignal::Aborted`]として届く）、指示を送るだけですぐ戻る。
//!
//! これらの呼び出しはREPLのコマンドコールバックから行われるが、`job`・`run`の実際の
//! 配信・完了待ちは`repl_commands`側で別スレッドに逃がしているため、待っている間も
//! REPL自体は次のコマンド（`abort`など）を受け付けられる（詳しくは`repl_commands.rs`の
//! `cmd_job`・`cmd_run`のコメント参照）。

use std::collections::HashSet;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rumqttc::{Client, QoS};

use crate::controller::{DeviceStatuses, InFlightJob, InFlightState, JobSignal, Roster};
use crate::job_queue::{JobQueue, JobStatus};
use crate::messages::{AbortMsg, CmdMsg, DeviceState, JobMsg};
use crate::mqtt_log;
use crate::seq::{next_seq, ControllerSeqState};

/// ジョブを送ってから、完了報告が来ないマイコンを「失敗」と判断するまでの待ち時間。
const JOB_TIMEOUT: Duration = Duration::from_secs(10);

/// 今オンラインで、かつ`Idle`状態（印字中・エラー中でない）のマイコン名の集合を返す。
/// ジョブを配信できる宛先は常にこの集合から選ぶ（印字中のマイコンへ重ねて送らない）。
fn idle_targets(roster: &Roster, device_statuses: &DeviceStatuses) -> HashSet<String> {
    let statuses = device_statuses.lock().unwrap();
    roster
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, &online)| online)
        .map(|(who, _)| who.clone())
        .filter(|who| !matches!(statuses.get(who), Some(DeviceState::Printing { .. } | DeviceState::Error { .. })))
        .collect()
}

/// ジョブを1件処理する。結果を人間向けの1メッセージとして返す
/// （呼び出し元の`job`・`run`コマンドがそのままREPLの応答として表示する）。
///
/// `id`が`Some`なら、そのIDのジョブを名指しで処理する（Pending以外の状態なら開始できない
/// 旨を返す）。`None`ならキューの先頭にあるPendingジョブを処理する（`run`を引数無しで
/// 呼んだときの、これまで通りの挙動）。
///
/// - 対象のPendingジョブが無ければ、何もせずその旨を返す。
/// - 既に別のジョブが処理中（`inflight`が埋まっている）なら、配信はせずその旨を返す
///   （同時に処理するジョブは常に1件だけ、という設計を守るための防御）。
/// - 宛先にできるマイコン（オンラインかつIdle）が1台もいなければ、配信はせずその旨を返す
///   （以前のように「誰か来るまで待つ」ことはしない。呼び出し元をブロックしない設計にしている）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_one(
    client: &Client,
    name: &str,
    all_cmd_topic: &str,
    roster: &Roster,
    device_statuses: &DeviceStatuses,
    inflight: &InFlightState,
    seq: &ControllerSeqState,
    queue: &JobQueue,
    id: Option<&str>,
) -> String {
    let job = match id {
        Some(id) => match queue.get(id) {
            Some(job) if job.status == JobStatus::Pending => job,
            Some(job) => {
                return format!(
                    "ジョブ{id}は現在{:?}状態のため開始できません（Pendingのジョブのみ開始できます）",
                    job.status
                )
            }
            None => return format!("ジョブ{id}は見つかりません"),
        },
        None => {
            let Some(job) = queue.peek_next_pending() else {
                return "キューに未処理のジョブはありません".to_string();
            };
            job
        }
    };

    if inflight.lock().unwrap().is_some() {
        return format!("既に処理中のジョブがあります。完了を待つか、abortしてください（ジョブ{}はPendingのまま）", job.id);
    }

    let targets = idle_targets(roster, device_statuses);
    if targets.is_empty() {
        return format!(
            "ジョブ{}: 宛先にできるマイコン（オンラインかつアイドル）がいないため配信できません（ジョブはPendingのまま。後でもう一度runしてください）",
            job.id
        );
    }

    queue.mark_dispatched(&job.id);

    let (tx, rx) = mpsc::channel::<JobSignal>();
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
    let mut aborted = false;
    while !remaining.is_empty() {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match rx.recv_timeout(deadline - now) {
            Ok(JobSignal::Done(who)) => {
                remaining.remove(&who);
            }
            Ok(JobSignal::Aborted) => {
                aborted = true;
                break;
            }
            Err(_) => break, // タイムアウト（これ以上待っても来ない）
        }
    }
    *inflight.lock().unwrap() = None; // 待つのをやめたので、共有状態も片付ける

    if aborted {
        queue.mark_aborted(&job.id);
        format!("ジョブ{}は中断されました", job.id)
    } else if remaining.is_empty() {
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

/// 止まっている（Pendingの）ジョブを、先頭から順に進められるだけ全部処理する
/// （`run`をIDを指定せずに呼んだときの動作）。
///
/// 1件ごとの結果メッセージのリストを返す。あるジョブが「配信できる宛先がいない」
/// 理由で処理できなかった場合、そのジョブはPendingのまま状態が変わらないので、
/// それ以上ループを続けても同じ結果を繰り返すだけになる。また、あるジョブが`abort`で
/// 中断された場合も、それ以上は利用者の意図に反する可能性があるのでそこで打ち切る。
/// どちらの場合も、残りのPendingジョブは次に`run`を呼んだときのために残る。
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_all(
    client: &Client,
    name: &str,
    all_cmd_topic: &str,
    roster: &Roster,
    device_statuses: &DeviceStatuses,
    inflight: &InFlightState,
    seq: &ControllerSeqState,
    queue: &JobQueue,
) -> Vec<String> {
    let mut messages = Vec::new();
    loop {
        let Some(job) = queue.peek_next_pending() else {
            break;
        };
        messages.push(run_one(
            client,
            name,
            all_cmd_topic,
            roster,
            device_statuses,
            inflight,
            seq,
            queue,
            Some(&job.id),
        ));

        // Pendingのまま（誰も宛先にできなかった）か、Abortedになった場合はそこで打ち切る。
        let stop = queue
            .get(&job.id)
            .map(|j| matches!(j.status, JobStatus::Pending | JobStatus::Aborted))
            .unwrap_or(true);
        if stop {
            break;
        }
    }
    if messages.is_empty() {
        messages.push("キューに未処理のジョブはありません".to_string());
    }
    messages
}

/// 処理中(Dispatched)のジョブを中断させる。指示を送るだけで、実際に止まるのを待たない
/// （待っているのは、そのジョブを配信した`run_one`の完了待ちループの方）。
pub(crate) fn abort(
    client: &Client,
    name: &str,
    all_cmd_topic: &str,
    seq: &ControllerSeqState,
    queue: &JobQueue,
    id: &str,
) -> String {
    match queue.get(id) {
        Some(job) if job.status == JobStatus::Dispatched => {
            let msg = AbortMsg {
                id: job.id.clone(),
                from: name.to_string(),
                seq: next_seq(&seq.job_counter),
            };
            let payload = serde_json::to_vec(&CmdMsg::Abort(msg)).unwrap();
            mqtt_log::log_publish(all_cmd_topic, &payload);
            client.publish(all_cmd_topic, QoS::AtLeastOnce, false, payload).unwrap();
            format!("ジョブ{id}の中断を要求しました")
        }
        Some(job) => format!(
            "ジョブ{id}は現在{:?}状態のため中断できません（処理中(Dispatched)のジョブのみ中断できます）",
            job.status
        ),
        None => format!("ジョブ{id}は見つかりません"),
    }
}
