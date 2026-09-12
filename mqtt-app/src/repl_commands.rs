//! # `reedline-repl-rs`を使った標準入力コマンド受け付け（試験導入版）
//!
//! [`crate::stdin_commands`]（自作の`if line.starts_with(...)`チェーン）を、
//! `reedline-repl-rs`クレートに置き換えたもの。元の実装はそのまま残してあるので、
//! 気に入らなければ[`crate::controller::run`]内の呼び出しを`stdin_commands::spawn`へ
//! 戻すだけで元通りになる（このファイルごと削除しても他に影響しない）。
//!
//! ## 元の実装との違い
//!
//! - コマンド解析・`--help`・矢印キーでの履歴/補完は`reedline-repl-rs`（内部で`clap`と
//!   `reedline`を使用）に任せている。
//! - `reedline-repl-rs`のコールバックは**関数ポインタ**（`fn(...)`）であり、クロージャの
//!   ように外側の変数をキャプチャできない。そのため、必要な状態（MQTTクライアント・
//!   キューなど）は全部[`Context`]構造体にまとめ、`&mut Context`として毎回受け取る形にした。
//! - `reedline-repl-rs`は「1行＝1コマンド呼び出し」という設計で、元の実装にあった
//!   「プレフィックス無しの行はそのままチャットとして送る」という挙動は無い。そのため
//!   チャットも`chat <本文>`という明示コマンドに変え、`/qos0`〜`/qos2`プレフィックスは
//!   `--qos <0|1|2>`オプションに置き換えている。

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use reedline_repl_rs::clap::{Arg, ArgMatches, Command};
use reedline_repl_rs::{Repl, Result as ReplResult};
use rumqttc::{Client, QoS};

use crate::controller::PendingOffers;
use crate::job_queue::{JobQueue, JobStatus};
use crate::messages::{CmdMsg, OfferMsg};
use crate::mqtt_log;
use crate::seq::{next_seq, ControllerSeqState};

/// コールバック（関数ポインタ）に閉じ込められない、コマンド間で共有する状態一式。
struct Context {
    client: Client,
    name: String,
    topic: String,
    pending_offers: PendingOffers,
    seq: ControllerSeqState,
    queue: JobQueue,
}

/// `content`引数（複数トークンに分かれうる）を、スペース区切りの1本の文字列へ戻す。
fn joined_arg(args: &ArgMatches, name: &str) -> String {
    args.get_many::<String>(name).unwrap().cloned().collect::<Vec<_>>().join(" ")
}

fn cmd_job(args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let id = ctx.queue.enqueue(joined_arg(&args, "content"));
    Ok(Some(format!("ジョブ{id}をキューに追加しました（バックグラウンドで順番に配信されます。queueで確認できます）")))
}

fn cmd_queue(args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let status_filter = match args.get_one::<String>("status") {
        None => None,
        Some(s) => match JobStatus::parse(s) {
            Some(status) => Some(status),
            None => return Ok(Some("使い方: queue [pending|dispatched|done|failed]".to_string())),
        },
    };
    let jobs: Vec<_> =
        ctx.queue.list().into_iter().filter(|j| status_filter.is_none_or(|s| j.status == s)).collect();
    if jobs.is_empty() {
        return Ok(Some("該当するジョブはありません".to_string()));
    }
    let lines: Vec<String> =
        jobs.iter().map(|j| format!("{} [{:?}] {}", j.id, j.status, j.content)).collect();
    Ok(Some(lines.join("\n")))
}

fn cmd_status(args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let id = args.get_one::<String>("id").unwrap();
    Ok(Some(match ctx.queue.get(id) {
        Some(job) => format!("{} [{:?}] {}", job.id, job.status, job.content),
        None => format!("ジョブ{id}は見つかりません"),
    }))
}

fn cmd_cancel(args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let id = args.get_one::<String>("id").unwrap();
    Ok(Some(if ctx.queue.cancel(id) {
        format!("ジョブ{id}を取り消しました")
    } else {
        format!("ジョブ{id}は取り消せません（存在しないか、既に配信中/完了済みです）")
    }))
}

fn cmd_retry(args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let id = args.get_one::<String>("id").unwrap();
    Ok(Some(if ctx.queue.retry(id) {
        format!("ジョブ{id}をPendingへ戻しました。再度配信されます")
    } else {
        format!("ジョブ{id}は再実行できません（存在しないか、Failed状態ではありません）")
    }))
}

