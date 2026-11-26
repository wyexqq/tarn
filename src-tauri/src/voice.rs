use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::oneshot;

use crate::speech_worker::{encode_request, WorkerReply, WORKER_FLAG};
use crate::tools::hide_console_window;

const ENGINE_URL: &str = "https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.2/whisper-bin-x64.zip";
const ENGINE_SHA256: &str = "49dcc16de826f20bd53d44f947a1ae49dfa81f86cad67a64d80820cb192d674a";
const ENGINE_FILE: &str = "parakeet.dll";
const ENGINE_OVERRIDE_VARIABLE: &str = "TARN_SPEECH_CLI";
const MODEL_URL: &str = "https://huggingface.co/ggml-org/parakeet-GGUF/resolve/main/ggml-parakeet-tdt-0.6b-v3-q4_k.bin";
const MODEL_SHA256: &str = "8b205b8b39c6535e153de6fb11c51db46125d45c4f16ba496fe41a0fe71b885e";
const MODEL_FILE: &str = "ggml-parakeet-tdt-0.6b-v3-q4_k.bin";
const REPLACED_MODEL_FILES: [&str; 2] = ["ggml-base-q5_1.bin", "ggml-small-q5_1.bin"];
const MAX_RECOGNITION_TIME: Duration = Duration::from_secs(600);
const WAV_HEADER_BYTES: usize = 44;
const WORKER_STOPPED_MESSAGE: &str = "Распознавание остановилось";
#[cfg(windows)]
const QUIET_BACKGROUND_PROCESS_FLAGS: u32 = 0x0800_0000 | 0x0000_4000;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    pub is_ready: bool,
    pub is_engine_installed: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VoiceProgress {
    pub stage: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RecognitionRequest {
    pub wav_base64: String,
    pub is_live: bool,
}

pub fn voice_folder(data_dir: &Path) -> PathBuf {
    data_dir.join("voice")
}

fn engine_folder(voice_dir: &Path) -> PathBuf {
    voice_dir.join("engine")
}

fn models_folder(voice_dir: &Path) -> PathBuf {
    voice_dir.join("models")
}

fn model_path(voice_dir: &Path) -> PathBuf {
    models_folder(voice_dir).join(MODEL_FILE)
}

fn engine_path(voice_dir: &Path) -> Option<PathBuf> {
    if let Ok(path) = std::env::var(ENGINE_OVERRIDE_VARIABLE) {
        return Some(PathBuf::from(path));
    }
    walkdir::WalkDir::new(engine_folder(voice_dir))
        .into_iter()
        .filter_map(Result::ok)
        .find(|entry| entry.file_name() == ENGINE_FILE)
        .map(|entry| entry.into_path())
}

pub fn status(voice_dir: &Path) -> VoiceStatus {
    let is_engine_installed = engine_path(voice_dir).is_some();
    VoiceStatus {
        is_ready: is_engine_installed && model_path(voice_dir).exists(),
        is_engine_installed,
    }
}

async fn download_verified(
    http: &reqwest::Client,
    url: &str,
    expected_sha256: &str,
    destination: &Path,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let response = http.get(url).send().await.map_err(|error| format!("Не удалось начать загрузку: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("Сервер загрузки вернул ошибку {}", response.status().as_u16()));
    }
    let total = response.content_length().unwrap_or(0);
    let unfinished = destination.with_extension("part");
    let mut file = tokio::fs::File::create(&unfinished).await.map_err(|error| format!("Не удалось создать файл: {error}"))?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|error| format!("Загрузка оборвалась: {error}"))?;
        hasher.update(&bytes);
        file.write_all(&bytes).await.map_err(|error| format!("Не удалось записать файл: {error}"))?;
        done += bytes.len() as u64;
        on_progress(done, total);
    }
    file.flush().await.map_err(|error| format!("Не удалось записать файл: {error}"))?;
    drop(file);
    let actual: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    if actual != expected_sha256 {
        let _ = tokio::fs::remove_file(&unfinished).await;
        return Err("Скачанный файл повреждён или подменён. Попробуй ещё раз".to_string());
    }
    tokio::fs::rename(&unfinished, destination).await.map_err(|error| format!("Не удалось сохранить файл: {error}"))
}

fn powershell_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

