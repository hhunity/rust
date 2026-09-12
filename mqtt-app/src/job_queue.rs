//! # 印刷ジョブの永続化キュー
//!
//! ジョブは常に「全マイコンへの一斉配信」で、同時に処理中のジョブは最大1件（直列実行）
//! という前提なので、キューの中身は「ジョブの並び＋各ジョブの状態」というシンプルな
//! リスト1本で表せる。SQLiteのような別プロセスのDB・スキーマ定義は不要で、
//! `serde_json`でリスト全体を1つのJSONファイルへ書き出すだけで永続化できる
//! （キューは小規模である前提で、この単純さを優先している）。
//!
//! 実際にキューを進める（MQTTで配信して完了を待つ）処理は[`crate::job_worker`]が担当し、
//! このモジュールは「今どんなジョブがあるか」を持つデータ構造と、その読み書きだけに専念する。

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// キューに入っているが、まだ配信していない。
    Pending,
    /// 配信済みで、オンラインな全マイコンの完了報告を待っている。
    Dispatched,
    /// 宛先にした全マイコンから完了報告が揃った。
    Done,
    /// タイムアウトするまでに完了報告が揃わなかった。
    Failed,
}

impl JobStatus {
    /// `/queue`の絞り込み・`/status`表示用に、コマンドライン文字列からの変換をここに集約する。
    /// 大文字小文字は問わない（`Pending`でも`pending`でもよい）。
    pub fn parse(s: &str) -> Option<JobStatus> {
        match s.to_ascii_lowercase().as_str() {
            "pending" => Some(JobStatus::Pending),
            "dispatched" => Some(JobStatus::Dispatched),
            "done" => Some(JobStatus::Done),
            "failed" => Some(JobStatus::Failed),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QueuedJob {
    pub id: String,
    pub content: String,
    pub status: JobStatus,
}

struct QueueState {
    jobs: VecDeque<QueuedJob>,
}

/// 永続化された印刷ジョブキュー。`Arc`で包まれているので`.clone()`しても中身は共有される
/// （複数スレッド・複数の呼び出し元から同じキューを指すハンドルとして渡し回せる）。
#[derive(Clone)]
pub struct JobQueue {
    state: Arc<Mutex<QueueState>>,
    /// `enqueue`されたら起こす合図。[`Self::wait_for_next_pending`]がここでブロックする。
    ready: Arc<Condvar>,
    path: Arc<PathBuf>,
}

impl JobQueue {
    /// `path`のファイルからキューを読み込む。ファイルが無ければ空のキューから始める。
    ///
    /// 前回のプロセスが「Dispatched（配信中）」のまま終了していたジョブはPendingへ戻す。
    /// 配信中に落ちた場合、実際に印刷まで終わっていたかはこちらからは分からないため、
    /// 安全側に倒して「配信からやり直す」扱いにしている（ジョブ内容は再送されても
    /// 問題ない前提＝べき等性が必要）。
    pub fn load_or_create(path: PathBuf) -> Self {
        let mut jobs = load_from_file(&path);
        let mut changed = false;
        for job in jobs.iter_mut() {
            if job.status == JobStatus::Dispatched {
                job.status = JobStatus::Pending;
                changed = true;
            }
        }
        let queue = JobQueue {
            state: Arc::new(Mutex::new(QueueState { jobs })),
            ready: Arc::new(Condvar::new()),
            path: Arc::new(path),
        };
        if changed {
            let guard = queue.state.lock().unwrap();
            queue.save_locked(&guard.jobs);
        }
        queue
    }

    /// ジョブをキューの末尾に追加し、発行したIDを返す。
    pub fn enqueue(&self, content: String) -> String {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let id = format!("job-{nanos}");
        let mut guard = self.state.lock().unwrap();
        guard.jobs.push_back(QueuedJob { id: id.clone(), content, status: JobStatus::Pending });
        self.save_locked(&guard.jobs);
        self.ready.notify_one();
        id
    }

    /// 今キューにある全ジョブのスナップショット（`/queue`表示用）。
    pub fn list(&self) -> Vec<QueuedJob> {
        self.state.lock().unwrap().jobs.iter().cloned().collect()
    }

    /// IDを指定して1件だけ取得する（`/status`用）。無ければ`None`。
    pub fn get(&self, id: &str) -> Option<QueuedJob> {
        self.state.lock().unwrap().jobs.iter().find(|j| j.id == id).cloned()
    }

    /// Pending状態のジョブだけをキューから取り消せる。既に配信中/完了/失敗のジョブは
    /// 今さら止める手段が無いので取り消せない（`false`を返す）。
    pub fn cancel(&self, id: &str) -> bool {
        let mut guard = self.state.lock().unwrap();
        let before = guard.jobs.len();
        guard.jobs.retain(|j| !(j.id == id && j.status == JobStatus::Pending));
        let removed = guard.jobs.len() != before;
        if removed {
            self.save_locked(&guard.jobs);
        }
        removed
    }

    /// 次に処理すべきPendingジョブが現れるまでブロックし、そのジョブを返す
    /// （キューからの取り出しはしない。呼び出し側は続けて[`Self::mark_dispatched`]を呼ぶこと）。
    ///
    /// ジョブは常に投入した順番どおりに処理される前提なので、「先頭から見て最初に
    /// 見つかったPending」を返せば十分（それより手前は必ずDone/Failed済みのはず）。
    pub fn wait_for_next_pending(&self) -> QueuedJob {
        let mut guard = self.state.lock().unwrap();
        loop {
            if let Some(job) = guard.jobs.iter().find(|j| j.status == JobStatus::Pending) {
                return job.clone();
            }
            guard = self.ready.wait(guard).unwrap();
        }
    }

    pub fn mark_dispatched(&self, id: &str) {
        self.update_status(id, JobStatus::Dispatched);
    }

    pub fn mark_done(&self, id: &str) {
        self.update_status(id, JobStatus::Done);
    }

    pub fn mark_failed(&self, id: &str) {
        self.update_status(id, JobStatus::Failed);
    }

    /// Failed状態のジョブをPendingへ戻し、キューの中の元の位置（＝投入した順番）から
    /// もう一度処理させる。Failed以外（Pending/Dispatched/Done）は対象外で`false`を返す
    /// （Pendingは既に順番待ち中、Dispatched/Doneを今さら差し戻す意味が無いため）。
    pub fn retry(&self, id: &str) -> bool {
        let mut guard = self.state.lock().unwrap();
        let Some(job) = guard.jobs.iter_mut().find(|j| j.id == id) else {
            return false;
        };
        if job.status != JobStatus::Failed {
            return false;
        }
        job.status = JobStatus::Pending;
        self.save_locked(&guard.jobs);
        self.ready.notify_one();
        true
    }

    /// Done/Failedになったジョブ（＝もう動かない履歴）をキューから取り除く。
    /// 削除した件数を返す。実行中(Pending/Dispatched)のジョブには触れない。
    pub fn clear_finished(&self) -> usize {
        let mut guard = self.state.lock().unwrap();
        let before = guard.jobs.len();
        guard.jobs.retain(|j| matches!(j.status, JobStatus::Pending | JobStatus::Dispatched));
        let removed = before - guard.jobs.len();
        if removed > 0 {
            self.save_locked(&guard.jobs);
        }
        removed
    }

    fn update_status(&self, id: &str, status: JobStatus) {
        let mut guard = self.state.lock().unwrap();
        if let Some(job) = guard.jobs.iter_mut().find(|j| j.id == id) {
            job.status = status;
        }
        self.save_locked(&guard.jobs);
    }

    /// キュー全体を、一時ファイルへ書いてからリネームする形でファイルへ書き出す
    /// （書き込み途中でプロセスが落ちても、リネームは1操作なので既存ファイルが
    /// 中途半端な内容で壊れることはない）。
    fn save_locked(&self, jobs: &VecDeque<QueuedJob>) {
        let json = serde_json::to_vec_pretty(jobs).unwrap();
        let tmp_path = self.path.with_extension("json.tmp");
        fs::write(&tmp_path, json)
            .unwrap_or_else(|e| panic!("ジョブキューの書き込みに失敗しました: {e}"));
        fs::rename(&tmp_path, &*self.path)
            .unwrap_or_else(|e| panic!("ジョブキューファイルの置き換えに失敗しました: {e}"));
    }
}

/// ファイルが無い・読めない場合は「まだ何も無い」として空のキューを返す
/// （初回起動時にファイルが存在しないのは正常なケースなので、エラーにはしない）。
fn load_from_file(path: &Path) -> VecDeque<QueuedJob> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .unwrap_or_else(|e| panic!("ジョブキューファイル{}の読み込みに失敗しました: {e}", path.display())),
        Err(_) => VecDeque::new(),
    }
}
