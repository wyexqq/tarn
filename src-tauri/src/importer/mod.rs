pub mod claude_code;
pub mod codex;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use walkdir::WalkDir;

use crate::db::{insert_image, Database, IMPORT_REFRESH_PENDING_SETTING};
use crate::images::StoredImage;
use crate::state::AppState;
use crate::types::{Attachment, ImportProgress, ImportRequest, ImportSource};

pub const SOURCE_CLAUDE_CODE: &str = "claude_code";
pub const SOURCE_CODEX: &str = "codex";

const WATCH_INTERVAL: Duration = Duration::from_secs(180);
const PROGRESS_EVERY_FILES: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedMessage {
    pub id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub content: String,
    pub parts: Value,
    pub attachments: Vec<Attachment>,
    pub thinking: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCompaction {
    pub up_to_message_id: String,
    pub summary: String,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedChat {
    pub id: String,
    pub title: String,
    pub cwd: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub leaf_id: Option<String>,
    pub messages: Vec<ParsedMessage>,
    pub images: Vec<StoredImage>,
    pub compactions: Vec<ParsedCompaction>,
}

pub fn path_setting_key(source: &str) -> String {
    format!("importPath.{source}")
}

fn default_source_path(source: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    if source == SOURCE_CODEX {
        home.join(".codex").join("sessions")
    } else {
        home.join(".claude").join("projects")
    }
}

pub fn claude_code_projects_folder(db: &Database) -> PathBuf {
    source_path(db, SOURCE_CLAUDE_CODE)
}

fn source_path(db: &Database, source: &str) -> PathBuf {
    db.get_setting(&path_setting_key(source))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_source_path(source))
}

pub fn jsonl_files(source: &str, root: &Path) -> Vec<PathBuf> {
    let max_depth = if source == SOURCE_CLAUDE_CODE { 2 } else { 8 };
    WalkDir::new(root)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "jsonl"))
        .map(|entry| entry.into_path())
        .collect()
}

pub fn scan_sources(state: &AppState) -> Result<Vec<ImportSource>, String> {
    let mut sources = Vec::new();
    for source in [SOURCE_CLAUDE_CODE, SOURCE_CODEX] {
        let (path, imported_chats) = state.with_db(|db| Ok((source_path(db, source), db.imported_chat_count(source)?)))?;
        let exists = path.is_dir();
        let files = if exists { jsonl_files(source, &path) } else { Vec::new() };
        let total_bytes = files
            .iter()
            .filter_map(|file| std::fs::metadata(file).ok())
            .map(|metadata| metadata.len())
            .sum();
        sources.push(ImportSource {
            source: source.to_string(),
            path: path.to_string_lossy().to_string(),
            exists,
            file_count: files.len(),
            total_bytes,
            imported_chats,
        });
    }
    Ok(sources)
}

fn file_signature(path: &Path) -> Option<(i64, i64)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((metadata.len() as i64, modified))
}