fn cmd_clear(_args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let removed = ctx.queue.clear_finished();
    Ok(Some(format!("完了/失敗済みのジョブを{removed}件削除しました")))
}

fn cmd_send(args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let to = args.get_one::<String>("to").unwrap();
    let path_str = args.get_one::<String>("path").unwrap();
    let path = PathBuf::from(path_str);
    let metadata = match fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => return Ok(Some(format!("ファイルが読めません: {path_str} ({e})"))),
    };
    let filename =
        path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| path_str.clone());

    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let id = format!("{}-{nanos}", ctx.name);
    ctx.pending_offers.lock().unwrap().insert(id.clone(), path);

    let offer = OfferMsg {
        id,
        from: ctx.name.clone(),
        filename: filename.clone(),
        size: metadata.len(),
        seq: next_seq(&ctx.seq.offer_counter),
    };
    let offer_topic = format!("{}/NCMD/{to}", ctx.topic);
    let payload = serde_json::to_vec(&CmdMsg::FileOffer(offer)).unwrap();
    mqtt_log::log_publish(&offer_topic, &payload);
    ctx.client.publish(&offer_topic, QoS::AtLeastOnce, false, payload).unwrap();
    Ok(Some(format!(
        "{to}へ {filename} ({} bytes) の送信を申し出ました。相手の応答を待っています…",
        metadata.len()
    )))
}

fn cmd_chat(args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let qos = match args.get_one::<String>("qos").map(String::as_str) {
        Some("0") => QoS::AtMostOnce,
        Some("2") => QoS::ExactlyOnce,
        _ => QoS::AtLeastOnce,
    };
    let message = format!("{}: {}", ctx.name, joined_arg(&args, "text"));
    mqtt_log::log_publish(&ctx.topic, message.as_bytes());
    ctx.client.publish(&ctx.topic, qos, false, message.as_bytes()).unwrap();
    Ok(None)
}

/// 標準入力を`reedline-repl-rs`のREPLとして受け付ける専用スレッドを立てる。
/// この関数自体はスレッドを立てたらすぐ返る。
pub(crate) fn spawn(
    client: Client,
    name: String,
    topic: String,
    pending_offers: PendingOffers,
    seq: ControllerSeqState,
    queue: JobQueue,
) {
    thread::spawn(move || {
        let context = Context { client, name, topic, pending_offers, seq, queue };
        let mut repl = Repl::new(context)
            .with_name("mqtt-server")
            .with_description("チャット・ファイル送信・印刷ジョブキューの操作")
            .with_command(
                Command::new("job")
                    .about("印刷ジョブをキューに追加する（内容はスペースを含んでよい）")
                    .arg(Arg::new("content").required(true).num_args(1..)),
                cmd_job,
            )
            .with_command(
                Command::new("queue")
                    .about("キューの一覧を表示する（状態を付けると絞り込める）")
                    .arg(Arg::new("status").required(false)),
                cmd_queue,
            )
            .with_command(
                Command::new("status")
                    .about("指定した1件のジョブの状態を表示する")
                    .arg(Arg::new("id").required(true)),
                cmd_status,
            )
            .with_command(
                Command::new("cancel")
                    .about("まだ配信していないジョブをキューから取り消す")
                    .arg(Arg::new("id").required(true)),
                cmd_cancel,
            )
            .with_command(
                Command::new("retry")
                    .about("失敗したジョブをもう一度Pendingへ戻す")
                    .arg(Arg::new("id").required(true)),
                cmd_retry,
            )
            .with_command(
                Command::new("clear").about("完了/失敗済みのジョブをキューから削除する"),
                cmd_clear,
            )
            .with_command(
                Command::new("send")
                    .about("宛先のマイコンへファイル送信を申し出る")
                    .arg(Arg::new("to").required(true))
                    .arg(Arg::new("path").required(true)),
                cmd_send,
            )
            .with_command(
                Command::new("chat")
                    .about("チャットメッセージを送る")
                    .arg(Arg::new("qos").long("qos").help("0/1/2（省略時は1）"))
                    .arg(Arg::new("text").required(true).num_args(1..)),
                cmd_chat,
            );

        if let Err(e) = repl.run() {
            eprintln!("[system] コマンド入力ループが終了しました: {e}");
        }
    });
}