async fn extract_archive(archive: &Path, destination: &Path) -> Result<(), String> {
    let script = format!(
        "Expand-Archive -LiteralPath {} -DestinationPath {} -Force",
        powershell_literal(&archive.to_string_lossy()),
        powershell_literal(&destination.to_string_lossy()),
    );
    let mut command = Command::new("powershell");
    command.args(["-NoProfile", "-NonInteractive", "-Command", &script]).stdout(Stdio::null()).stderr(Stdio::null());
    hide_console_window(&mut command);
    let status = command.status().await.map_err(|error| format!("Не удалось распаковать: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("Не удалось распаковать программу распознавания".to_string())
    }
}

async fn remove_replaced_models(voice_dir: &Path) {
    for file_name in REPLACED_MODEL_FILES {
        let _ = tokio::fs::remove_file(models_folder(voice_dir).join(file_name)).await;
    }
}

pub async fn install(http: &reqwest::Client, voice_dir: &Path, mut on_progress: impl FnMut(VoiceProgress)) -> Result<(), String> {
    tokio::fs::create_dir_all(models_folder(voice_dir)).await.map_err(|error| format!("Не удалось создать папку: {error}"))?;
    if engine_path(voice_dir).is_none() {
        let archive = voice_dir.join("engine.zip");
        download_verified(http, ENGINE_URL, ENGINE_SHA256, &archive, |done_bytes, total_bytes| {
            on_progress(VoiceProgress { stage: "engine".to_string(), done_bytes, total_bytes });
        })
        .await?;
        extract_archive(&archive, &engine_folder(voice_dir)).await?;
        let _ = tokio::fs::remove_file(&archive).await;
        if engine_path(voice_dir).is_none() {
            return Err("В архиве нет программы распознавания".to_string());
        }
    }
    if !model_path(voice_dir).exists() {
        download_verified(http, MODEL_URL, MODEL_SHA256, &model_path(voice_dir), |done_bytes, total_bytes| {
            on_progress(VoiceProgress { stage: "model".to_string(), done_bytes, total_bytes });
        })
        .await?;
    }
    remove_replaced_models(voice_dir).await;
    Ok(())
}

fn worker_command(voice_dir: &Path) -> Result<Command, String> {
    let engine = engine_path(voice_dir).ok_or("Распознавание речи не установлено")?;
    if std::env::var_os(ENGINE_OVERRIDE_VARIABLE).is_some() {
        return Ok(Command::new(engine));
    }
    let engine_directory = engine.parent().ok_or("Распознавание речи не установлено")?;
    let application = std::env::current_exe().map_err(|error| format!("Не удалось запустить распознавание: {error}"))?;
    let mut command = Command::new(application);
    command.arg(WORKER_FLAG).arg(engine_directory).arg(model_path(voice_dir));
    Ok(command)
}

fn run_quietly_in_background(command: &mut Command) {
    #[cfg(windows)]
    command.creation_flags(QUIET_BACKGROUND_PROCESS_FLAGS);
    #[cfg(not(windows))]
    hide_console_window(command);
}

type PendingReplies = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<String, String>>>>>;

struct RunningWorker {
    input: tokio::sync::Mutex<ChildStdin>,
    pending: PendingReplies,
    next_id: AtomicU64,
    is_alive: Arc<AtomicBool>,
    _process: Child,
}

fn result_of_reply(reply: WorkerReply) -> Result<String, String> {
    match reply.error {
        Some(error) => Err(format!("Ошибка распознавания: {error}")),
        None => Ok(reply.text.unwrap_or_default()),
    }
}

async fn deliver_replies(output: ChildStdout, pending: PendingReplies, is_alive: Arc<AtomicBool>) {
    let mut lines = BufReader::new(output).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(reply) = serde_json::from_str::<WorkerReply>(&line) else {
            continue;
        };
        let waiting = pending.lock().ok().and_then(|mut replies| replies.remove(&reply.id));
        if let Some(waiting) = waiting {
            let _ = waiting.send(result_of_reply(reply));
        }
    }
    is_alive.store(false, Ordering::SeqCst);
    if let Ok(mut replies) = pending.lock() {
        for (_, waiting) in replies.drain() {
            let _ = waiting.send(Err(WORKER_STOPPED_MESSAGE.to_string()));
        }
    }
}

fn start_worker(voice_dir: &Path) -> Result<RunningWorker, String> {
    if !model_path(voice_dir).exists() {
        return Err("Модель распознавания не скачана".to_string());
    }
    let mut command = worker_command(voice_dir)?;
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    run_quietly_in_background(&mut command);
    let mut process = command.spawn().map_err(|error| format!("Не удалось запустить распознавание: {error}"))?;
    let input = process.stdin.take().ok_or(WORKER_STOPPED_MESSAGE)?;
    let output = process.stdout.take().ok_or(WORKER_STOPPED_MESSAGE)?;
    let pending: PendingReplies = Arc::new(Mutex::new(HashMap::new()));
    let is_alive = Arc::new(AtomicBool::new(true));
    tauri::async_runtime::spawn(deliver_replies(output, pending.clone(), is_alive.clone()));
    Ok(RunningWorker {
        input: tokio::sync::Mutex::new(input),
        pending,
        next_id: AtomicU64::new(1),
        is_alive,
        _process: process,
    })
}