fn file_is_current(connection: &Connection, path: &str, signature: (i64, i64)) -> bool {
    connection
        .query_row(
            "SELECT size, modified FROM import_files WHERE path = ?1",
            [path],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .ok()
        .flatten()
        .is_some_and(|stored| stored == signature)
}

pub fn store_parsed_chat(connection: &mut Connection, source: &str, source_file: &str, signature: (i64, i64), chat: &ParsedChat) -> Result<bool, rusqlite::Error> {
    let transaction = connection.transaction()?;
    let deleted: bool = transaction
        .query_row("SELECT deleted FROM chats WHERE id = ?1", [&chat.id], |row| row.get::<_, i64>(0))
        .optional()?
        .is_some_and(|value| value != 0);
    let should_store = !deleted && !chat.messages.is_empty();
    if should_store {
        transaction.execute(
            "INSERT INTO chats(id, title, source, source_path, cwd, active_leaf_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                title = CASE WHEN chats.title_locked = 1 THEN chats.title ELSE excluded.title END,
                source_path = excluded.source_path,
                cwd = CASE WHEN chats.access_mode = 'chat' THEN excluded.cwd ELSE chats.cwd END,
                updated_at = MAX(chats.updated_at, excluded.updated_at),
                active_leaf_id = CASE
                    WHEN EXISTS (SELECT 1 FROM messages WHERE chat_id = chats.id AND imported = 0) THEN chats.active_leaf_id
                    ELSE excluded.active_leaf_id
                END",
            params![chat.id, chat.title, source, source_file, chat.cwd, chat.leaf_id, chat.created_at, chat.updated_at],
        )?;
        for image in &chat.images {
            insert_image(&transaction, &chat.id, image)?;
        }
        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO messages(id, chat_id, parent_id, role, content, parts, attachments, thinking, model, input_tokens, output_tokens, imported, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12)
                 ON CONFLICT(id) DO UPDATE SET
                    parent_id = excluded.parent_id,
                    content = excluded.content,
                    parts = excluded.parts,
                    attachments = excluded.attachments,
                    thinking = excluded.thinking,
                    model = excluded.model,
                    input_tokens = excluded.input_tokens,
                    output_tokens = excluded.output_tokens
                 WHERE content <> excluded.content OR parts <> excluded.parts OR attachments <> excluded.attachments",
            )?;
            for message in &chat.messages {
                let attachments = serde_json::to_string(&message.attachments).unwrap_or_else(|_| "[]".to_string());
                statement.execute(params![
                    message.id,
                    chat.id,
                    message.parent_id,
                    message.role,
                    message.content,
                    message.parts.to_string(),
                    attachments,
                    message.thinking,
                    message.model,
                    message.input_tokens,
                    message.output_tokens,
                    message.created_at
                ])?;
            }
        }
        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO compactions(chat_id, up_to_message_id, summary)
                 SELECT ?1, ?2, ?3
                 WHERE NOT EXISTS (SELECT 1 FROM compactions WHERE chat_id = ?1 AND up_to_message_id = ?2)",
            )?;
            for compaction in &chat.compactions {
                statement.execute(params![chat.id, compaction.up_to_message_id, compaction.summary])?;
            }
        }
    }
    transaction.execute(
        "INSERT INTO import_files(path, source, size, modified, chat_id) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(path) DO UPDATE SET size = excluded.size, modified = excluded.modified, chat_id = excluded.chat_id",
        params![source_file, source, signature.0, signature.1, chat.id],
    )?;
    transaction.commit()?;
    Ok(should_store)
}

fn parse_file(source: &str, path: &Path) -> std::io::Result<ParsedChat> {
    if source == SOURCE_CODEX {
        codex::parse_file(path)
    } else {
        claude_code::parse_file(path)
    }
}

fn import_source(app: &AppHandle, connection: &mut Connection, request: &ImportRequest, background: bool) -> usize {
    let root = PathBuf::from(&request.path);
    let files = jsonl_files(&request.source, &root);
    let files_total = files.len();
    let mut chats_imported = 0usize;
    let mut failed_files = 0usize;
    for (index, file) in files.iter().enumerate() {
        let path_text = file.to_string_lossy().to_string();
        if let Some(signature) = file_signature(file) {
            if !file_is_current(connection, &path_text, signature) {
                match parse_file(&request.source, file) {
                    Ok(chat) => match store_parsed_chat(connection, &request.source, &path_text, signature, &chat) {
                        Ok(true) => chats_imported += 1,
                        Ok(false) => {}
                        Err(_) => failed_files += 1,
                    },
                    Err(_) => failed_files += 1,
                }
            }
        }
        let files_done = index + 1;
        let should_report = !background && (files_done % PROGRESS_EVERY_FILES == 0 || files_done == files_total);
        if should_report {
            let _ = app.emit(
                "import-progress",
                ImportProgress {
                    source: request.source.clone(),
                    files_done,
                    files_total,
                    chats_imported,
                    finished: false,
                    failed_files,
                    background,
                },
            );
        }
    }
    let _ = app.emit(
        "import-progress",
        ImportProgress {
            source: request.source.clone(),
            files_done: files_total,
            files_total,
            chats_imported,
            finished: true,
            failed_files,
            background,
        },
    );
    chats_imported
}

