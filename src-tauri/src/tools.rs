use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::sync::Notify;
use walkdir::WalkDir;

use crate::providers::ToolSpec;

const MAX_READ_BYTES: u64 = 5 * 1024 * 1024;
const MAX_RESULT_CHARS: usize = 60_000;
const DEFAULT_READ_LINES: usize = 2_000;
const MAX_LISTED_ENTRIES: usize = 500;
const MAX_SEARCH_MATCHES: usize = 200;
const MAX_SEARCHED_FILE_BYTES: u64 = 1024 * 1024;
const DEFAULT_COMMAND_SECONDS: u64 = 120;
const MAX_COMMAND_SECONDS: u64 = 600;
pub const IGNORED_FOLDERS: [&str; 6] = [".git", "node_modules", "target", "dist", "__pycache__", ".venv"];
const MAX_CAPTURED_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const TOOL_READ_FILE: &str = "read_file";
pub const TOOL_LIST_FILES: &str = "list_files";
pub const TOOL_SEARCH_TEXT: &str = "search_text";
pub const TOOL_WRITE_FILE: &str = "write_file";
pub const TOOL_EDIT_FILE: &str = "edit_file";
pub const TOOL_RUN_COMMAND: &str = "run_command";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessMode {
    Chat,
    Plan,
    Ask,
    AcceptEdits,
    Auto,
    Full,
}

