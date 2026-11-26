use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use base64::Engine;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::background::keep_last_bytes;
use crate::tools::{hide_console_window, kill_process_tree, kill_process_tree_now, Utf8Chunker};

const FOLDER_MARKER: &str = "<<TARN_CWD>>";
const MAX_KEPT_HISTORY_BYTES: usize = 100_000;
const PREPARE_SHELL: &str = "[Console]::OutputEncoding = [Text.Encoding]::UTF8; $OutputEncoding = [Text.Encoding]::UTF8; $ProgressPreference = 'SilentlyContinue'";

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TerminalView {
    pub text: String,
    pub folder: String,
    pub is_running: bool,
}

pub enum TerminalEvent {
    Output { chat_id: String, text: String },
    Folder { chat_id: String, folder: String },
}

pub type TerminalNotifier = Arc<dyn Fn(TerminalEvent) + Send + Sync>;

struct Shell {
    input: mpsc::UnboundedSender<String>,
    process_id: Option<u32>,
}

#[derive(Default)]
struct Terminal {
    history: String,
    folder: String,
    shell: Option<Shell>,
}

#[derive(Default, Clone)]
pub struct UserTerminals {
    terminals: Arc<Mutex<HashMap<String, Terminal>>>,
}

pub fn command_line_for_shell(typed: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(typed.as_bytes());
    format!("try {{ Invoke-Expression ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'))) }} catch {{ Write-Error $_ }}")
}

fn folder_report_line() -> String {
    format!("Write-Output ('{FOLDER_MARKER}' + (Get-Location).Path)")
}

#[derive(Default)]
pub struct MarkerReader {
    unfinished_line: String,
}

pub struct ReadPiece {
    pub shown: String,
    pub folder: Option<String>,
}

impl MarkerReader {
    pub fn push(&mut self, text: &str) -> ReadPiece {
        let mut pending = std::mem::take(&mut self.unfinished_line);
        pending.push_str(text);
        let mut piece = ReadPiece { shown: String::new(), folder: None };
        let mut rest = pending.as_str();
        while let Some(line_end) = rest.find('\n') {
            let line = &rest[..=line_end];
            match line.trim_end().strip_prefix(FOLDER_MARKER) {
                Some(folder) => piece.folder = Some(folder.to_string()),
                None => piece.shown.push_str(line),
            }
            rest = &rest[line_end + 1..];
        }
        let could_become_marker = FOLDER_MARKER.starts_with(rest) || rest.starts_with(FOLDER_MARKER);
        if !rest.is_empty() && could_become_marker {
            self.unfinished_line = rest.to_string();
        } else {
            piece.shown.push_str(rest);
        }
        piece
    }
}

impl UserTerminals {
    fn with_terminals<T>(&self, action: impl FnOnce(&mut HashMap<String, Terminal>) -> T) -> T {
        let mut terminals = self.terminals.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        action(&mut terminals)
    }

    pub fn view(&self, chat_id: &str) -> TerminalView {
        self.with_terminals(|terminals| {
            terminals.get(chat_id).map_or_else(TerminalView::default, |terminal| TerminalView {
                text: terminal.history.clone(),
                folder: terminal.folder.clone(),
                is_running: terminal.shell.is_some(),
            })
        })
    }

    fn remember_output(&self, chat_id: &str, text: &str) {
        self.with_terminals(|terminals| {
            let terminal = terminals.entry(chat_id.to_string()).or_default();
            terminal.history.push_str(text);
            keep_last_bytes(&mut terminal.history, MAX_KEPT_HISTORY_BYTES);
        });
    }

    fn remember_folder(&self, chat_id: &str, folder: &str) {
        self.with_terminals(|terminals| terminals.entry(chat_id.to_string()).or_default().folder = folder.to_string());
    }

    fn read_into_history(&self, chat_id: String, pipe: Option<impl AsyncRead + Unpin + Send + 'static>, notify: TerminalNotifier) -> tauri::async_runtime::JoinHandle<()> {
        let terminals = self.clone();
        tauri::async_runtime::spawn(async move {
            let Some(mut pipe) = pipe else {
                return;
            };
            let mut chunker = Utf8Chunker::default();
            let mut markers = MarkerReader::default();
            let mut chunk = [0u8; 8192];
            while let Ok(count) = pipe.read(&mut chunk).await {
                if count == 0 {
                    break;
                }
                let piece = markers.push(&chunker.push(&chunk[..count]));
                if let Some(folder) = piece.folder {
                    terminals.remember_folder(&chat_id, &folder);
                    notify(TerminalEvent::Folder { chat_id: chat_id.clone(), folder });
                }
                if !piece.shown.is_empty() {
                    terminals.remember_output(&chat_id, &piece.shown);
                    notify(TerminalEvent::Output { chat_id: chat_id.clone(), text: piece.shown });
                }
            }
        })
    }

