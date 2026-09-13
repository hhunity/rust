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
//! - `job`はキューに積んだうえで、その場で配信を試みる。ただし起動時にファイルから
//!   引き継いだ未処理ジョブは、自動では配信されない（勝手に印刷が始まると困るため）。
//!   宛先が誰もオンラインでない等の理由で配信できなかったジョブはPendingのまま
//!   「止まった」状態になり、`run`コマンドで再開できる。`run`はIDを指定すればその
//!   ジョブだけを、省略すればPendingジョブを先頭から順に進められるだけ全部処理する
//!   （詳しくは[`crate::job_dispatch`]参照）。
//! - `job`・`run`の実際の配信・完了待ちは、コールバック自身の中ではなく**別スレッド**で
//!   行っている。理由は、`abort`（処理中のジョブを中断するコマンド）を使えるようにする
//!   ため。`reedline-repl-rs`は1行読んでコールバックを実行し終わるまで次の行を読まない
//!   （＝コールバックが実行中の間、入力自体を受け付けない）ので、`job`/`run`が完了まで
//!   その場でブロックし続けると、印字中に`abort`を打つこと自体ができなくなってしまう。
//!   そのため`cmd_job`/`cmd_run`は「配信を始めた」ことだけをその場で返し、実際の待ち受けは
//!   [`std::thread::spawn`]した別スレッドに任せて、結果が出たら[`PRINTER`]経由で後から表示する。

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use reedline_repl_rs::clap::{Arg, ArgMatches, Command};
use reedline_repl_rs::reedline::ExternalPrinter;
use reedline_repl_rs::{Repl, Result as ReplResult};
use rumqttc::{Client, QoS};

use crate::controller::{DeviceCapabilities, DeviceStatuses, InFlightState, PendingOffers, Roster};
use crate::job_dispatch;
use crate::job_queue::{JobQueue, JobStatus};
use crate::messages::{CmdMsg, OfferMsg};
use crate::mqtt_log;
use crate::seq::{next_seq, ControllerSeqState};

/// `job`・`run`が結果を待つ間に別スレッドへ逃がすため、そのスレッドから状況を表示する
/// のに使う`ExternalPrinter`。`Repl`本体を経由しないと取得できない値なので（`spawn`の
/// 中で構築される）、いったんここに入れておいて`cmd_job`等から参照する。
static PRINTER: OnceLock<ExternalPrinter<String>> = OnceLock::new();

/// コールバック（関数ポインタ）に閉じ込められない、コマンド間で共有する状態一式。
struct Context {
    client: Client,
    name: String,
    topic: String,
    pending_offers: PendingOffers,
    roster: Roster,
    device_statuses: DeviceStatuses,
    device_capabilities: DeviceCapabilities,
    inflight: InFlightState,
    seq: ControllerSeqState,
    queue: JobQueue,
}