impl AccessMode {
    pub fn parse(value: &str) -> AccessMode {
        match value {
            "plan" => AccessMode::Plan,
            "ask" => AccessMode::Ask,
            "acceptEdits" => AccessMode::AcceptEdits,
            "auto" => AccessMode::Auto,
            "full" => AccessMode::Full,
            _ => AccessMode::Chat,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            AccessMode::Chat => "компьютер пользователя недоступен",
            AccessMode::Plan => "можно только читать файлы и составлять план, ничего не менять",
            AccessMode::Ask => "читать можно сразу, каждое изменение подтверждает пользователь",
            AccessMode::AcceptEdits => "файлы в папке можно менять сразу, команды подтверждает пользователь",
            AccessMode::Auto => "файлы в папке можно менять сразу, рискованные команды подтверждает пользователь",
            AccessMode::Full => "можно всё без подтверждений",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolRisk {
    Read,
    Edit,
    Command(String),
    External { server: String, read_only: bool },
}

impl ToolRisk {
    pub fn permission_scope(&self) -> String {
        match self {
            ToolRisk::Read => "read".to_string(),
            ToolRisk::Edit => "edit".to_string(),
            ToolRisk::Command(_) => "command".to_string(),
            ToolRisk::External { server, .. } => format!("external:{server}"),
        }
    }
}

const COMMAND_INJECTION_SYMBOLS: [char; 13] = [';', '|', '&', '>', '<', '`', '$', '(', ')', '{', '}', '@', '\n'];
const OUTSIDE_FOLDER_FRAGMENTS: [&str; 5] = ["..", ":", "~", " /", " \\"];
const WRITING_FLAGS: [&str; 3] = ["--output", "--pre", "--ext-diff"];
const EVERYDAY_COMMAND_PREFIXES: [&str; 15] = [
    "ls", "dir", "pwd", "cat", "type", "rg", "grep", "findstr",
    "get-childitem", "get-content", "get-location", "select-string",
    "git status", "git log", "git diff",
];

pub fn command_is_everyday(command: &str) -> bool {
    let normalized = command.trim().to_lowercase();
    let can_do_more_than_read = normalized.contains(COMMAND_INJECTION_SYMBOLS)
        || OUTSIDE_FOLDER_FRAGMENTS.iter().any(|fragment| normalized.contains(fragment))
        || WRITING_FLAGS.iter().any(|flag| normalized.contains(flag));
    if can_do_more_than_read {
        return false;
    }
    EVERYDAY_COMMAND_PREFIXES.iter().any(|prefix| {
        normalized == *prefix || normalized.strip_prefix(prefix).is_some_and(|rest| rest.starts_with(' '))
    })
}

pub fn needs_approval(mode: AccessMode, risk: &ToolRisk) -> bool {
    match (mode, risk) {
        (AccessMode::Full, _) => false,
        (_, ToolRisk::Read) => false,
        (_, ToolRisk::External { read_only: true, .. }) => false,
        (AccessMode::AcceptEdits | AccessMode::Auto, ToolRisk::Edit) => false,
        (AccessMode::Auto, ToolRisk::Command(command)) => !command_is_everyday(command),
        _ => true,
    }
}

pub fn is_allowed_at_all(mode: AccessMode, risk: &ToolRisk) -> bool {
    match mode {
        AccessMode::Chat => false,
        AccessMode::Plan => matches!(risk, ToolRisk::Read | ToolRisk::External { read_only: true, .. }),
        _ => true,
    }
}

pub fn can_change_more_than_saved_files(risk: &ToolRisk) -> bool {
    match risk {
        ToolRisk::Read | ToolRisk::Edit => false,
        ToolRisk::Command(command) => !command_is_everyday(command),
        ToolRisk::External { read_only, .. } => !read_only,
    }
}

pub fn risk_of_builtin(name: &str, input: &Value) -> Option<ToolRisk> {
    match name {
        TOOL_READ_FILE | TOOL_LIST_FILES | TOOL_SEARCH_TEXT => Some(ToolRisk::Read),
        TOOL_WRITE_FILE | TOOL_EDIT_FILE => Some(ToolRisk::Edit),
        TOOL_RUN_COMMAND => Some(ToolRisk::Command(input["command"].as_str().unwrap_or_default().to_string())),
        _ => None,
    }
}

pub struct EnabledTools {
    pub read: bool,
    pub edit: bool,
    pub command: bool,
}

pub fn spec(name: &str, description: &str, schema: Value) -> ToolSpec {
    ToolSpec { name: name.to_string(), description: description.to_string(), schema }
}

pub fn builtin_specs(mode: AccessMode, enabled: &EnabledTools) -> Vec<ToolSpec> {
    let mut specs = Vec::new();
    if mode == AccessMode::Chat {
        return specs;
    }
    if enabled.read {
        specs.push(spec(
            TOOL_READ_FILE,
            "Прочитать текстовый файл. Относительный путь считается от рабочей папки.",
            json!({ "type": "object", "properties": {
                "path": { "type": "string" },
                "offset": { "type": "integer", "description": "С какой строки начать, от 1" },
                "limit": { "type": "integer", "description": "Сколько строк прочитать" }
            }, "required": ["path"] }),
        ));
        specs.push(spec(
            TOOL_LIST_FILES,
            "Показать файлы и папки. Служебные папки вроде node_modules и .git пропускаются.",
            json!({ "type": "object", "properties": {
                "path": { "type": "string", "description": "По умолчанию рабочая папка" },
                "recursive": { "type": "boolean" }
            } }),
        ));
        specs.push(spec(
            TOOL_SEARCH_TEXT,
            "Найти текст в файлах без учёта регистра. Показывает файл, строку и её содержимое.",
            json!({ "type": "object", "properties": {
                "query": { "type": "string" },
                "path": { "type": "string", "description": "По умолчанию рабочая папка" }
            }, "required": ["query"] }),
        ));
    }
    if mode == AccessMode::Plan {
        return specs;
    }
    if enabled.edit {
        specs.push(spec(
            TOOL_WRITE_FILE,
            "Создать файл или полностью заменить его содержимое.",
            json!({ "type": "object", "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            }, "required": ["path", "content"] }),
        ));
        specs.push(spec(
            TOOL_EDIT_FILE,
            "Заменить точный фрагмент текста в файле. Фрагмент должен встречаться один раз, иначе укажи replace_all.",
            json!({ "type": "object", "properties": {
                "path": { "type": "string" },
                "old_string": { "type": "string" },
                "new_string": { "type": "string" },
                "replace_all": { "type": "boolean" }
            }, "required": ["path", "old_string", "new_string"] }),
        ));
    }
    if enabled.command {
        specs.push(spec(
            TOOL_RUN_COMMAND,
            "Выполнить команду PowerShell в рабочей папке и вернуть её вывод.",
            json!({ "type": "object", "properties": {
                "command": { "type": "string" },
                "timeout_seconds": { "type": "integer", "description": "По умолчанию 120, максимум 600" }
            }, "required": ["command"] }),
        ));
    }
    specs
}

pub type OutputSink = Arc<dyn Fn(&str) + Send + Sync>;

pub struct ToolContext {
    pub mode: AccessMode,
    pub folder: Option<PathBuf>,
    pub extra_read_roots: Vec<PathBuf>,
    pub output_sink: Option<OutputSink>,
}

fn clean_path_text(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

fn canonicalize_allowing_new_file(path: &Path) -> Result<PathBuf, String> {
    let mut existing = path;
    let mut missing_parts: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        if let Ok(canonical) = existing.canonicalize() {
            let mut full = canonical;
            for part in missing_parts.iter().rev() {
                full.push(part);
            }
            return Ok(full);
        }
        let Some(name) = existing.file_name() else {
            return Err(format!("Не удалось разобрать путь {}", path.display()));
        };
        missing_parts.push(name);
        existing = existing.parent().ok_or_else(|| format!("Не удалось разобрать путь {}", path.display()))?;
    }
}

fn root_contains(root: &Path, canonical: &Path) -> bool {
    root.canonicalize().is_ok_and(|canonical_root| canonical.starts_with(canonical_root))
}

fn resolve_path(raw: &str, context: &ToolContext, for_reading: bool) -> Result<PathBuf, String> {
    let given = Path::new(raw);
    if given.components().any(|part| matches!(part, Component::ParentDir)) && !given.is_absolute() && context.mode != AccessMode::Full {
        return Err("Путь не может выходить за пределы рабочей папки".to_string());
    }
    let joined = if given.is_absolute() {
        given.to_path_buf()
    } else {
        context.folder.as_ref().ok_or("Рабочая папка не выбрана. Укажи полный путь")?.join(given)
    };
    let canonical = canonicalize_allowing_new_file(&joined)?;
    if context.mode == AccessMode::Full {
        return Ok(canonical);
    }
    let in_folder = context.folder.as_ref().is_some_and(|folder| root_contains(folder, &canonical));
    let in_read_root = for_reading && context.extra_read_roots.iter().any(|root| root_contains(root, &canonical));
    if in_folder || in_read_root {
        Ok(canonical)
    } else {
        Err(format!("{} вне рабочей папки. В этом режиме туда нельзя", clean_path_text(&canonical)))
    }
}

pub fn readable_path(raw: &str, context: &ToolContext) -> Result<PathBuf, String> {
    resolve_path(raw, context, true)
}

pub fn path_changed_by(name: &str, input: &Value, context: &ToolContext) -> Option<PathBuf> {
    if name != TOOL_WRITE_FILE && name != TOOL_EDIT_FILE {
        return None;
    }
    resolve_path(input["path"].as_str()?, context, false).ok()
}

fn limit_result(text: String) -> String {
    match text.char_indices().nth(MAX_RESULT_CHARS) {
        Some((index, _)) => format!("{}\n… результат обрезан", &text[..index]),
        None => text,
    }
}

fn required_text<'a>(input: &'a Value, field: &str) -> Result<&'a str, String> {
    input[field].as_str().ok_or_else(|| format!("Не указано поле {field}"))
}

fn is_ignored_entry(entry: &walkdir::DirEntry) -> bool {
    entry.file_type().is_dir() && entry.depth() > 0 && IGNORED_FOLDERS.iter().any(|name| entry.file_name() == *name)
}

fn read_text_file(path: &Path) -> Result<String, String> {
    let metadata = std::fs::metadata(path).map_err(|_| format!("Файл не найден: {}", clean_path_text(path)))?;
    if metadata.is_dir() {
        return Err(format!("{} это папка", clean_path_text(path)));
    }
    if metadata.len() > MAX_READ_BYTES {
        return Err(format!("{} больше 5 МБ", clean_path_text(path)));
    }
    let bytes = std::fs::read(path).map_err(|error| format!("Не удалось прочитать файл: {error}"))?;
    String::from_utf8(bytes).map_err(|_| format!("{} не текстовый файл", clean_path_text(path)))
}

fn read_file(input: &Value, context: &ToolContext) -> Result<String, String> {
    let path = resolve_path(required_text(input, "path")?, context, true)?;
    let text = read_text_file(&path)?;
    let offset = input["offset"].as_u64().unwrap_or(1).max(1) as usize;
    let limit = input["limit"].as_u64().map_or(DEFAULT_READ_LINES, |value| value as usize);
    let selected: Vec<&str> = text.lines().skip(offset - 1).take(limit).collect();
    Ok(limit_result(selected.join("\n")))
}

fn list_files(input: &Value, context: &ToolContext) -> Result<String, String> {
    let path = resolve_path(input["path"].as_str().unwrap_or("."), context, true)?;
    let max_depth = if input["recursive"].as_bool().unwrap_or(false) { usize::MAX } else { 1 };
    let walker = WalkDir::new(&path).max_depth(max_depth).sort_by_file_name().into_iter();
    let mut lines = Vec::new();
    for entry in walker.filter_entry(|entry| !is_ignored_entry(entry)).filter_map(Result::ok).skip(1).take(MAX_LISTED_ENTRIES) {
        let relative = entry.path().strip_prefix(&path).unwrap_or(entry.path());
        let suffix = if entry.file_type().is_dir() { "/" } else { "" };
        lines.push(format!("{}{suffix}", relative.display()));
    }
    if lines.is_empty() {
        return Ok("Папка пуста".to_string());
    }
    Ok(lines.join("\n"))
}

fn search_text(input: &Value, context: &ToolContext) -> Result<String, String> {
    let query = required_text(input, "query")?.to_lowercase();
    let path = resolve_path(input["path"].as_str().unwrap_or("."), context, true)?;
    let mut matches = Vec::new();
    let walker = WalkDir::new(&path).into_iter().filter_entry(|entry| !is_ignored_entry(entry));
    'files: for entry in walker.filter_map(Result::ok).filter(|entry| entry.file_type().is_file()) {
        let is_small = entry.metadata().is_ok_and(|metadata| metadata.len() <= MAX_SEARCHED_FILE_BYTES);
        if !is_small {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&query) {
                let relative = entry.path().strip_prefix(&path).unwrap_or(entry.path());
                matches.push(format!("{}:{}: {}", relative.display(), number + 1, line.trim()));
                if matches.len() >= MAX_SEARCH_MATCHES {
                    break 'files;
                }
            }
        }
    }
    if matches.is_empty() {
        return Ok("Ничего не найдено".to_string());
    }
    Ok(limit_result(matches.join("\n")))
}

fn write_file(input: &Value, context: &ToolContext) -> Result<String, String> {
    let path = resolve_path(required_text(input, "path")?, context, false)?;
    let content = required_text(input, "content")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("Не удалось создать папку: {error}"))?;
    }
    std::fs::write(&path, content).map_err(|error| format!("Не удалось записать файл: {error}"))?;
    Ok(format!("Записан файл {}", clean_path_text(&path)))
}

