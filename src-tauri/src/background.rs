use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::keep_awake::RunningWork;
use crate::tools::{hide_console_window, kill_process_tree, kill_process_tree_now, Utf8Chunker};

const MAX_KEPT_OUTPUT_BYTES: usize = 200_000;
const OUTPUT_SHOWN_TO_MODEL_CHARS: usize = 4_000;
const START_WAIT: Duration = Duration::from_secs(5);
const START_CHECK_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummary {
    pub id: String,
    pub chat_id: String,
    pub name: String,
    pub command: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub was_stopped: bool,
}

impl TaskSummary {
    pub fn is_running(&self) -> bool {
        self.finished_at.is_none()
    }
}

pub enum BackgroundEvent {
    Changed { chat_id: String },
    Output { task_id: String, text: String },
}

pub type BackgroundNotifier = Arc<dyn Fn(BackgroundEvent) + Send + Sync>;

struct Task {
    summary: TaskSummary,
    output: String,
    process_id: Option<u32>,
    keeps_computer_awake: Option<RunningWork>,
}

#[derive(Default, Clone)]
pub struct BackgroundTasks {
    tasks: Arc<Mutex<HashMap<String, Task>>>,
}

pub struct NewTask {
    pub chat_id: String,
    pub name: String,
    pub command: String,
    pub folder: Option<PathBuf>,
    pub keeps_computer_awake: Option<RunningWork>,
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn keep_last_bytes(text: &mut String, limit: usize) {
    if text.len() <= limit {
        return;
    }
    let mut cut = text.len() - limit;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    text.drain(..cut);
}

fn last_chars(text: &str, limit: usize) -> &str {
    let total = text.chars().count();
    if total <= limit {
        return text;
    }
    let start = text.char_indices().nth(total - limit).map_or(0, |(index, _)| index);
    &text[start..]
}

impl BackgroundTasks {
    fn with_tasks<T>(&self, action: impl FnOnce(&mut HashMap<String, Task>) -> T) -> T {
        let mut tasks = self.tasks.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        action(&mut tasks)
    }

    pub fn list(&self, chat_id: &str) -> Vec<TaskSummary> {
        let mut found: Vec<TaskSummary> = self.with_tasks(|tasks| tasks.values().filter(|task| task.summary.chat_id == chat_id).map(|task| task.summary.clone()).collect());
        found.sort_by_key(|summary| std::cmp::Reverse(summary.started_at));
        found
    }

    pub fn output(&self, task_id: &str) -> Option<String> {
        self.with_tasks(|tasks| tasks.get(task_id).map(|task| task.output.clone()))
    }

    pub fn summary(&self, task_id: &str) -> Option<TaskSummary> {
        self.with_tasks(|tasks| tasks.get(task_id).map(|task| task.summary.clone()))
    }

    fn append_output(&self, task_id: &str, text: &str) {
        self.with_tasks(|tasks| {
            if let Some(task) = tasks.get_mut(task_id) {
                task.output.push_str(text);
                keep_last_bytes(&mut task.output, MAX_KEPT_OUTPUT_BYTES);
            }
        });
    }

    fn mark_finished(&self, task_id: &str, exit_code: Option<i32>) {
        self.with_tasks(|tasks| {
            if let Some(task) = tasks.get_mut(task_id) {
                task.summary.finished_at = Some(now_millis());
                task.summary.exit_code = exit_code;
                task.process_id = None;
                task.keeps_computer_awake = None;
            }
        });
    }

    fn read_into_task(&self, task_id: String, pipe: Option<impl AsyncRead + Unpin + Send + 'static>, notify: BackgroundNotifier) -> tauri::async_runtime::JoinHandle<()> {
        let tasks = self.clone();
        tauri::async_runtime::spawn(async move {
            let Some(mut pipe) = pipe else {
                return;
            };
            let mut chunker = Utf8Chunker::default();
            let mut chunk = [0u8; 8192];
            while let Ok(count) = pipe.read(&mut chunk).await {
                if count == 0 {
                    break;
                }
                let text = chunker.push(&chunk[..count]);
                tasks.append_output(&task_id, &text);
                notify(BackgroundEvent::Output { task_id: task_id.clone(), text });
            }
        })
    }