    fn start_shell(&self, chat_id: &str, folder: Option<PathBuf>, notify: TerminalNotifier) -> Result<mpsc::UnboundedSender<String>, String> {
        let mut command = Command::new("powershell");
        command
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide_console_window(&mut command);
        let folder = folder.filter(|path| path.is_dir()).or_else(dirs::home_dir);
        if let Some(folder) = &folder {
            command.current_dir(folder);
        }
        let mut child = command.spawn().map_err(|error| format!("Не удалось запустить PowerShell: {error}"))?;
        let mut stdin = child.stdin.take().ok_or("PowerShell не принял ввод")?;
        let (sender, mut receiver) = mpsc::unbounded_channel::<String>();
        tauri::async_runtime::spawn(async move {
            while let Some(line) = receiver.recv().await {
                let written = stdin.write_all(format!("{line}\n").as_bytes()).await;
                if written.is_err() || stdin.flush().await.is_err() {
                    break;
                }
            }
        });
        let standard = self.read_into_history(chat_id.to_string(), child.stdout.take(), notify.clone());
        let errors = self.read_into_history(chat_id.to_string(), child.stderr.take(), notify.clone());
        let process_id = child.id();
        self.with_terminals(|terminals| {
            let terminal = terminals.entry(chat_id.to_string()).or_default();
            terminal.shell = Some(Shell { input: sender.clone(), process_id });
            if let Some(folder) = &folder {
                terminal.folder = folder.display().to_string();
            }
        });
        let terminals = self.clone();
        let finished_chat = chat_id.to_string();
        tauri::async_runtime::spawn(async move {
            let _ = child.wait().await;
            let _ = standard.await;
            let _ = errors.await;
            terminals.with_terminals(|all| {
                if let Some(terminal) = all.get_mut(&finished_chat) {
                    let still_this_shell = terminal.shell.as_ref().is_some_and(|shell| shell.process_id == process_id);
                    if still_this_shell {
                        terminal.shell = None;
                    }
                }
            });
            notify(TerminalEvent::Folder { chat_id: finished_chat.clone(), folder: terminals.view(&finished_chat).folder });
        });
        let _ = sender.send(PREPARE_SHELL.to_string());
        Ok(sender)
    }

    pub fn run(&self, chat_id: &str, typed: &str, folder: Option<PathBuf>, notify: TerminalNotifier) -> Result<(), String> {
        let existing = self.with_terminals(|terminals| terminals.get(chat_id).and_then(|terminal| terminal.shell.as_ref().map(|shell| shell.input.clone())));
        let input = match existing {
            Some(input) => input,
            None => self.start_shell(chat_id, folder, notify.clone())?,
        };
        let echo = format!("> {typed}\n");
        self.remember_output(chat_id, &echo);
        notify(TerminalEvent::Output { chat_id: chat_id.to_string(), text: echo });
        input.send(command_line_for_shell(typed)).map_err(|_| "Терминал закрылся, попробуй ещё раз".to_string())?;
        input.send(folder_report_line()).map_err(|_| "Терминал закрылся, попробуй ещё раз".to_string())
    }

    pub async fn stop(&self, chat_id: &str) {
        let process_id = self.with_terminals(|terminals| terminals.get_mut(chat_id).and_then(|terminal| terminal.shell.take()).and_then(|shell| shell.process_id));
        if let Some(process_id) = process_id {
            kill_process_tree(process_id).await;
        }
    }

    pub fn clear(&self, chat_id: &str) {
        self.with_terminals(|terminals| {
            if let Some(terminal) = terminals.get_mut(chat_id) {
                terminal.history.clear();
            }
        });
    }

    pub fn stop_all_now(&self) {
        let process_ids: Vec<u32> = self.with_terminals(|terminals| {
            terminals.values_mut().filter_map(|terminal| terminal.shell.take()).filter_map(|shell| shell.process_id).collect()
        });
        for process_id in process_ids {
            kill_process_tree_now(process_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn folder_marker_is_taken_out_even_when_split() {
        let mut reader = MarkerReader::default();
        let first = reader.push("привет\n<<TARN");
        assert_eq!(first.shown, "привет\n");
        assert!(first.folder.is_none());
        let second = reader.push("_CWD>>C:\\Проект\r\nдальше");
        assert_eq!(second.folder.as_deref(), Some("C:\\Проект"));
        assert_eq!(second.shown, "дальше");
    }

    #[test]
    fn typed_text_reaches_the_shell_untouched() {
        let typed = "Write-Output 'кавычка \" и $x'";
        let line = command_line_for_shell(typed);
        assert!(line.is_ascii());
        let encoded = line.split('\'').nth(1).unwrap();
        let decoded = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), typed);
    }

    #[test]
    fn the_shell_keeps_its_folder_between_commands() {
        let terminals = UserTerminals::default();
        let notify: TerminalNotifier = Arc::new(|_| {});
        let folder = std::env::temp_dir().join(format!("tarn-terminal-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(folder.join("внутри")).unwrap();
        tauri::async_runtime::block_on(async {
            terminals.run("c", "cd внутри", Some(folder.clone()), notify.clone()).unwrap();
            terminals.run("c", "Write-Output 'готово'", None, notify.clone()).unwrap();
            for _ in 0..100 {
                if terminals.view("c").text.contains("готово\n") || terminals.view("c").text.contains("готово\r\n") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            for _ in 0..30 {
                if terminals.view("c").folder.ends_with("внутри") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let view = terminals.view("c");
            assert!(view.text.contains("готово"), "{}", view.text);
            assert!(view.folder.ends_with("внутри"), "{}", view.folder);
            assert!(!view.text.contains(FOLDER_MARKER));
            terminals.stop("c").await;
        });
        let _ = std::fs::remove_dir_all(folder);
    }
}