fn edit_file(input: &Value, context: &ToolContext) -> Result<String, String> {
    let path = resolve_path(required_text(input, "path")?, context, false)?;
    let old_text = required_text(input, "old_string")?;
    let new_text = required_text(input, "new_string")?;
    if old_text.is_empty() {
        return Err("Заменяемый фрагмент пуст".to_string());
    }
    let current = read_text_file(&path)?;
    let occurrences = current.matches(old_text).count();
    let replace_all = input["replace_all"].as_bool().unwrap_or(false);
    if occurrences == 0 {
        return Err("Фрагмент не найден в файле".to_string());
    }
    if occurrences > 1 && !replace_all {
        return Err(format!("Фрагмент встречается {occurrences} раз. Уточни его или укажи replace_all"));
    }
    std::fs::write(&path, current.replace(old_text, new_text)).map_err(|error| format!("Не удалось записать файл: {error}"))?;
    Ok(format!("Изменён файл {}, замен: {occurrences}", clean_path_text(&path)))
}

pub fn hide_console_window(command: &mut Command) {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    #[cfg(not(windows))]
    let _ = command;
}

pub fn kill_process_tree_now(process_id: u32) {
    let mut killer = std::process::Command::new("taskkill");
    killer.args(["/T", "/F", "/PID", &process_id.to_string()]).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        killer.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = killer.status();
}