/// `content`引数（複数トークンに分かれうる）を、スペース区切りの1本の文字列へ戻す。
fn joined_arg(args: &ArgMatches, name: &str) -> String {
    args.get_many::<String>(name)
        .unwrap()
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `job`・`run`本体の処理を、呼び出し元をブロックしない別スレッドで実行する。
/// スレッドの中身（`work`）が返したメッセージは、完了時に[`PRINTER`]経由で表示される。
fn spawn_dispatch(work: impl FnOnce() -> String + Send + 'static) {
    thread::spawn(move || {
        let result = work();
        if let Some(printer) = PRINTER.get() {
            let _ = printer.print(result);
        }
    });
}

/// キューに積んだうえで、その場で配信を試みる（実際の配信・完了待ちは別スレッドで行う。
/// 詳しくはこのファイル冒頭のコメント参照）。宛先が誰もオンラインでない等の理由で
/// 配信できなければ、ジョブはPendingのまま「止まった」状態になる（それを後から
/// 再開させるのが[`cmd_run`]の役目）。
fn cmd_job(args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let id = ctx.queue.enqueue(joined_arg(&args, "content"));
    let client = ctx.client.clone();
    let name = ctx.name.clone();
    let all_cmd_topic = format!("{}/NCMD/all", ctx.topic);
    let roster = ctx.roster.clone();
    let device_statuses = ctx.device_statuses.clone();
    let inflight = ctx.inflight.clone();
    let seq = ctx.seq.clone();
    let queue = ctx.queue.clone();
    spawn_dispatch(move || {
        // ここは名指し(Some(&id))ではなくNone(先頭のPendingを処理)のまま。キューは厳密に
        // 投入順で処理する設計なので、自分より前に止まっているジョブがあればそちらが
        // 優先されるべきで、今追加した自分を横入りさせるべきではないため。
        job_dispatch::run_one(
            &client,
            &name,
            &all_cmd_topic,
            &roster,
            &device_statuses,
            &inflight,
            &seq,
            &queue,
            None,
        )
    });
    Ok(Some(format!(
        "ジョブ{id}をキューに追加し、配信を開始しました（結果は追って表示されます。queueでも確認できます）"
    )))
}

/// 止まっている（配信できずPendingのままの）ジョブを再開する。IDを指定すればそのジョブ
/// だけを名指しで再開し、省略すればPendingジョブを先頭から順に進められるだけ全部処理する。
/// `job`同様、実際の処理は別スレッドで行い、その場ではすぐ返る。
fn cmd_run(args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let client = ctx.client.clone();
    let name = ctx.name.clone();
    let all_cmd_topic = format!("{}/NCMD/all", ctx.topic);
    let roster = ctx.roster.clone();
    let device_statuses = ctx.device_statuses.clone();
    let inflight = ctx.inflight.clone();
    let seq = ctx.seq.clone();
    let queue = ctx.queue.clone();
    match args.get_one::<String>("id").cloned() {
        Some(id) => {
            spawn_dispatch(move || {
                job_dispatch::run_one(
                    &client,
                    &name,
                    &all_cmd_topic,
                    &roster,
                    &device_statuses,
                    &inflight,
                    &seq,
                    &queue,
                    Some(&id),
                )
            });
            Ok(Some("再開を開始しました（結果は追って表示されます）".to_string()))
        }
        None => {
            spawn_dispatch(move || {
                job_dispatch::run_all(
                    &client,
                    &name,
                    &all_cmd_topic,
                    &roster,
                    &device_statuses,
                    &inflight,
                    &seq,
                    &queue,
                )
                .join("\n")
            });
            Ok(Some(
                "止まっているジョブの再開を開始しました（結果は追って表示されます）".to_string(),
            ))
        }
    }
}

/// 処理中(Dispatched)のジョブを中断させる。指示を送るだけなのでその場で結果が分かり、
/// 別スレッドに逃がす必要はない（実際に止まったかどうかは、待っている`job`/`run`側の
/// スレッドが後から表示する）。
fn cmd_abort(args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let id = args.get_one::<String>("id").unwrap();
    let all_cmd_topic = format!("{}/NCMD/all", ctx.topic);
    Ok(Some(job_dispatch::abort(
        &ctx.client,
        &ctx.name,
        &all_cmd_topic,
        &ctx.seq,
        &ctx.queue,
        id,
    )))
}

fn cmd_queue(args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let status_filter = match args.get_one::<String>("status") {
        None => None,
        Some(s) => match JobStatus::parse(s) {
            Some(status) => Some(status),
            None => {
                return Ok(Some(
                    "使い方: queue [pending|dispatched|done|failed|aborted]".to_string(),
                ))
            }
        },
    };
    let jobs: Vec<_> = ctx
        .queue
        .list()
        .into_iter()
        .filter(|j| status_filter.is_none_or(|s| j.status == s))
        .collect();
    if jobs.is_empty() {
        return Ok(Some("該当するジョブはありません".to_string()));
    }
    let lines: Vec<String> = jobs
        .iter()
        .map(|j| format!("{} [{:?}] {}", j.id, j.status, j.content))
        .collect();
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
    Ok(Some(format!(
        "完了/失敗済みのジョブを{removed}件削除しました"
    )))
}

/// オンラインなマイコンの一覧を、状態(idle/printing/error)と印刷能力つきで表示する。
fn cmd_devices(_args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let roster = ctx.roster.lock().unwrap();
    if roster.is_empty() {
        return Ok(Some("オンラインのマイコンはいません".to_string()));
    }
    let statuses = ctx.device_statuses.lock().unwrap();
    let capabilities = ctx.device_capabilities.lock().unwrap();
    let mut names: Vec<&String> = roster.keys().collect();
    names.sort();
    let lines: Vec<String> = names
        .into_iter()
        .map(|name| {
            let status = statuses.get(name).map(|s| format!("{s:?}")).unwrap_or_else(|| "不明".to_string());
            match capabilities.get(name) {
                Some(caps) => format!(
                    "{name} [{status}] 機種: {} / 対応用紙: {}",
                    caps.model,
                    caps.paper_sizes.join(",")
                ),
                None => format!("{name} [{status}]"),
            }
        })
        .collect();
    Ok(Some(lines.join("\n")))
}

fn cmd_send(args: ArgMatches, ctx: &mut Context) -> ReplResult<Option<String>> {
    let to = args.get_one::<String>("to").unwrap();
    let path_str = args.get_one::<String>("path").unwrap();
    let path = PathBuf::from(path_str);
    let metadata = match fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => return Ok(Some(format!("ファイルが読めません: {path_str} ({e})"))),
    };
    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path_str.clone());

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
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
    ctx.client
        .publish(&offer_topic, QoS::AtLeastOnce, false, payload)
        .unwrap();
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
    ctx.client
        .publish(&ctx.topic, qos, false, message.as_bytes())
        .unwrap();
    Ok(None)
}