pub fn sync_one_chat(state: &AppState, chat_id: &str) -> Result<i64, String> {
    let (chat, messages_before) = state.with_db(|db| Ok((db.get_chat(chat_id)?, db.message_count(chat_id)?)))?;
    let file_text = chat.source_path.ok_or("У этого чата нет исходного файла")?;
    let file = Path::new(&file_text);
    let signature = file_signature(file).ok_or("Исходный файл чата не найден")?;
    let parsed = parse_file(&chat.source, file).map_err(|error| format!("Не удалось прочитать файл чата: {error}"))?;
    if parsed.id != chat_id {
        return Err("Форк не докачивается: докачай исходный чат".to_string());
    }
    let mut connection = Connection::open(&state.db_path).map_err(|error| format!("ошибка базы данных: {error}"))?;
    let _ = connection.busy_timeout(Duration::from_secs(15));
    store_parsed_chat(&mut connection, &chat.source, &file_text, signature, &parsed).map_err(|error| format!("ошибка базы данных: {error}"))?;
    let messages_after = state.with_db(|db| db.message_count(chat_id))?;
    Ok(messages_after - messages_before)
}

pub fn existing_sources(state: &AppState) -> Vec<ImportRequest> {
    [SOURCE_CLAUDE_CODE, SOURCE_CODEX]
        .iter()
        .filter_map(|source| {
            let path = state.with_db(|db| Ok(source_path(db, source))).ok()?;
            path.is_dir().then(|| ImportRequest { source: source.to_string(), path: path.to_string_lossy().to_string() })
        })
        .collect()
}

pub fn start_import(app: AppHandle, sources: Vec<ImportRequest>, background: bool) -> Result<(), String> {
    let state: Arc<AppState> = app.state::<Arc<AppState>>().inner().clone();
    if state.import_running.swap(true, Ordering::SeqCst) {
        return if background {
            Ok(())
        } else {
            Err("Импорт уже идёт".to_string())
        };
    }
    let running_flag = state.import_running.clone();
    let db_path = state.db_path.clone();
    std::thread::spawn(move || {
        if let Ok(database) = Database::open(&db_path) {
            drop(database);
        }
        if let Ok(mut connection) = Connection::open(&db_path) {
            let _ = connection.busy_timeout(Duration::from_secs(15));
            let _ = connection.pragma_update(None, "synchronous", "NORMAL");
            let mut chats_imported = 0;
            for request in &sources {
                chats_imported += import_source(&app, &mut connection, request, background);
            }
            let _ = connection.execute("DELETE FROM settings WHERE key = ?1", [IMPORT_REFRESH_PENDING_SETTING]);
            if chats_imported > 0 {
                let _ = app.emit("chats-changed", ());
            }
            if !background {
                let _ = app.emit("import-finished", chats_imported);
            }
        }
        running_flag.store(false, Ordering::SeqCst);
    });
    Ok(())
}

fn setting_is_true(state: &AppState, key: &str) -> bool {
    state
        .with_db(|db| Ok(db.get_setting(key)))
        .ok()
        .flatten()
        .is_some_and(|value| value == "true")
}

pub fn finish_pending_refresh(app: AppHandle) {
    let state: Arc<AppState> = app.state::<Arc<AppState>>().inner().clone();
    if !setting_is_true(&state, IMPORT_REFRESH_PENDING_SETTING) {
        return;
    }
    let requests = sources_with_imported_chats(&state);
    if !requests.is_empty() {
        let _ = start_import(app, requests, true);
    }
}

fn watched_sources(state: &AppState) -> Vec<ImportRequest> {
    if setting_is_true(state, "importWatch") {
        sources_with_imported_chats(state)
    } else {
        Vec::new()
    }
}

fn sources_with_imported_chats(state: &AppState) -> Vec<ImportRequest> {
    let mut requests = Vec::new();
    for source in [SOURCE_CLAUDE_CODE, SOURCE_CODEX] {
        let imported = state.with_db(|db| db.imported_chat_count(source)).unwrap_or(0);
        if imported == 0 {
            continue;
        }
        if let Ok(path) = state.with_db(|db| Ok(source_path(db, source))) {
            requests.push(ImportRequest {
                source: source.to_string(),
                path: path.to_string_lossy().to_string(),
            });
        }
    }
    requests
}

pub fn start_background_watch(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(WATCH_INTERVAL);
        let state: Arc<AppState> = app.state::<Arc<AppState>>().inner().clone();
        let requests = watched_sources(&state);
        if !requests.is_empty() {
            let _ = start_import(app.clone(), requests, true);
        }
    });
}