#[derive(Default)]
pub struct Utf8Chunker {
    unfinished: Vec<u8>,
}

impl Utf8Chunker {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.unfinished.extend_from_slice(bytes);
        let complete_length = match std::str::from_utf8(&self.unfinished) {
            Ok(_) => self.unfinished.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => self.unfinished.len(),
        };
        let complete: Vec<u8> = self.unfinished.drain(..complete_length).collect();
        String::from_utf8_lossy(&complete).to_string()
    }
}

pub async fn kill_process_tree(process_id: u32) {
    let mut killer = Command::new("taskkill");
    killer.args(["/T", "/F", "/PID", &process_id.to_string()]).stdout(Stdio::null()).stderr(Stdio::null());
    hide_console_window(&mut killer);
    let _ = killer.status().await;
}

fn collect_limited(pipe: Option<impl AsyncRead + Unpin + Send + 'static>, sink: Option<OutputSink>) -> tauri::async_runtime::JoinHandle<Vec<u8>> {
    tauri::async_runtime::spawn(async move {
        let mut collected = Vec::new();
        let Some(mut pipe) = pipe else {
            return collected;
        };
        let mut chunker = Utf8Chunker::default();
        let mut chunk = [0u8; 8192];
        while let Ok(count) = pipe.read(&mut chunk).await {
            if count == 0 {
                break;
            }
            if let Some(sink) = &sink {
                sink(&chunker.push(&chunk[..count]));
            }
            let room = MAX_CAPTURED_OUTPUT_BYTES.saturating_sub(collected.len());
            collected.extend_from_slice(&chunk[..count.min(room)]);
        }
        collected
    })
}