impl RunningWorker {
    async fn recognize(&self, is_live: bool, pcm: &[u8]) -> Result<String, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().map_err(|_| WORKER_STOPPED_MESSAGE)?.insert(id, sender);
        let request = encode_request(id, is_live, pcm);
        let written = {
            let mut input = self.input.lock().await;
            match input.write_all(&request).await {
                Ok(()) => input.flush().await,
                Err(error) => Err(error),
            }
        };
        if written.is_err() {
            if let Ok(mut replies) = self.pending.lock() {
                replies.remove(&id);
            }
            return Err(WORKER_STOPPED_MESSAGE.to_string());
        }
        let reply = tokio::time::timeout(MAX_RECOGNITION_TIME, receiver)
            .await
            .map_err(|_| "Распознавание заняло больше 10 минут".to_string())?;
        reply.map_err(|_| WORKER_STOPPED_MESSAGE.to_string())?
    }
}

#[derive(Default)]
pub struct SpeechService {
    running: tokio::sync::Mutex<Option<Arc<RunningWorker>>>,
}

impl SpeechService {
    async fn worker(&self, voice_dir: &Path) -> Result<Arc<RunningWorker>, String> {
        let mut running = self.running.lock().await;
        if let Some(worker) = running.as_ref().filter(|worker| worker.is_alive.load(Ordering::SeqCst)) {
            return Ok(worker.clone());
        }
        let worker = Arc::new(start_worker(voice_dir)?);
        *running = Some(worker.clone());
        Ok(worker)
    }

    pub async fn prepare(&self, voice_dir: &Path) -> Result<(), String> {
        self.worker(voice_dir).await.map(|_| ())
    }

    pub async fn transcribe(&self, voice_dir: &Path, request: &RecognitionRequest) -> Result<String, String> {
        let audio = base64::engine::general_purpose::STANDARD.decode(&request.wav_base64).map_err(|_| "Запись повреждена".to_string())?;
        let pcm = audio.get(WAV_HEADER_BYTES..).unwrap_or_default();
        self.worker(voice_dir).await?.recognize(request.is_live, pcm).await
    }

    pub async fn release(&self) {
        self.running.lock().await.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_voice_folder() -> PathBuf {
        let voice_dir = std::env::temp_dir().join(format!("tarn-voice-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(models_folder(&voice_dir)).unwrap();
        voice_dir
    }

    #[test]
    fn worker_reply_becomes_text_or_error() {
        assert_eq!(result_of_reply(WorkerReply { id: 1, text: Some("Привет".to_string()), error: None }), Ok("Привет".to_string()));
        assert_eq!(result_of_reply(WorkerReply { id: 1, text: None, error: None }), Ok(String::new()));
        assert!(result_of_reply(WorkerReply { id: 1, text: None, error: Some("код 1".to_string()) }).unwrap_err().contains("код 1"));
    }

    #[test]
    fn model_file_decides_whether_speech_is_ready_for_a_missing_engine_too() {
        let voice_dir = temporary_voice_folder();
        std::env::remove_var(ENGINE_OVERRIDE_VARIABLE);
        assert!(!status(&voice_dir).is_ready);
        assert!(!status(&voice_dir).is_engine_installed);
        std::fs::write(model_path(&voice_dir), "x").unwrap();
        assert!(!status(&voice_dir).is_ready);
        let engine_dir = engine_folder(&voice_dir).join("Release");
        std::fs::create_dir_all(&engine_dir).unwrap();
        std::fs::write(engine_dir.join(ENGINE_FILE), "x").unwrap();
        assert!(status(&voice_dir).is_ready);
    }

    #[tokio::test]
    async fn replaced_models_are_removed_and_the_current_one_stays() {
        let voice_dir = temporary_voice_folder();
        std::fs::write(models_folder(&voice_dir).join(REPLACED_MODEL_FILES[0]), "x").unwrap();
        std::fs::write(model_path(&voice_dir), "x").unwrap();
        remove_replaced_models(&voice_dir).await;
        assert!(!models_folder(&voice_dir).join(REPLACED_MODEL_FILES[0]).exists());
        assert!(model_path(&voice_dir).exists());
    }

    #[test]
    fn single_quotes_in_paths_are_escaped_for_powershell() {
        assert_eq!(powershell_literal("C:\\it's"), "'C:\\it''s'");
    }
}
