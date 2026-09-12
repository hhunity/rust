//! # 印刷ジョブキューを順番に処理するバックグラウンドワーカー
//!
//! [`crate::job_queue::JobQueue`]からPendingなジョブを1つずつ取り出し、`<topic>/NCMD/all`へ
//! 配信して、オンラインな全マイコンの完了報告が揃うまで待つ。同時に処理するジョブは常に
//! 最大1件（キューは直列専用で、プリンタごとの並列実行はしない設計）なので、
//! [`crate::controller::InFlightState`]も単一スロット（`Option`）のままで構わない。
//!
//! 以前は`/job`コマンドの入力スレッド（`stdin_commands.rs`）がこの配信・待ち受けを
//! その場で行っていたが、ここに切り出すことで「`/job`はキューに積むだけ」
//! 「積まれたキューを順番に捌くのはこのワーカー」と役割を分離した。

use std::collections::HashSet;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use rumqttc::{Client, QoS};

use crate::controller::{InFlightJob, InFlightState, Roster};
use crate::job_queue::JobQueue;
use crate::messages::{CmdMsg, JobMsg};
use crate::mqtt_log;
use crate::seq::{next_seq, ControllerSeqState};

/// ジョブを送ってから、完了報告が来ないマイコンを「失敗」と判断するまでの待ち時間。
const JOB_TIMEOUT: Duration = Duration::from_secs(10);

/// オンラインなマイコンが1台もいないとき、次に確認し直すまでの間隔。
const NO_TARGET_RETRY_INTERVAL: Duration = Duration::from_secs(1);

/// ワーカースレッドを立てる。この関数自体はスレッドを立てたらすぐ返る。
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn(
    client: Client,
    name: String,
    all_cmd_topic: String,
    roster: Roster,
    inflight: InFlightState,
    seq: ControllerSeqState,
    queue: JobQueue,
) {
    thread::spawn(move || loop {
        let job = queue.wait_for_next_pending();

        // 宛先にする「今オンラインの全マイコン」は、配信する瞬間に確定させる
        // （1台もいなければ、誰かがオンラインになるまでこのジョブの番のままキューをブロックする）。
        let targets = loop {
            let targets: HashSet<String> = roster
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, &online)| online)
                .map(|(who, _)| who.clone())
                .collect();
            if !targets.is_empty() {
                break targets;
            }
            thread::sleep(NO_TARGET_RETRY_INTERVAL);
        };

        queue.mark_dispatched(&job.id);

        let (tx, rx) = mpsc::channel::<String>();
        *inflight.lock().unwrap() = Some(InFlightJob { id: job.id.clone(), tx });

        let msg = JobMsg {
            id: job.id.clone(),
            from: name.clone(),
            content: job.content.clone(),
            seq: next_seq(&seq.job_counter),
        };
        let payload = serde_json::to_vec(&CmdMsg::Job(msg)).unwrap();
        mqtt_log::log_publish(&all_cmd_topic, &payload);
        client.publish(&all_cmd_topic, QoS::AtLeastOnce, false, payload).unwrap();
        println!(
            "[system] ジョブ{}を{}台のマイコン({targets:?})へ配信しました。完了を待っています…",
            job.id,
            targets.len()
        );

        let mut remaining = targets;
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
            println!("[system] ジョブ{}は全員完了しました", job.id);
            queue.mark_done(&job.id);
        } else {
            println!(
                "[system] エラー: ジョブ{}は次のマイコンから応答がありませんでした: {remaining:?}",
                job.id
            );
            queue.mark_failed(&job.id);
        }
    });
}