pub fn truncate_with_mark(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((index, _)) => format!("{}\n… обрезано при импорте", &text[..index]),
        None => text.to_string(),
    }
}

pub fn parse_timestamp(value: Option<&str>) -> i64 {
    value
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .map(|moment| moment.timestamp_millis())
        .unwrap_or(0)
}

pub fn title_from_text(text: &str) -> String {
    let first_line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or("").trim();
    let limited = truncate_chars(first_line, 70);
    if limited.len() < first_line.len() {
        format!("{limited}…")
    } else {
        limited.to_string()
    }
}

fn truncate_chars(text: &str, limit: usize) -> &str {
    match text.char_indices().nth(limit) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn open_memory() -> Connection {
        let directory = std::env::temp_dir().join(format!("tarn-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("t.db");
        drop(Database::open(&path).unwrap());
        Connection::open(path).unwrap()
    }

    fn sample_chat(content: &str) -> ParsedChat {
        ParsedChat {
            id: "chat-1".into(),
            title: "Заголовок".into(),
            cwd: None,
            created_at: 1,
            updated_at: 2,
            leaf_id: Some("m1".into()),
            images: Vec::new(),
            compactions: Vec::new(),
            messages: vec![ParsedMessage {
                id: "m1".into(),
                parent_id: None,
                role: "user".into(),
                content: content.into(),
                parts: json!([]),
                attachments: Vec::new(),
                thinking: None,
                model: None,
                input_tokens: None,
                output_tokens: None,
                created_at: 1,
            }],
        }
    }

    #[test]
    fn repeated_import_updates_without_duplicates_and_keeps_user_continuation() {
        let mut connection = open_memory();
        store_parsed_chat(&mut connection, SOURCE_CLAUDE_CODE, "f.jsonl", (1, 1), &sample_chat("старый")).unwrap();
        connection
            .execute(
                "INSERT INTO messages(id, chat_id, parent_id, role, content, created_at) VALUES ('local', 'chat-1', 'm1', 'user', 'моё', 5)",
                [],
            )
            .unwrap();
        connection.execute("UPDATE chats SET active_leaf_id = 'local' WHERE id = 'chat-1'", []).unwrap();
        store_parsed_chat(&mut connection, SOURCE_CLAUDE_CODE, "f.jsonl", (2, 2), &sample_chat("новый")).unwrap();
        let count: i64 = connection.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 2);
        let content: String = connection.query_row("SELECT content FROM messages WHERE id = 'm1'", [], |row| row.get(0)).unwrap();
        assert_eq!(content, "новый");
        let leaf: String = connection.query_row("SELECT active_leaf_id FROM chats", [], |row| row.get(0)).unwrap();
        assert_eq!(leaf, "local");
    }

    #[test]
    fn deleted_chat_is_not_restored_by_import() {
        let mut connection = open_memory();
        store_parsed_chat(&mut connection, SOURCE_CODEX, "f.jsonl", (1, 1), &sample_chat("текст")).unwrap();
        connection.execute("DELETE FROM messages", []).unwrap();
        connection.execute("UPDATE chats SET deleted = 1", []).unwrap();
        let stored = store_parsed_chat(&mut connection, SOURCE_CODEX, "f.jsonl", (2, 2), &sample_chat("текст")).unwrap();
        assert!(!stored);
        let count: i64 = connection.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn imported_compaction_is_stored_once() {
        let mut connection = open_memory();
        let mut chat = sample_chat("текст");
        chat.compactions.push(ParsedCompaction { up_to_message_id: "m1".into(), summary: "сводка".into() });
        store_parsed_chat(&mut connection, SOURCE_CLAUDE_CODE, "f.jsonl", (1, 1), &chat).unwrap();
        store_parsed_chat(&mut connection, SOURCE_CLAUDE_CODE, "f.jsonl", (2, 2), &chat).unwrap();
        let count: i64 = connection.query_row("SELECT COUNT(*) FROM compactions", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn truncation_marks_cut_text() {
        assert_eq!(truncate_with_mark("да", 10), "да");
        assert!(truncate_with_mark("привет", 3).starts_with("при\n…"));
    }
}