async fn wait_for_output(reader: tauri::async_runtime::JoinHandle<Vec<u8>>) -> Vec<u8> {
    tokio::time::timeout(OUTPUT_DRAIN_TIMEOUT, reader)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default()
}

fn combined_output(standard: &[u8], errors: &[u8]) -> String {
    let standard = String::from_utf8_lossy(standard);
    let errors = String::from_utf8_lossy(errors);
    let mut text = standard.trim_end().to_string();
    if !errors.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(errors.trim_end());
    }
    if text.is_empty() {
        text = "Команда ничего не вывела".to_string();
    }
    limit_result(text)
}

async fn stop_process_tree(child: &mut Child) {
    if let Some(process_id) = child.id() {
        kill_process_tree(process_id).await;
    }
    let _ = child.kill().await;
}

async fn run_command(input: &Value, context: &ToolContext, cancel: &Arc<Notify>) -> Result<String, String> {
    let command_text = required_text(input, "command")?;
    if context.folder.is_none() && context.mode != AccessMode::Full {
        return Err("Рабочая папка не выбрана".to_string());
    }
    let seconds = input["timeout_seconds"].as_u64().unwrap_or(DEFAULT_COMMAND_SECONDS).clamp(1, MAX_COMMAND_SECONDS);
    let script = format!("[Console]::OutputEncoding = [Text.Encoding]::UTF8; {command_text}");
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    hide_console_window(&mut command);
    if let Some(folder) = &context.folder {
        command.current_dir(folder);
    }
    let mut child = command.spawn().map_err(|error| format!("Не удалось запустить команду: {error}"))?;
    let standard_reader = collect_limited(child.stdout.take(), context.output_sink.clone());
    let errors_reader = collect_limited(child.stderr.take(), context.output_sink.clone());
    let status = tokio::select! {
        result = child.wait() => result.map_err(|error| format!("Команда прервалась: {error}"))?,
        _ = tokio::time::sleep(Duration::from_secs(seconds)) => {
            stop_process_tree(&mut child).await;
            return Err(format!("Команда не завершилась за {seconds} с и остановлена"));
        }
        _ = cancel.notified() => {
            cancel.notify_one();
            stop_process_tree(&mut child).await;
            return Err("Команда остановлена пользователем".to_string());
        }
    };
    let standard = wait_for_output(standard_reader).await;
    let errors = wait_for_output(errors_reader).await;
    let text = combined_output(&standard, &errors);
    if status.success() {
        Ok(text)
    } else {
        Err(format!("Код выхода {}\n{text}", status.code().unwrap_or(-1)))
    }
}