    pub fn start(&self, task: NewTask, notify: BackgroundNotifier) -> Result<String, String> {
        let script = format!("[Console]::OutputEncoding = [Text.Encoding]::UTF8; {}", task.command);
        let mut command = Command::new("powershell");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide_console_window(&mut command);
        if let Some(folder) = &task.folder {
            command.current_dir(folder);
        }
        let mut child = command.spawn().map_err(|error| format!("Не удалось запустить команду: {error}"))?;
        let task_id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        let summary = TaskSummary {
            id: task_id.clone(),
            chat_id: task.chat_id.clone(),
            name: task.name,
            command: task.command,
            started_at: now_millis(),
            finished_at: None,
            exit_code: None,
            was_stopped: false,
        };
        let stored = Task { summary, output: String::new(), process_id: child.id(), keeps_computer_awake: task.keeps_computer_awake };
        self.with_tasks(|tasks| tasks.insert(task_id.clone(), stored));
        let standard = self.read_into_task(task_id.clone(), child.stdout.take(), notify.clone());
        let errors = self.read_into_task(task_id.clone(), child.stderr.take(), notify.clone());
        notify(BackgroundEvent::Changed { chat_id: task.chat_id.clone() });
        let tasks = self.clone();
        let finished_id = task_id.clone();
        let chat_id = task.chat_id;
        tauri::async_runtime::spawn(async move {
            let status = child.wait().await;
            let _ = standard.await;
            let _ = errors.await;
            tasks.mark_finished(&finished_id, status.ok().and_then(|status| status.code()));
            notify(BackgroundEvent::Changed { chat_id });
        });
        Ok(task_id)
    }

    pub async fn give_time_to_start(&self, task_id: &str) {
        let mut waited = Duration::ZERO;
        while waited < START_WAIT {
            let still_running = self.summary(task_id).is_some_and(|summary| summary.is_running());
            if !still_running {
                return;
            }
            tokio::time::sleep(START_CHECK_INTERVAL).await;
            waited += START_CHECK_INTERVAL;
        }
    }

    pub async fn stop(&self, task_id: &str) -> Result<(), String> {
        let process_id = self.with_tasks(|tasks| {
            let task = tasks.get_mut(task_id)?;
            task.summary.was_stopped = task.summary.is_running();
            task.process_id
        });
        if let Some(process_id) = process_id {
            kill_process_tree(process_id).await;
        }
        Ok(())
    }

    pub fn stop_all_now(&self) {
        let process_ids: Vec<u32> = self.with_tasks(|tasks| tasks.values().filter_map(|task| task.process_id).collect());
        for process_id in process_ids {
            kill_process_tree_now(process_id);
        }
    }

    pub fn report_for_model(&self, task_id: &str) -> Option<String> {
        let (summary, output) = self.with_tasks(|tasks| tasks.get(task_id).map(|task| (task.summary.clone(), task.output.clone())))?;
        let state = match (summary.finished_at, summary.was_stopped, summary.exit_code) {
            (None, _, _) => "работает".to_string(),
            (Some(_), true, _) => "остановлена".to_string(),
            (Some(_), false, Some(code)) => format!("завершилась с кодом {code}"),
            (Some(_), false, None) => "завершилась".to_string(),
        };
        let shown = last_chars(output.trim_end(), OUTPUT_SHOWN_TO_MODEL_CHARS);
        let shown = if shown.is_empty() { "вывода пока нет" } else { shown };
        Some(format!("Задача {} «{}»: {state}.\n{shown}", summary.id, summary.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn silent() -> BackgroundNotifier {
        Arc::new(|_| {})
    }

    #[test]
    fn keeps_only_the_newest_output() {
        let mut text = "старое новое".to_string();
        keep_last_bytes(&mut text, 10);
        assert_eq!(text, "новое");
        keep_last_bytes(&mut text, 9);
        assert_eq!(text, "овое");
        assert_eq!(last_chars("абвгд", 2), "гд");
    }

    #[test]
    fn a_task_runs_in_the_background_and_reports_its_end() {
        let tasks = BackgroundTasks::default();
        let task = NewTask { chat_id: "c".into(), name: "проверка".into(), command: "Write-Output 'готов'; exit 2".into(), folder: None, keeps_computer_awake: None };
        tauri::async_runtime::block_on(async {
            let id = tasks.start(task, silent()).unwrap();
            assert!(tasks.summary(&id).is_some());
            for _ in 0..80 {
                if !tasks.summary(&id).unwrap().is_running() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let report = tasks.report_for_model(&id).unwrap();
            assert!(report.contains("завершилась с кодом 2"), "{report}");
            assert!(report.contains("готов"), "{report}");
            assert_eq!(tasks.list("c").len(), 1);
            assert!(tasks.list("другой").is_empty());
        });
    }

    #[test]
    fn a_stopped_task_says_so() {
        let tasks = BackgroundTasks::default();
        let task = NewTask { chat_id: "c".into(), name: "долгая".into(), command: "Start-Sleep 60".into(), folder: None, keeps_computer_awake: None };
        tauri::async_runtime::block_on(async {
            let id = tasks.start(task, silent()).unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
            tasks.stop(&id).await.unwrap();
            for _ in 0..80 {
                if !tasks.summary(&id).unwrap().is_running() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            assert!(tasks.report_for_model(&id).unwrap().contains("остановлена"));
        });
    }
}