/// 標準入力を`reedline-repl-rs`のREPLとして受け付ける専用スレッドを立てる。
/// この関数自体はスレッドを立てたらすぐ返るが、`Repl`（と、その内部の
/// `ExternalPrinter`）はこの関数の中で先に組み立てる。得られた`ExternalPrinter`は
/// [`PRINTER`]に保存して`cmd_job`等の別スレッドからも使えるようにしつつ、戻り値としても
/// 返して`controller`のメインループなど他のスレッドにも渡せるようにしている（詳しくは
/// `controller.rs`の`say`ヘルパーのコメントも参照）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn(
    client: Client,
    name: String,
    topic: String,
    pending_offers: PendingOffers,
    roster: Roster,
    device_statuses: DeviceStatuses,
    device_capabilities: DeviceCapabilities,
    inflight: InFlightState,
    seq: ControllerSeqState,
    queue: JobQueue,
) -> ExternalPrinter<String> {
    let context = Context {
        client,
        name,
        topic,
        pending_offers,
        roster,
        device_statuses,
        device_capabilities,
        inflight,
        seq,
        queue,
    };
    let mut repl = Repl::new(context)
        .with_name("mqtt-server")
        .with_description("チャット・ファイル送信・印刷ジョブキューの操作")
        .with_command(
            Command::new("job")
                .about("印刷ジョブをキューに追加し、その場で配信を試みる（内容はスペースを含んでよい）")
                .arg(Arg::new("content").required(true).num_args(1..)),
            cmd_job,
        )
        .with_command(
            Command::new("run")
                .about("止まっている(Pendingの)ジョブを再開する（IDを省略すると先頭から全件）")
                .arg(Arg::new("id").required(false)),
            cmd_run,
        )
        .with_command(
            Command::new("abort")
                .about("処理中(Dispatched)のジョブを中断する")
                .arg(Arg::new("id").required(true)),
            cmd_abort,
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
            Command::new("devices").about("オンラインなマイコンの状態・印刷能力を一覧表示する"),
            cmd_devices,
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

    let printer = repl.external_printer();
    let _ = PRINTER.set(printer.clone());
    thread::spawn(move || {
        if let Err(e) = repl.run() {
            eprintln!("[system] コマンド入力ループが終了しました: {e}");
        }
    });
    printer
}