pub async fn run_builtin(name: &str, input: &Value, context: &ToolContext, cancel: &Arc<Notify>) -> Result<String, String> {
    match name {
        TOOL_READ_FILE => read_file(input, context),
        TOOL_LIST_FILES => list_files(input, context),
        TOOL_SEARCH_TEXT => search_text(input, context),
        TOOL_WRITE_FILE => write_file(input, context),
        TOOL_EDIT_FILE => edit_file(input, context),
        TOOL_RUN_COMMAND => run_command(input, context, cancel).await,
        _ => Err(format!("Неизвестный инструмент {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_folder() -> PathBuf {
        let folder = std::env::temp_dir().join(format!("tarn-tools-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn context_in(folder: &Path, mode: AccessMode) -> ToolContext {
        ToolContext { mode, folder: Some(folder.to_path_buf()), extra_read_roots: Vec::new(), output_sink: None }
    }

    #[test]
    fn approval_follows_the_chosen_mode() {
        let edit = ToolRisk::Edit;
        let safe = ToolRisk::Command("git status".to_string());
        let risky = ToolRisk::Command("git push".to_string());
        assert!(!needs_approval(AccessMode::Ask, &ToolRisk::Read));
        assert!(needs_approval(AccessMode::Ask, &edit));
        assert!(!needs_approval(AccessMode::AcceptEdits, &edit));
        assert!(needs_approval(AccessMode::AcceptEdits, &safe));
        assert!(!needs_approval(AccessMode::Auto, &safe));
        assert!(needs_approval(AccessMode::Auto, &risky));
        assert!(!needs_approval(AccessMode::Full, &risky));
        assert!(!is_allowed_at_all(AccessMode::Plan, &edit));
        assert!(is_allowed_at_all(AccessMode::Plan, &ToolRisk::Read));
        assert!(!is_allowed_at_all(AccessMode::Chat, &ToolRisk::Read));
    }

    #[test]
    fn everyday_commands_cannot_smuggle_others() {
        assert!(command_is_everyday("git status"));
        assert!(command_is_everyday("git log -5"));
        assert!(command_is_everyday("Get-Content src/main.rs"));
        assert!(!command_is_everyday("git statusx"));
        assert!(!command_is_everyday("git status; Remove-Item -Recurse a"));
        assert!(!command_is_everyday("cat a.txt | Remove-Item"));
        assert!(!command_is_everyday("ls $(Remove-Item a)"));
        assert!(!command_is_everyday("ls (Remove-Item a)"));
        assert!(!command_is_everyday("Remove-Item a"));
        assert!(!command_is_everyday("cargo test"));
        assert!(!command_is_everyday("git diff --output=a.txt"));
        assert!(!command_is_everyday("rg --pre evil x"));
        assert!(!command_is_everyday("cat ../secret.txt"));
        assert!(!command_is_everyday("cat C:\\Users\\secret.txt"));
    }

    #[test]
    fn plan_mode_offers_only_reading_tools() {
        let enabled = EnabledTools { read: true, edit: true, command: true };
        let plan = builtin_specs(AccessMode::Plan, &enabled);
        assert_eq!(plan.len(), 3);
        assert!(plan.iter().all(|tool| tool.name != TOOL_WRITE_FILE && tool.name != TOOL_RUN_COMMAND));
        assert!(builtin_specs(AccessMode::Chat, &enabled).is_empty());
        assert_eq!(builtin_specs(AccessMode::Ask, &enabled).len(), 6);
        assert_eq!(builtin_specs(AccessMode::Ask, &EnabledTools { read: true, edit: false, command: false }).len(), 3);
    }

    #[test]
    fn files_outside_the_folder_are_refused() {
        let folder = temp_folder();
        let outside = temp_folder();
        let secret = outside.join("secret.txt");
        std::fs::write(&secret, "секрет").unwrap();
        let context = context_in(&folder, AccessMode::Ask);
        let absolute = json!({ "path": secret.to_string_lossy() });
        assert!(read_file(&absolute, &context).unwrap_err().contains("вне рабочей папки"));
        assert!(read_file(&json!({ "path": "../secret.txt" }), &context).is_err());
        assert!(write_file(&json!({ "path": secret.to_string_lossy(), "content": "x" }), &context).is_err());
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "секрет");
        let full = context_in(&folder, AccessMode::Full);
        assert_eq!(read_file(&absolute, &full).unwrap(), "секрет");
    }

    #[test]
    fn extra_read_roots_allow_reading_but_not_writing() {
        let folder = temp_folder();
        let skills = temp_folder();
        let skill_file = skills.join("SKILL.md");
        std::fs::write(&skill_file, "инструкция").unwrap();
        let context = ToolContext { mode: AccessMode::Ask, folder: Some(folder), extra_read_roots: vec![skills], output_sink: None };
        let path = skill_file.to_string_lossy().to_string();
        assert_eq!(read_file(&json!({ "path": path }), &context).unwrap(), "инструкция");
        assert!(write_file(&json!({ "path": path, "content": "x" }), &context).is_err());
    }

    #[test]
    fn only_file_tools_name_a_path_they_change() {
        let folder = temp_folder();
        let context = context_in(&folder, AccessMode::AcceptEdits);
        let input = json!({ "path": "new/a.txt", "content": "x" });
        assert!(path_changed_by(TOOL_WRITE_FILE, &input, &context).unwrap().ends_with("a.txt"));
        assert!(path_changed_by(TOOL_EDIT_FILE, &input, &context).is_some());
        assert!(path_changed_by(TOOL_READ_FILE, &input, &context).is_none());
        assert!(path_changed_by(TOOL_RUN_COMMAND, &input, &context).is_none());
    }

    #[test]
    fn everyday_commands_and_read_only_tools_leave_nothing_behind() {
        assert!(!can_change_more_than_saved_files(&ToolRisk::Read));
        assert!(!can_change_more_than_saved_files(&ToolRisk::Edit));
        assert!(!can_change_more_than_saved_files(&ToolRisk::Command("git status".to_string())));
        assert!(can_change_more_than_saved_files(&ToolRisk::Command("npm install".to_string())));
        assert!(!can_change_more_than_saved_files(&ToolRisk::External { server: "s".to_string(), read_only: true }));
        assert!(can_change_more_than_saved_files(&ToolRisk::External { server: "s".to_string(), read_only: false }));
    }

    #[test]
    fn writes_edits_lists_and_searches_in_the_folder() {
        let folder = temp_folder();
        let context = context_in(&folder, AccessMode::Full);
        write_file(&json!({ "path": "sub/a.txt", "content": "первая\nвторая\nвторая" }), &context).unwrap();
        assert_eq!(read_file(&json!({ "path": "sub/a.txt", "offset": 2, "limit": 1 }), &context).unwrap(), "вторая");
        assert!(edit_file(&json!({ "path": "sub/a.txt", "old_string": "вторая", "new_string": "x" }), &context).unwrap_err().contains("2 раз"));
        edit_file(&json!({ "path": "sub/a.txt", "old_string": "первая", "new_string": "новая" }), &context).unwrap();
        assert!(search_text(&json!({ "query": "НОВАЯ" }), &context).unwrap().contains(":1: новая"));
        std::fs::create_dir_all(folder.join("node_modules")).unwrap();
        std::fs::write(folder.join("node_modules").join("b.txt"), "новая").unwrap();
        let listing = list_files(&json!({ "recursive": true }), &context).unwrap();
        assert!(listing.contains("a.txt"));
        assert!(!listing.contains("b.txt"));
        assert_eq!(search_text(&json!({ "query": "новая" }), &context).unwrap().lines().count(), 1);
    }

    #[test]
    fn letters_split_between_chunks_arrive_whole() {
        let mut chunker = Utf8Chunker::default();
        let bytes = "привет".as_bytes();
        let first = chunker.push(&bytes[..3]);
        let second = chunker.push(&bytes[3..]);
        assert_eq!(first, "п");
        assert_eq!(format!("{first}{second}"), "привет");
    }

    #[test]
    fn command_output_is_shown_while_it_runs() {
        let folder = temp_folder();
        let seen = Arc::new(std::sync::Mutex::new(String::new()));
        let seen_by_sink = seen.clone();
        let sink: OutputSink = Arc::new(move |text: &str| seen_by_sink.lock().unwrap().push_str(text));
        let context = ToolContext { output_sink: Some(sink), ..context_in(&folder, AccessMode::Full) };
        let cancel = Arc::new(Notify::new());
        tauri::async_runtime::block_on(async {
            run_builtin(TOOL_RUN_COMMAND, &json!({ "command": "Write-Output 'живой вывод'" }), &context, &cancel).await.unwrap();
        });
        assert!(seen.lock().unwrap().contains("живой вывод"));
    }

    #[test]
    fn runs_command_in_folder_and_reports_failure() {
        let folder = temp_folder();
        let context = context_in(&folder, AccessMode::Full);
        let cancel = Arc::new(Notify::new());
        tauri::async_runtime::block_on(async {
            let printed = run_builtin(TOOL_RUN_COMMAND, &json!({ "command": "Write-Output 'привет'; (Get-Location).Path" }), &context, &cancel).await.unwrap();
            assert!(printed.contains("привет"), "{printed}");
            assert!(printed.contains(folder.file_name().unwrap().to_str().unwrap()), "{printed}");
            let failed = run_builtin(TOOL_RUN_COMMAND, &json!({ "command": "exit 3" }), &context, &cancel).await.unwrap_err();
            assert!(failed.contains("Код выхода 3"), "{failed}");
            cancel.notify_one();
            let stopped = run_builtin(TOOL_RUN_COMMAND, &json!({ "command": "Start-Sleep 30" }), &context, &cancel).await.unwrap_err();
            assert!(stopped.contains("остановлена"), "{stopped}");
        });
    }
}
