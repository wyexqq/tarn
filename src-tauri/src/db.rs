use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::Value;

use crate::claude_code_export::{ExportTurn, ImageBlock};
use crate::compaction::Compaction;
use crate::file_history::{self, FileChange, RestoreReport, UndoEntry};
use crate::file_panel::ChatFileChange;
use crate::images::{encode_base64, StoredImage};
use crate::types::{
    Artifact, Attachment, ChatSummary, Message, ModelInfo, PriceEntry, ProviderUsage, RequestLogEntry, RewindOutcome, RewindPreview, SearchHit, ThreadPage,
    PlanStep, UsageSummary,
};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS chats (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'local',
    source_path TEXT,
    cwd TEXT,
    model_ref TEXT,
    active_leaf_id TEXT,
    pinned INTEGER NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0,
    title_locked INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS chats_updated ON chats(deleted, updated_at DESC);

CREATE TABLE IF NOT EXISTS messages (
    rowid INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    chat_id TEXT NOT NULL,
    parent_id TEXT,
    role TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    parts TEXT NOT NULL DEFAULT '[]',
    thinking TEXT,
    provider TEXT,
    model TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    cost REAL,
    status TEXT NOT NULL DEFAULT 'done',
    error TEXT,
    note TEXT,
    attachments TEXT NOT NULL DEFAULT '[]',
    imported INTEGER NOT NULL DEFAULT 0,
    compare_group TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_parent ON messages(chat_id, parent_id);
CREATE INDEX IF NOT EXISTS messages_created ON messages(created_at);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    content='messages',
    content_rowid='rowid',
    tokenize="unicode61 remove_diacritics 2"
);
CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
END;
CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
END;
CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE OF content ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
    INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
END;

CREATE TABLE IF NOT EXISTS import_files (
    path TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    size INTEGER NOT NULL,
    modified INTEGER NOT NULL,
    chat_id TEXT
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS custom_providers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    protocol TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS models (
    provider TEXT NOT NULL,
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    max_output INTEGER,
    max_input INTEGER,
    PRIMARY KEY (provider, id)
);

CREATE TABLE IF NOT EXISTS prices (
    model TEXT PRIMARY KEY,
    input_per_million REAL NOT NULL,
    output_per_million REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS request_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at INTEGER NOT NULL,
    host TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    bytes_sent INTEGER NOT NULL,
    bytes_received INTEGER NOT NULL,
    status TEXT NOT NULL,
    message_count INTEGER NOT NULL,
    files TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS images (
    id TEXT PRIMARY KEY,
    mime TEXT NOT NULL,
    data BLOB NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS chat_images (
    chat_id TEXT NOT NULL,
    image_id TEXT NOT NULL,
    PRIMARY KEY (chat_id, image_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS chat_images_image ON chat_images(image_id);

CREATE TABLE IF NOT EXISTS file_changes (
    message_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    path TEXT NOT NULL,
    existed_before INTEGER NOT NULL,
    content_before BLOB,
    hash_after TEXT,
    PRIMARY KEY (message_id, path)
);
CREATE INDEX IF NOT EXISTS file_changes_chat ON file_changes(chat_id);

CREATE TABLE IF NOT EXISTS file_undo (
    chat_id TEXT NOT NULL,
    path TEXT NOT NULL,
    content BLOB,
    hash_after_restore TEXT,
    PRIMARY KEY (chat_id, path)
);
CREATE TABLE IF NOT EXISTS compactions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_id TEXT NOT NULL,
    up_to_message_id TEXT NOT NULL,
    summary TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS compactions_of_chat ON compactions(chat_id);
CREATE TABLE IF NOT EXISTS chat_plans (
    chat_id TEXT PRIMARY KEY,
    steps TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY,
    chat_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    title TEXT NOT NULL,
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    size INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS artifacts_of_chat ON artifacts(chat_id);
"#;

const COLUMN_ADDITIONS: [(&str, &str, &str); 3] = [
    ("models", "effort_levels", "TEXT NOT NULL DEFAULT '[]'"),
    ("chats", "access_mode", "TEXT NOT NULL DEFAULT 'chat'"),
    ("messages", "origin_id", "TEXT"),
];

const CLAUDE_CODE_MESSAGE_PREFIX: &str = "cc:";
const TOOL_RESULT_LIMIT_IN_EXPORT: usize = 2_000;

pub struct ClaudeCodeExport {
    pub chat: ChatSummary,
    pub source_file: Option<String>,
    pub copied_up_to_uuid: Option<String>,
    pub turns: Vec<ExportTurn>,
}

fn tool_call_lines(parts: &Value) -> Vec<String> {
    let Some(parts) = parts.as_array() else {
        return Vec::new();
    };
    parts
        .iter()
        .filter(|part| part["type"] == "tool_call")
        .map(|part| {
            let name = part["name"].as_str().unwrap_or_default();
            let input = part["input"].as_str().unwrap_or_default();
            let result = crate::importer::truncate_with_mark(part["result"].as_str().unwrap_or_default(), TOOL_RESULT_LIMIT_IN_EXPORT);
            format!("[{name}] {input}\n{result}")
        })
        .collect()
}

const IMPORT_FORMAT_VERSION: i64 = 2;
const FORK_TITLE_PREFIX: &str = "Форк: ";
pub const IMPORT_REFRESH_PENDING_SETTING: &str = "importRefreshPending";

fn table_has_column(connection: &Connection, table: &str, column: &str) -> Result<bool, rusqlite::Error> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn add_missing_columns(connection: &Connection) -> Result<(), rusqlite::Error> {
    for (table, column, definition) in COLUMN_ADDITIONS {
        if !table_has_column(connection, table, column)? {
            connection.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))?;
        }
    }
    Ok(())
}

pub fn insert_image(connection: &Connection, chat_id: &str, image: &StoredImage) -> Result<(), rusqlite::Error> {
    connection.execute(
        "INSERT OR IGNORE INTO images(id, mime, data) VALUES (?1, ?2, ?3)",
        params![image.id, image.mime, image.bytes],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO chat_images(chat_id, image_id) VALUES (?1, ?2)",
        params![chat_id, image.id],
    )?;
    Ok(())
}

const MESSAGE_COLUMNS: &str = "m.id, m.chat_id, m.parent_id, m.role, m.content, m.parts, m.thinking, m.provider, m.model, \
    m.input_tokens, m.output_tokens, m.cost, m.status, m.error, m.note, m.attachments, m.imported, m.compare_group, m.created_at";

const CHAT_COLUMNS: &str = "id, title, source, cwd, model_ref, created_at, updated_at, pinned, access_mode, source_path";

pub fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn sql_error(error: rusqlite::Error) -> String {
    format!("ошибка базы данных: {error}")
}

fn parse_json_column(text: String) -> Value {
    serde_json::from_str(&text).unwrap_or_else(|_| Value::Array(Vec::new()))
}

fn message_from_row(row: &Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        parent_id: row.get(2)?,
        role: row.get(3)?,
        content: row.get(4)?,
        parts: parse_json_column(row.get(5)?),
        thinking: row.get(6)?,
        provider: row.get(7)?,
        model: row.get(8)?,
        input_tokens: row.get(9)?,
        output_tokens: row.get(10)?,
        cost: row.get(11)?,
        status: row.get(12)?,
        error: row.get(13)?,
        note: row.get(14)?,
        attachments: parse_json_column(row.get(15)?),
        imported: row.get::<_, i64>(16)? != 0,
        compare_group: row.get(17)?,
        created_at: row.get(18)?,
        sibling_ids: Vec::new(),
    })
}

fn chat_from_row(row: &Row) -> rusqlite::Result<ChatSummary> {
    Ok(ChatSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        source: row.get(2)?,
        cwd: row.get(3)?,
        model_ref: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        pinned: row.get::<_, i64>(7)? != 0,
        access_mode: row.get(8)?,
        source_path: row.get(9)?,
    })
}

pub fn fts_query(raw: &str) -> Option<String> {
    let tokens: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\"*"))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

pub struct NewMessage<'a> {
    pub id: &'a str,
    pub chat_id: &'a str,
    pub parent_id: Option<&'a str>,
    pub role: &'a str,
    pub content: &'a str,
    pub attachments: &'a Value,
    pub provider: Option<&'a str>,
    pub model: Option<&'a str>,
    pub status: &'a str,
    pub compare_group: Option<&'a str>,
}

pub struct FinishedMessage<'a> {
    pub id: &'a str,
    pub content: &'a str,
    pub parts: &'a Value,
    pub thinking: Option<&'a str>,
    pub provider: &'a str,
    pub model: &'a str,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cost: Option<f64>,
    pub status: &'a str,
    pub error: Option<&'a str>,
    pub note: Option<&'a str>,
}

pub struct RequestRecord<'a> {
    pub host: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub bytes_sent: i64,
    pub bytes_received: i64,
    pub status: &'a str,
    pub message_count: i64,
    pub files: &'a str,
}

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "cache_size", -8000)?;
        connection.busy_timeout(std::time::Duration::from_secs(15))?;
        connection.execute_batch(SCHEMA)?;
        add_missing_columns(&connection)?;
        let database = Self { connection };
        database.seed_known_prices()?;
        database.request_reimport_after_format_change()?;
        Ok(database)
    }

    fn request_reimport_after_format_change(&self) -> Result<(), rusqlite::Error> {
        let version: i64 = self.connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version >= IMPORT_FORMAT_VERSION {
            return Ok(());
        }
        let imported_files: i64 = self.connection.query_row("SELECT COUNT(*) FROM import_files", [], |row| row.get(0))?;
        if imported_files > 0 {
            self.connection.execute("DELETE FROM import_files", [])?;
            self.connection.execute(
                "INSERT OR REPLACE INTO settings(key, value) VALUES (?1, 'true')",
                [IMPORT_REFRESH_PENDING_SETTING],
            )?;
        }
        self.connection.pragma_update(None, "user_version", IMPORT_FORMAT_VERSION)?;
        Ok(())
    }

    pub fn move_images_to_store(&self, chat_id: &str, attachments: Vec<Attachment>) -> Result<Vec<Attachment>, String> {
        let mut stored = Vec::with_capacity(attachments.len());
        for attachment in attachments {
            let image = attachment
                .data_base64
                .as_deref()
                .filter(|_| attachment.kind == "image")
                .and_then(|encoded| StoredImage::from_base64(&attachment.mime, encoded));
            match image {
                Some(image) => {
                    insert_image(&self.connection, chat_id, &image).map_err(sql_error)?;
                    stored.push(image.attachment(&attachment.name));
                }
                None => stored.push(attachment),
            }
        }
        Ok(stored)
    }

    pub fn image_base64(&self, image_id: &str) -> Result<Option<(String, String)>, String> {
        self.connection
            .query_row("SELECT mime, data FROM images WHERE id = ?1", [image_id], |row| {
                Ok((row.get::<_, String>(0)?, encode_base64(&row.get::<_, Vec<u8>>(1)?)))
            })
            .optional()
            .map_err(sql_error)
    }

    fn seed_known_prices(&self) -> Result<(), rusqlite::Error> {
        for (model, input, output) in crate::providers::known_prices() {
            self.connection.execute(
                "INSERT OR IGNORE INTO prices(model, input_per_million, output_per_million) VALUES (?1, ?2, ?3)",
                params![model, input, output],
            )?;
        }
        Ok(())
    }

    pub fn mark_interrupted_messages(&self) -> Result<(), rusqlite::Error> {
        self.connection.execute(
            "UPDATE messages SET status = 'interrupted', note = 'Ответ прервался при закрытии приложения' WHERE status = 'streaming'",
            [],
        )?;
        Ok(())
    }

    pub fn list_chats(&self) -> Result<Vec<ChatSummary>, String> {
        let sql = format!("SELECT {CHAT_COLUMNS} FROM chats WHERE deleted = 0 ORDER BY pinned DESC, updated_at DESC");
        let mut statement = self.connection.prepare_cached(&sql).map_err(sql_error)?;
        let rows = statement.query_map([], chat_from_row).map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    pub fn get_chat(&self, chat_id: &str) -> Result<ChatSummary, String> {
        let sql = format!("SELECT {CHAT_COLUMNS} FROM chats WHERE id = ?1");
        self.connection
            .query_row(&sql, [chat_id], chat_from_row)
            .map_err(sql_error)
    }

    pub fn create_chat(&self, id: &str, title: &str, model_ref: Option<&str>) -> Result<(), String> {
        let now = now_millis();
        self.connection
            .execute(
                "INSERT INTO chats(id, title, model_ref, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
                params![id, title, model_ref, now],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn set_chat_access(&self, chat_id: &str, access_mode: &str, folder: Option<&str>) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE chats SET access_mode = ?2, cwd = COALESCE(?3, cwd) WHERE id = ?1",
                params![chat_id, access_mode, folder],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn active_leaf(&self, chat_id: &str) -> Result<Option<String>, String> {
        self.connection
            .query_row("SELECT active_leaf_id FROM chats WHERE id = ?1", [chat_id], |row| row.get(0))
            .map_err(sql_error)
    }

    pub fn set_active_leaf(&self, chat_id: &str, leaf: &str, model_ref: Option<&str>) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE chats SET active_leaf_id = ?2, model_ref = COALESCE(?3, model_ref), updated_at = ?4 WHERE id = ?1",
                params![chat_id, leaf, model_ref, now_millis()],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn rename_chat(&self, chat_id: &str, title: &str) -> Result<(), String> {
        self.connection
            .execute("UPDATE chats SET title = ?2, title_locked = 1 WHERE id = ?1", params![chat_id, title])
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn set_pinned(&self, chat_id: &str, pinned: bool) -> Result<(), String> {
        self.connection
            .execute("UPDATE chats SET pinned = ?2 WHERE id = ?1", params![chat_id, pinned as i64])
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn delete_chat(&self, chat_id: &str) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM messages WHERE chat_id = ?1", [chat_id])
            .map_err(sql_error)?;
        self.connection
            .execute("UPDATE chats SET deleted = 1, active_leaf_id = NULL WHERE id = ?1", [chat_id])
            .map_err(sql_error)?;
        self.connection
            .execute("DELETE FROM chat_images WHERE chat_id = ?1", [chat_id])
            .map_err(sql_error)?;
        self.connection
            .execute("DELETE FROM images WHERE id NOT IN (SELECT image_id FROM chat_images)", [])
            .map_err(sql_error)?;
        self.connection
            .execute("DELETE FROM file_changes WHERE chat_id = ?1", [chat_id])
            .map_err(sql_error)?;
        self.connection
            .execute("DELETE FROM file_undo WHERE chat_id = ?1", [chat_id])
            .map_err(sql_error)?;
        self.connection
            .execute("DELETE FROM compactions WHERE chat_id = ?1", [chat_id])
            .map_err(sql_error)?;
        self.connection
            .execute("DELETE FROM chat_plans WHERE chat_id = ?1", [chat_id])
            .map_err(sql_error)?;
        self.connection
            .execute("DELETE FROM artifacts WHERE chat_id = ?1", [chat_id])
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn set_plan(&self, chat_id: &str, steps: &[PlanStep]) -> Result<(), String> {
        let steps_json = serde_json::to_string(steps).map_err(|error| error.to_string())?;
        self.connection
            .execute(
                "INSERT INTO chat_plans(chat_id, steps, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(chat_id) DO UPDATE SET steps = excluded.steps, updated_at = excluded.updated_at",
                params![chat_id, steps_json, chrono::Utc::now().timestamp_millis()],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn plan_of(&self, chat_id: &str) -> Result<Vec<PlanStep>, String> {
        let stored: Option<String> = self
            .connection
            .query_row("SELECT steps FROM chat_plans WHERE chat_id = ?1", [chat_id], |row| row.get(0))
            .optional()
            .map_err(sql_error)?;
        Ok(stored.and_then(|steps| serde_json::from_str(&steps).ok()).unwrap_or_default())
    }

    pub fn add_artifact(&self, artifact: &Artifact) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO artifacts(id, chat_id, message_id, title, kind, path, size, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![artifact.id, artifact.chat_id, artifact.message_id, artifact.title, artifact.kind, artifact.path, artifact.size, artifact.created_at],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    fn artifact_from_row(row: &Row) -> rusqlite::Result<Artifact> {
        Ok(Artifact {
            id: row.get(0)?,
            chat_id: row.get(1)?,
            message_id: row.get(2)?,
            title: row.get(3)?,
            kind: row.get(4)?,
            path: row.get(5)?,
            size: row.get(6)?,
            created_at: row.get(7)?,
        })
    }

    pub fn artifacts_of(&self, chat_id: &str) -> Result<Vec<Artifact>, String> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT id, chat_id, message_id, title, kind, path, size, created_at FROM artifacts WHERE chat_id = ?1 ORDER BY created_at")
            .map_err(sql_error)?;
        let rows = statement.query_map([chat_id], Self::artifact_from_row).map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    pub fn get_artifact(&self, artifact_id: &str) -> Result<Artifact, String> {
        self.connection
            .query_row(
                "SELECT id, chat_id, message_id, title, kind, path, size, created_at FROM artifacts WHERE id = ?1",
                [artifact_id],
                Self::artifact_from_row,
            )
            .map_err(sql_error)
    }

    pub fn chat_file_changes(&self, chat_id: &str) -> Result<Vec<ChatFileChange>, String> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT path, existed_before, content_before FROM file_changes WHERE chat_id = ?1 ORDER BY rowid")
            .map_err(sql_error)?;
        let rows = statement
            .query_map([chat_id], |row| Ok(ChatFileChange { path: row.get(0)?, existed_before: row.get(1)?, content_before: row.get(2)? }))
            .map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    pub fn add_compaction(&self, chat_id: &str, compaction: &Compaction) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO compactions(chat_id, up_to_message_id, summary) VALUES (?1, ?2, ?3)",
                params![chat_id, compaction.up_to_message_id, compaction.summary],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn latest_compaction_in(&self, path: &[Message]) -> Result<Option<Compaction>, String> {
        let Some(chat_id) = path.last().map(|message| message.chat_id.as_str()) else {
            return Ok(None);
        };
        let position_of: HashMap<&str, usize> = path.iter().enumerate().map(|(position, message)| (message.id.as_str(), position)).collect();
        let mut statement = self
            .connection
            .prepare("SELECT up_to_message_id, summary FROM compactions WHERE chat_id = ?1 ORDER BY id")
            .map_err(sql_error)?;
        let rows = statement
            .query_map([chat_id], |row| Ok(Compaction { up_to_message_id: row.get(0)?, summary: row.get(1)? }))
            .map_err(sql_error)?;
        let mut latest: Option<(usize, Compaction)> = None;
        for row in rows {
            let compaction = row.map_err(sql_error)?;
            let Some(&position) = position_of.get(compaction.up_to_message_id.as_str()) else {
                continue;
            };
            let is_later = latest.as_ref().map_or(true, |(latest_position, _)| position >= *latest_position);
            if is_later {
                latest = Some((position, compaction));
            }
        }
        Ok(latest.map(|(_, compaction)| compaction))
    }

    pub fn insert_message(&self, message: &NewMessage) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO messages(id, chat_id, parent_id, role, content, attachments, provider, model, status, compare_group, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    message.id,
                    message.chat_id,
                    message.parent_id,
                    message.role,
                    message.content,
                    message.attachments.to_string(),
                    message.provider,
                    message.model,
                    message.status,
                    message.compare_group,
                    now_millis()
                ],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn save_partial(&self, message_id: &str, content: &str, parts: &Value) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE messages SET content = ?2, parts = ?3 WHERE id = ?1",
                params![message_id, content, parts.to_string()],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn finish_message(&self, finished: &FinishedMessage) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE messages SET content = ?2, thinking = ?3, provider = ?4, model = ?5, input_tokens = ?6,
                 output_tokens = ?7, cost = ?8, status = ?9, error = ?10, note = ?11, parts = ?12 WHERE id = ?1",
                params![
                    finished.id,
                    finished.content,
                    finished.thinking,
                    finished.provider,
                    finished.model,
                    finished.input_tokens,
                    finished.output_tokens,
                    finished.cost,
                    finished.status,
                    finished.error,
                    finished.note,
                    finished.parts.to_string()
                ],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    fn sibling_ids(&self, message: &Message) -> Result<Vec<String>, String> {
        if message.parent_id.is_none() && message.imported {
            return Ok(Vec::new());
        }
        let mut statement = self
            .connection
            .prepare_cached("SELECT id FROM messages WHERE chat_id = ?1 AND parent_id IS ?2 AND role = ?3 ORDER BY created_at, rowid")
            .map_err(sql_error)?;
        let rows = statement
            .query_map(params![message.chat_id, message.parent_id, message.role], |row| row.get::<_, String>(0))
            .map_err(sql_error)?;
        let ids = rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
        Ok(if ids.len() > 1 { ids } else { Vec::new() })
    }

    pub fn get_message(&self, message_id: &str) -> Result<Message, String> {
        let sql = format!("SELECT {MESSAGE_COLUMNS} FROM messages m WHERE m.id = ?1");
        let mut message = self
            .connection
            .query_row(&sql, [message_id], message_from_row)
            .map_err(sql_error)?;
        message.sibling_ids = self.sibling_ids(&message)?;
        Ok(message)
    }

    pub fn path_messages(&self, leaf_id: &str, limit: i64) -> Result<(Vec<Message>, bool), String> {
        let sql = format!(
            "WITH RECURSIVE path(id, parent_id, depth) AS (
                SELECT id, parent_id, 0 FROM messages WHERE id = ?1
                UNION ALL
                SELECT p.id, p.parent_id, path.depth + 1 FROM messages p JOIN path ON p.id = path.parent_id
             )
             SELECT {MESSAGE_COLUMNS} FROM path JOIN messages m ON m.id = path.id ORDER BY path.depth ASC LIMIT ?2"
        );
        let mut statement = self.connection.prepare_cached(&sql).map_err(sql_error)?;
        let rows = statement
            .query_map(params![leaf_id, limit + 1], message_from_row)
            .map_err(sql_error)?;
        let mut messages = rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
        let has_more = messages.len() as i64 > limit;
        messages.truncate(limit as usize);
        messages.reverse();
        Ok((messages, has_more))
    }

    pub fn thread_page(&self, chat_id: &str, before_id: Option<&str>, limit: i64, ensure_id: Option<&str>) -> Result<ThreadPage, String> {
        let start = match before_id {
            Some(message_id) => self.get_message(message_id)?.parent_id,
            None => self.resolve_leaf(chat_id, ensure_id)?,
        };
        let Some(start_id) = start else {
            return Ok(ThreadPage { messages: Vec::new(), has_more: false });
        };
        let (mut messages, mut has_more) = self.path_messages(&start_id, limit)?;
        if let Some(target) = ensure_id {
            let mut expanded_limit = limit;
            while has_more && !messages.iter().any(|message| message.id == target) {
                expanded_limit *= 2;
                let expanded = self.path_messages(&start_id, expanded_limit)?;
                messages = expanded.0;
                has_more = expanded.1;
            }
        }
        for message in &mut messages {
            message.sibling_ids = self.sibling_ids(message)?;
        }
        Ok(ThreadPage { messages, has_more })
    }

    fn resolve_leaf(&self, chat_id: &str, ensure_id: Option<&str>) -> Result<Option<String>, String> {
        let active = self.active_leaf(chat_id)?;
        let Some(target) = ensure_id else {
            return Ok(active);
        };
        if let Some(leaf) = &active {
            let (path, _) = self.path_messages(leaf, i64::MAX / 4)?;
            if path.iter().any(|message| message.id == target) {
                return Ok(active);
            }
        }
        let leaf = self.deepest_descendant(target)?;
        self.set_active_leaf(chat_id, &leaf, None)?;
        Ok(Some(leaf))
    }

    pub fn deepest_descendant(&self, message_id: &str) -> Result<String, String> {
        let mut current = message_id.to_string();
        loop {
            let child: Option<String> = self
                .connection
                .query_row(
                    "SELECT id FROM messages WHERE parent_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT 1",
                    [&current],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sql_error)?;
            match child {
                Some(next) => current = next,
                None => return Ok(current),
            }
        }
    }

    fn refuse_while_answering(&self, chat_id: &str) -> Result<(), String> {
        let answering: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM messages WHERE chat_id = ?1 AND status = 'streaming'", [chat_id], |row| row.get(0))
            .map_err(sql_error)?;
        if answering > 0 {
            return Err("Дождись конца ответа".to_string());
        }
        Ok(())
    }

    fn last_kept_message(message: &Message) -> Option<String> {
        if message.role == "user" {
            message.parent_id.clone()
        } else {
            Some(message.id.clone())
        }
    }

    pub fn fork_chat(&self, chat_id: &str, message_id: Option<&str>, new_chat_id: &str) -> Result<ChatSummary, String> {
        self.refuse_while_answering(chat_id)?;
        let last_kept = match message_id {
            Some(id) => Self::last_kept_message(&self.get_message(id)?),
            None => self.active_leaf(chat_id)?,
        };
        let copied = match &last_kept {
            Some(id) => self.path_messages(id, i64::MAX / 4)?.0,
            None => Vec::new(),
        };
        let source = self.get_chat(chat_id)?;
        let title = if source.title.starts_with(FORK_TITLE_PREFIX) {
            source.title.clone()
        } else {
            format!("{FORK_TITLE_PREFIX}{}", source.title)
        };
        let transaction = self.connection.unchecked_transaction().map_err(sql_error)?;
        let now = now_millis();
        transaction
            .execute(
                "INSERT INTO chats(id, title, source, source_path, cwd, model_ref, access_mode, title_locked, created_at, updated_at)
                 SELECT ?1, ?2, source, source_path, cwd, model_ref, access_mode, 1, ?3, ?3 FROM chats WHERE id = ?4",
                params![new_chat_id, title, now, chat_id],
            )
            .map_err(sql_error)?;
        let mut new_parent: Option<String> = None;
        for message in &copied {
            let new_id = uuid::Uuid::new_v4().to_string();
            transaction
                .execute(
                    "INSERT INTO messages(id, chat_id, parent_id, role, content, parts, thinking, provider, model, status, error, note, attachments, imported, created_at, origin_id)
                     SELECT ?1, ?2, ?3, role, content, parts, thinking, provider, model, status, error, note, attachments, imported, created_at,
                            COALESCE(origin_id, CASE WHEN id LIKE 'cc:%' THEN id END)
                     FROM messages WHERE id = ?4",
                    params![new_id, new_chat_id, new_parent, message.id],
                )
                .map_err(sql_error)?;
            new_parent = Some(new_id);
        }
        transaction
            .execute("UPDATE chats SET active_leaf_id = ?2 WHERE id = ?1", params![new_chat_id, new_parent])
            .map_err(sql_error)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO chat_images(chat_id, image_id) SELECT ?1, image_id FROM chat_images WHERE chat_id = ?2",
                params![new_chat_id, chat_id],
            )
            .map_err(sql_error)?;
        transaction.commit().map_err(sql_error)?;
        self.get_chat(new_chat_id)
    }

    pub fn message_count(&self, chat_id: &str) -> Result<i64, String> {
        self.connection
            .query_row("SELECT COUNT(*) FROM messages WHERE chat_id = ?1", [chat_id], |row| row.get(0))
            .map_err(sql_error)
    }

    fn claude_code_uuid_of(&self, message_id: &str) -> Result<Option<String>, String> {
        let origin_id: Option<String> = self
            .connection
            .query_row("SELECT origin_id FROM messages WHERE id = ?1", [message_id], |row| row.get(0))
            .map_err(sql_error)?;
        let full_id = origin_id.unwrap_or_else(|| message_id.to_string());
        Ok(full_id.strip_prefix(CLAUDE_CODE_MESSAGE_PREFIX).map(str::to_string))
    }

    fn export_turn_of(&self, message: &Message) -> Result<Option<ExportTurn>, String> {
        let mut sections = Vec::new();
        if !message.content.trim().is_empty() {
            sections.push(message.content.clone());
        }
        sections.extend(tool_call_lines(&message.parts));
        let mut images = Vec::new();
        let attachments: Vec<Attachment> = serde_json::from_value(message.attachments.clone()).unwrap_or_default();
        for attachment in &attachments {
            if let Some(text) = &attachment.text {
                sections.push(format!("[{}]\n{text}", attachment.name));
            }
            if let Some(image_id) = &attachment.image_id {
                if let Some((mime, data_base64)) = self.image_base64(image_id)? {
                    images.push(ImageBlock { mime, data_base64 });
                }
            }
        }
        if sections.is_empty() && images.is_empty() {
            return Ok(None);
        }
        let own_id_is_uuid = uuid::Uuid::parse_str(&message.id).is_ok();
        Ok(Some(ExportTurn {
            uuid: if own_id_is_uuid { message.id.clone() } else { uuid::Uuid::new_v4().to_string() },
            role: message.role.clone(),
            text: sections.join("\n\n"),
            images,
            model: message.model.clone(),
            created_at: message.created_at,
        }))
    }

    pub fn claude_code_export(&self, chat_id: &str) -> Result<ClaudeCodeExport, String> {
        self.refuse_while_answering(chat_id)?;
        let chat = self.get_chat(chat_id)?;
        let path = match self.active_leaf(chat_id)? {
            Some(leaf) => self.path_messages(&leaf, i64::MAX / 4)?.0,
            None => Vec::new(),
        };
        if path.is_empty() {
            return Err("В чате пока нет сообщений".to_string());
        }
        let source_file = chat
            .source_path
            .clone()
            .filter(|file| chat.source == crate::importer::SOURCE_CLAUDE_CODE && Path::new(file).is_file());
        let mut copied_up_to_uuid = None;
        let mut copied_count = 0;
        if source_file.is_some() {
            for message in &path {
                let Some(uuid) = self.claude_code_uuid_of(&message.id)? else {
                    break;
                };
                copied_up_to_uuid = Some(uuid);
                copied_count += 1;
            }
        }
        let mut turns = Vec::new();
        for message in path.iter().skip(copied_count) {
            turns.extend(self.export_turn_of(message)?);
        }
        let source_file = source_file.filter(|_| copied_count > 0);
        Ok(ClaudeCodeExport { chat, source_file, copied_up_to_uuid, turns })
    }

    pub fn mark_messages_as_imported(&self, chat_id: &str, message_ids: &[String]) -> Result<(), String> {
        let transaction = self.connection.unchecked_transaction().map_err(sql_error)?;
        for id in message_ids {
            let new_id = format!("{CLAUDE_CODE_MESSAGE_PREFIX}{id}");
            transaction
                .execute("UPDATE messages SET id = ?2, imported = 1 WHERE id = ?1 AND chat_id = ?3", params![id, new_id, chat_id])
                .map_err(sql_error)?;
            transaction.execute("UPDATE messages SET parent_id = ?2 WHERE parent_id = ?1", params![id, new_id]).map_err(sql_error)?;
            transaction.execute("UPDATE chats SET active_leaf_id = ?2 WHERE active_leaf_id = ?1", params![id, new_id]).map_err(sql_error)?;
            transaction.execute("UPDATE file_changes SET message_id = ?2 WHERE message_id = ?1", params![id, new_id]).map_err(sql_error)?;
        }
        transaction.commit().map_err(sql_error)
    }

    pub fn add_file_change(&self, message_id: &str, chat_id: &str, change: &FileChange) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO file_changes(message_id, chat_id, path, existed_before, content_before, hash_after)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(message_id, path) DO UPDATE SET hash_after = excluded.hash_after",
                params![message_id, chat_id, change.path, change.existed_before, change.content_before, change.hash_after],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    fn messages_after_rewind_point(&self, message: &Message) -> Result<Vec<Message>, String> {
        let Some(leaf) = self.active_leaf(&message.chat_id)? else {
            return Ok(Vec::new());
        };
        let (path, _) = self.path_messages(&leaf, i64::MAX / 4)?;
        let first_later_index = match Self::last_kept_message(message) {
            None => 0,
            Some(kept_id) => match path.iter().position(|item| item.id == kept_id) {
                Some(kept_index) => kept_index + 1,
                None => return Ok(Vec::new()),
            },
        };
        Ok(path.into_iter().skip(first_later_index).collect())
    }

    fn changed_paths_of_message(&self, message_id: &str) -> Result<Vec<String>, String> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT path FROM file_changes WHERE message_id = ?1")
            .map_err(sql_error)?;
        let rows = statement.query_map([message_id], |row| row.get::<_, String>(0)).map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    pub fn rewind_preview(&self, message_id: &str) -> Result<RewindPreview, String> {
        let message = self.get_message(message_id)?;
        let later_messages = self.messages_after_rewind_point(&message)?;
        let mut changed_paths = HashSet::new();
        for later in &later_messages {
            changed_paths.extend(self.changed_paths_of_message(&later.id)?);
        }
        let commands_left_effects = later_messages
            .iter()
            .filter_map(|later| later.parts.as_array())
            .flatten()
            .any(|part| part["changesBeyondFiles"].as_bool().unwrap_or(false));
        Ok(RewindPreview { changed_files: changed_paths.len(), commands_left_effects })
    }

    fn file_changes_of_message(&self, message_id: &str) -> Result<Vec<FileChange>, String> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT path, existed_before, content_before, hash_after FROM file_changes WHERE message_id = ?1 ORDER BY rowid")
            .map_err(sql_error)?;
        let rows = statement
            .query_map([message_id], |row| {
                Ok(FileChange {
                    path: row.get(0)?,
                    existed_before: row.get(1)?,
                    content_before: row.get(2)?,
                    hash_after: row.get(3)?,
                })
            })
            .map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    fn file_changes_after(&self, message: &Message) -> Result<Vec<FileChange>, String> {
        let mut merged: Vec<FileChange> = Vec::new();
        for later in self.messages_after_rewind_point(message)? {
            for change in self.file_changes_of_message(&later.id)? {
                match merged.iter_mut().find(|known| known.path == change.path) {
                    Some(known) => known.hash_after = change.hash_after,
                    None => merged.push(change),
                }
            }
        }
        Ok(merged)
    }

    fn replace_file_undo(&self, chat_id: &str, entries: &[UndoEntry]) -> Result<(), String> {
        let transaction = self.connection.unchecked_transaction().map_err(sql_error)?;
        transaction.execute("DELETE FROM file_undo WHERE chat_id = ?1", [chat_id]).map_err(sql_error)?;
        for entry in entries {
            transaction
                .execute(
                    "INSERT INTO file_undo(chat_id, path, content, hash_after_restore) VALUES (?1, ?2, ?3, ?4)",
                    params![chat_id, entry.path, entry.content, entry.hash_after_restore],
                )
                .map_err(sql_error)?;
        }
        transaction.commit().map_err(sql_error)
    }

    pub fn rewind_chat(&self, message_id: &str, restore_files: bool) -> Result<RewindOutcome, String> {
        let message = self.get_message(message_id)?;
        self.refuse_while_answering(&message.chat_id)?;
        let previous_leaf = self.active_leaf(&message.chat_id)?;
        let mut report = RestoreReport::default();
        if restore_files {
            report = file_history::restore(&self.file_changes_after(&message)?);
            self.replace_file_undo(&message.chat_id, &report.undo)?;
        }
        self.connection
            .execute(
                "UPDATE chats SET active_leaf_id = ?2, updated_at = ?3 WHERE id = ?1",
                params![message.chat_id, Self::last_kept_message(&message), now_millis()],
            )
            .map_err(sql_error)?;
        Ok(RewindOutcome { previous_leaf, restored_files: report.undo.len(), not_restored: report.not_restored })
    }

    pub fn undo_file_restore(&self, chat_id: &str) -> Result<Vec<String>, String> {
        self.refuse_while_answering(chat_id)?;
        let mut statement = self
            .connection
            .prepare_cached("SELECT path, content, hash_after_restore FROM file_undo WHERE chat_id = ?1")
            .map_err(sql_error)?;
        let rows = statement
            .query_map([chat_id], |row| {
                Ok(UndoEntry { path: row.get(0)?, content: row.get(1)?, hash_after_restore: row.get(2)? })
            })
            .map_err(sql_error)?;
        let entries = rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
        let not_restored = file_history::undo(&entries);
        self.connection.execute("DELETE FROM file_undo WHERE chat_id = ?1", [chat_id]).map_err(sql_error)?;
        Ok(not_restored)
    }

    pub fn search(&self, raw_query: &str, limit: i64) -> Result<Vec<SearchHit>, String> {
        let mut hits = Vec::new();
        let pattern = format!("%{}%", raw_query.trim());
        let mut title_statement = self
            .connection
            .prepare_cached(
                "SELECT id, title, source, updated_at FROM chats WHERE deleted = 0 AND title LIKE ?1
                 ORDER BY pinned DESC, updated_at DESC LIMIT ?2",
            )
            .map_err(sql_error)?;
        let title_rows = title_statement
            .query_map(params![pattern, limit], |row| {
                Ok(SearchHit {
                    chat_id: row.get(0)?,
                    message_id: None,
                    title: row.get(1)?,
                    source: row.get(2)?,
                    snippet: None,
                    updated_at: row.get(3)?,
                })
            })
            .map_err(sql_error)?;
        for hit in title_rows {
            hits.push(hit.map_err(sql_error)?);
        }
        let Some(query) = fts_query(raw_query) else {
            return Ok(hits);
        };
        let mut content_statement = self
            .connection
            .prepare_cached(
                "SELECT m.chat_id, m.id, c.title, c.source, snippet(messages_fts, 0, char(1), char(2), '…', 14), c.updated_at
                 FROM messages_fts
                 JOIN messages m ON m.rowid = messages_fts.rowid
                 JOIN chats c ON c.id = m.chat_id
                 WHERE messages_fts MATCH ?1 AND c.deleted = 0
                 ORDER BY rank LIMIT ?2",
            )
            .map_err(sql_error)?;
        let content_rows = content_statement
            .query_map(params![query, limit], |row| {
                Ok(SearchHit {
                    chat_id: row.get(0)?,
                    message_id: Some(row.get(1)?),
                    title: row.get(2)?,
                    source: row.get(3)?,
                    snippet: Some(row.get(4)?),
                    updated_at: row.get(5)?,
                })
            })
            .map_err(sql_error)?;
        for hit in content_rows {
            hits.push(hit.map_err(sql_error)?);
        }
        Ok(hits)
    }

    pub fn get_setting(&self, key: &str) -> Option<String> {
        self.connection
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn all_settings(&self) -> Result<Vec<(String, String)>, String> {
        let mut statement = self.connection.prepare_cached("SELECT key, value FROM settings").map_err(sql_error)?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    pub fn custom_providers(&self) -> Result<Vec<(String, String, String, String)>, String> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT id, name, base_url, protocol FROM custom_providers ORDER BY name")
            .map_err(sql_error)?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    pub fn add_custom_provider(&self, id: &str, name: &str, base_url: &str, protocol: &str) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO custom_providers(id, name, base_url, protocol) VALUES (?1, ?2, ?3, ?4)",
                params![id, name, base_url, protocol],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn delete_custom_provider(&self, id: &str) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM custom_providers WHERE id = ?1", [id])
            .map_err(sql_error)?;
        self.replace_models(id, &[])
    }

    pub fn replace_models(&self, provider: &str, models: &[ModelInfo]) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM models WHERE provider = ?1", [provider])
            .map_err(sql_error)?;
        for model in models {
            self.connection
                .execute(
                    "INSERT OR REPLACE INTO models(provider, id, name, max_output, max_input, effort_levels) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![model.provider, model.id, model.name, model.max_output, model.max_input, serde_json::json!(model.effort_levels).to_string()],
                )
                .map_err(sql_error)?;
        }
        Ok(())
    }

    pub fn list_models(&self) -> Result<Vec<ModelInfo>, String> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT provider, id, name, max_output, max_input, effort_levels FROM models ORDER BY provider, rowid")
            .map_err(sql_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(ModelInfo {
                    provider: row.get(0)?,
                    id: row.get(1)?,
                    name: row.get(2)?,
                    max_output: row.get(3)?,
                    max_input: row.get(4)?,
                    effort_levels: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
                })
            })
            .map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    pub fn model_count(&self, provider: &str) -> Result<i64, String> {
        self.connection
            .query_row("SELECT COUNT(*) FROM models WHERE provider = ?1", [provider], |row| row.get(0))
            .map_err(sql_error)
    }

    pub fn model_effort_levels(&self, provider: &str, model: &str) -> Vec<String> {
        self.connection
            .query_row(
                "SELECT effort_levels FROM models WHERE provider = ?1 AND id = ?2",
                params![provider, model],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|levels| serde_json::from_str(&levels).ok())
            .unwrap_or_default()
    }

    pub fn model_max_input(&self, provider: &str, model: &str) -> Option<i64> {
        self.connection
            .query_row(
                "SELECT max_input FROM models WHERE provider = ?1 AND id = ?2",
                params![provider, model],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten()
    }

    pub fn model_max_output(&self, provider: &str, model: &str) -> Option<i64> {
        self.connection
            .query_row(
                "SELECT max_output FROM models WHERE provider = ?1 AND id = ?2",
                params![provider, model],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten()
    }

    pub fn set_price(&self, model: &str, input: f64, output: f64) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO prices(model, input_per_million, output_per_million) VALUES (?1, ?2, ?3)
                 ON CONFLICT(model) DO UPDATE SET input_per_million = excluded.input_per_million, output_per_million = excluded.output_per_million",
                params![model, input, output],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn delete_price(&self, model: &str) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM prices WHERE model = ?1", [model])
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn list_prices(&self) -> Result<Vec<PriceEntry>, String> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT model, input_per_million, output_per_million FROM prices ORDER BY model")
            .map_err(sql_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(PriceEntry {
                    model: row.get(0)?,
                    input_per_million: row.get(1)?,
                    output_per_million: row.get(2)?,
                })
            })
            .map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    pub fn known_price(&self, model: &str) -> Option<(f64, f64)> {
        self.price_of(model).or_else(|| self.price_of(crate::providers::model_without_date_suffix(model)))
    }

    pub fn cheapest_model_of(&self, provider: &str) -> Option<String> {
        let models = self.list_models().ok()?;
        let priced = models
            .into_iter()
            .filter(|model| model.provider == provider)
            .filter_map(|model| self.known_price(&model.id).map(|(input_price, _)| (input_price, model.id)));
        priced.min_by(|left, right| left.0.total_cmp(&right.0)).map(|(_, id)| id)
    }

    pub fn model_name(&self, provider: &str, model: &str) -> Option<String> {
        self.connection
            .query_row("SELECT name FROM models WHERE provider = ?1 AND id = ?2", params![provider, model], |row| row.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn cost_for(&self, model: &str, input_tokens: Option<i64>, output_tokens: Option<i64>) -> Option<f64> {
        let (input_price, output_price) = self.known_price(model)?;
        let multiplier = crate::providers::long_prompt_price_multiplier(model, input_tokens.unwrap_or(0));
        let input_cost = input_tokens.unwrap_or(0) as f64 * input_price * multiplier / 1_000_000.0;
        let output_cost = output_tokens.unwrap_or(0) as f64 * output_price * multiplier / 1_000_000.0;
        Some(input_cost + output_cost)
    }

    fn price_of(&self, model: &str) -> Option<(f64, f64)> {
        self.connection
            .query_row(
                "SELECT input_per_million, output_per_million FROM prices WHERE model = ?1",
                [model],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn log_request(&self, record: &RequestRecord) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO request_log(created_at, host, provider, model, bytes_sent, bytes_received, status, message_count, files)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    now_millis(),
                    record.host,
                    record.provider,
                    record.model,
                    record.bytes_sent,
                    record.bytes_received,
                    record.status,
                    record.message_count,
                    record.files
                ],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn request_log(&self, limit: i64) -> Result<Vec<RequestLogEntry>, String> {
        let mut statement = self
            .connection
            .prepare_cached(
                "SELECT id, created_at, host, provider, model, bytes_sent, bytes_received, status, message_count, files
                 FROM request_log ORDER BY id DESC LIMIT ?1",
            )
            .map_err(sql_error)?;
        let rows = statement
            .query_map([limit], |row| {
                Ok(RequestLogEntry {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    host: row.get(2)?,
                    provider: row.get(3)?,
                    model: row.get(4)?,
                    bytes_sent: row.get(5)?,
                    bytes_received: row.get(6)?,
                    status: row.get(7)?,
                    message_count: row.get(8)?,
                    files: row.get(9)?,
                })
            })
            .map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    pub fn clear_request_log(&self) -> Result<(), String> {
        self.connection.execute("DELETE FROM request_log", []).map_err(sql_error)?;
        Ok(())
    }

    pub fn usage_summary(&self) -> Result<UsageSummary, String> {
        use chrono::{Datelike, Local, TimeZone};
        let now = Local::now();
        let day_start = Local
            .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
            .single()
            .map(|moment| moment.timestamp_millis())
            .unwrap_or(0);
        let month_start = Local
            .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
            .single()
            .map(|moment| moment.timestamp_millis())
            .unwrap_or(0);
        let today_cost: f64 = self
            .connection
            .query_row(
                "SELECT COALESCE(SUM(cost), 0) FROM messages WHERE imported = 0 AND created_at >= ?1",
                [day_start],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        let (month_cost, month_input_tokens, month_output_tokens): (f64, i64, i64) = self
            .connection
            .query_row(
                "SELECT COALESCE(SUM(cost), 0), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0)
                 FROM messages WHERE imported = 0 AND created_at >= ?1",
                [month_start],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(sql_error)?;
        let unpriced_messages: i64 = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE imported = 0 AND role = 'assistant' AND cost IS NULL
                 AND output_tokens IS NOT NULL AND created_at >= ?1",
                [month_start],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        let mut statement = self
            .connection
            .prepare_cached(
                "SELECT provider, COALESCE(SUM(cost), 0), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0)
                 FROM messages WHERE imported = 0 AND provider IS NOT NULL AND created_at >= ?1
                 GROUP BY provider ORDER BY 2 DESC",
            )
            .map_err(sql_error)?;
        let rows = statement
            .query_map([month_start], |row| {
                Ok(ProviderUsage {
                    provider: row.get(0)?,
                    cost: row.get(1)?,
                    input_tokens: row.get(2)?,
                    output_tokens: row.get(3)?,
                })
            })
            .map_err(sql_error)?;
        let by_provider = rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
        let budget = self.get_setting("monthlyBudget").and_then(|value| value.parse::<f64>().ok());
        Ok(UsageSummary {
            today_cost,
            month_cost,
            month_input_tokens,
            month_output_tokens,
            budget,
            by_provider,
            unpriced_messages,
        })
    }

    pub fn imported_chat_count(&self, source: &str) -> Result<i64, String> {
        self.connection
            .query_row(
                "SELECT COUNT(*) FROM chats WHERE source = ?1 AND deleted = 0",
                [source],
                |row| row.get(0),
            )
            .map_err(sql_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn memory_database() -> Database {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(SCHEMA).unwrap();
        add_missing_columns(&connection).unwrap();
        Database { connection }
    }

    #[test]
    fn a_compaction_counts_only_while_its_message_is_in_the_path() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let full_path = database.path_messages("a2", i64::MAX / 4).unwrap().0;
        let chat_id = full_path[0].chat_id.clone();
        assert_eq!(database.latest_compaction_in(&full_path).unwrap(), None);

        let early = Compaction { up_to_message_id: "a1".into(), summary: "раннее".into() };
        let late = Compaction { up_to_message_id: "a2".into(), summary: "позднее".into() };
        database.add_compaction(&chat_id, &early).unwrap();
        database.add_compaction(&chat_id, &late).unwrap();
        assert_eq!(database.latest_compaction_in(&full_path).unwrap(), Some(late));

        let rewound_path = database.path_messages("u2", i64::MAX / 4).unwrap().0;
        assert_eq!(database.latest_compaction_in(&rewound_path).unwrap(), Some(early));

        let first_question = database.path_messages("u1", i64::MAX / 4).unwrap().0;
        assert_eq!(database.latest_compaction_in(&first_question).unwrap(), None);

        database.delete_chat(&chat_id).unwrap();
        let left: i64 = database.connection.query_row("SELECT COUNT(*) FROM compactions", [], |row| row.get(0)).unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn the_compaction_furthest_along_the_path_wins_even_if_added_earlier() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let full_path = database.path_messages("a2", i64::MAX / 4).unwrap().0;
        let chat_id = full_path[0].chat_id.clone();
        let late = Compaction { up_to_message_id: "a2".into(), summary: "позднее".into() };
        let early = Compaction { up_to_message_id: "a1".into(), summary: "раннее".into() };
        database.add_compaction(&chat_id, &late).unwrap();
        database.add_compaction(&chat_id, &early).unwrap();
        assert_eq!(database.latest_compaction_in(&full_path).unwrap(), Some(late));
    }

    #[test]
    fn old_database_asks_for_one_reimport_after_upgrade() {
        let directory = std::env::temp_dir().join(format!("tarn-upgrade-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("t.db");
        {
            let database = Database::open(&path).unwrap();
            database
                .connection
                .execute("INSERT INTO import_files(path, source, size, modified) VALUES ('f.jsonl', 'claude_code', 1, 1)", [])
                .unwrap();
            database.connection.pragma_update(None, "user_version", 0).unwrap();
        }
        let upgraded = Database::open(&path).unwrap();
        let remaining: i64 = upgraded.connection.query_row("SELECT COUNT(*) FROM import_files", [], |row| row.get(0)).unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(upgraded.get_setting(IMPORT_REFRESH_PENDING_SETTING).as_deref(), Some("true"));
        upgraded.connection.execute("DELETE FROM settings WHERE key = ?1", [IMPORT_REFRESH_PENDING_SETTING]).unwrap();
        drop(upgraded);
        let reopened = Database::open(&path).unwrap();
        assert_eq!(reopened.get_setting(IMPORT_REFRESH_PENDING_SETTING), None);
    }

    #[test]
    fn old_database_gets_new_columns_and_keeps_its_rows() {
        let directory = std::env::temp_dir().join(format!("tarn-columns-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("t.db");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE chats (id TEXT PRIMARY KEY, title TEXT NOT NULL, source TEXT NOT NULL DEFAULT 'local', source_path TEXT, cwd TEXT,
                        model_ref TEXT, active_leaf_id TEXT, pinned INTEGER NOT NULL DEFAULT 0, deleted INTEGER NOT NULL DEFAULT 0,
                        title_locked INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
                     CREATE TABLE models (provider TEXT NOT NULL, id TEXT NOT NULL, name TEXT NOT NULL, max_output INTEGER, max_input INTEGER, PRIMARY KEY (provider, id));
                     INSERT INTO chats(id, title, created_at, updated_at) VALUES ('старый', 'Чат', 1, 1);
                     INSERT INTO models(provider, id, name) VALUES ('p', 'm', 'M');",
                )
                .unwrap();
        }
        let upgraded = Database::open(&path).unwrap();
        assert_eq!(upgraded.get_chat("старый").unwrap().access_mode, "chat");
        assert!(upgraded.list_models().unwrap()[0].effort_levels.is_empty());
        upgraded.set_chat_access("старый", "ask", Some("C:\\папка")).unwrap();
        let changed = upgraded.get_chat("старый").unwrap();
        assert_eq!(changed.access_mode, "ask");
        assert_eq!(changed.cwd.as_deref(), Some("C:\\папка"));
    }

    #[test]
    fn deleting_a_chat_removes_only_its_own_pictures() {
        let database = memory_database();
        let shared = StoredImage::from_base64("image/png", "aGVsbG8=").unwrap();
        let private = StoredImage::from_base64("image/png", "d29ybGQ=").unwrap();
        insert_image(&database.connection, "first", &shared).unwrap();
        insert_image(&database.connection, "first", &private).unwrap();
        insert_image(&database.connection, "second", &shared).unwrap();
        database.delete_chat("first").unwrap();
        assert!(database.image_base64(&shared.id).unwrap().is_some());
        assert!(database.image_base64(&private.id).unwrap().is_none());
    }

    fn add_message(database: &Database, id: &str, parent: Option<&str>, role: &str, content: &str) {
        let attachments = json!([]);
        database
            .insert_message(&NewMessage {
                id,
                chat_id: "chat",
                parent_id: parent,
                role,
                content,
                attachments: &attachments,
                provider: None,
                model: None,
                status: "done",
                compare_group: None,
            })
            .unwrap();
    }

    #[test]
    fn builds_thread_from_active_branch_with_siblings() {
        let database = memory_database();
        database.create_chat("chat", "Тест", None).unwrap();
        add_message(&database, "u1", None, "user", "первый");
        add_message(&database, "a1", Some("u1"), "assistant", "ответ один");
        add_message(&database, "a2", Some("u1"), "assistant", "ответ два");
        database.set_active_leaf("chat", "a2", None).unwrap();
        let page = database.thread_page("chat", None, 50, None).unwrap();
        let ids: Vec<&str> = page.messages.iter().map(|message| message.id.as_str()).collect();
        assert_eq!(ids, vec!["u1", "a2"]);
        assert_eq!(page.messages[1].sibling_ids, vec!["a1", "a2"]);
        assert!(!page.has_more);
    }

    fn thread_ids(database: &Database, chat_id: &str) -> Vec<String> {
        database.thread_page(chat_id, None, 50, None).unwrap().messages.into_iter().map(|message| message.id).collect()
    }

    fn chat_with_two_turns(database: &Database) {
        database.create_chat("chat", "Тест", None).unwrap();
        add_message(database, "u1", None, "user", "первый");
        add_message(database, "a1", Some("u1"), "assistant", "ответ один");
        add_message(database, "u2", Some("a1"), "user", "второй");
        add_message(database, "a2", Some("u2"), "assistant", "ответ два");
        database.set_active_leaf("chat", "a2", None).unwrap();
    }

    #[test]
    fn fork_of_a_whole_chat_copies_the_thread_and_keeps_the_original() {
        let database = memory_database();
        chat_with_two_turns(&database);
        database.connection.execute("UPDATE messages SET cost = 1.5, output_tokens = 10", []).unwrap();
        let fork = database.fork_chat("chat", None, "fork").unwrap();
        assert_eq!(fork.title, "Форк: Тест");
        let copies = database.thread_page("fork", None, 50, None).unwrap().messages;
        let texts: Vec<&str> = copies.iter().map(|message| message.content.as_str()).collect();
        assert_eq!(texts, vec!["первый", "ответ один", "второй", "ответ два"]);
        assert!(copies.iter().all(|message| message.chat_id == "fork" && message.cost.is_none() && message.output_tokens.is_none()));
        assert_eq!(thread_ids(&database, "chat"), vec!["u1", "a1", "u2", "a2"]);
        let again = database.fork_chat("fork", None, "fork2").unwrap();
        assert_eq!(again.title, "Форк: Тест");
    }

    #[test]
    fn fork_from_an_answer_ends_on_it_and_fork_from_a_question_ends_before_it() {
        let database = memory_database();
        chat_with_two_turns(&database);
        database.fork_chat("chat", Some("a1"), "from_answer").unwrap();
        assert_eq!(thread_ids(&database, "from_answer").len(), 2);
        database.fork_chat("chat", Some("u2"), "from_question").unwrap();
        assert_eq!(thread_ids(&database, "from_question").len(), 2);
        database.fork_chat("chat", Some("u1"), "from_first").unwrap();
        assert!(thread_ids(&database, "from_first").is_empty());
    }

    #[test]
    fn nothing_can_be_forked_or_rewound_during_an_answer() {
        let database = memory_database();
        chat_with_two_turns(&database);
        database.connection.execute("UPDATE messages SET status = 'streaming' WHERE id = 'a2'", []).unwrap();
        assert!(database.fork_chat("chat", None, "fork").is_err());
        assert!(database.rewind_chat("a1", false).is_err());
    }

    fn set_source_file(database: &Database, chat_id: &str, file: &str) {
        database
            .connection
            .execute("UPDATE chats SET source = 'claude_code', source_path = ?2 WHERE id = ?1", params![chat_id, file])
            .unwrap();
    }

    fn turn_with_imported_start(database: &Database) -> String {
        let file = temp_file("session.jsonl");
        std::fs::write(&file, "").unwrap();
        database.create_chat("chat", "Тест", None).unwrap();
        set_source_file(database, "chat", &file.to_string_lossy());
        add_message(database, "cc:u1", None, "user", "первый");
        add_message(database, "cc:a1", Some("cc:u1"), "assistant", "ответ");
        add_message(database, "11111111-1111-4111-8111-111111111111", Some("cc:a1"), "user", "новый вопрос");
        add_message(database, "22222222-2222-4222-8222-222222222222", Some("11111111-1111-4111-8111-111111111111"), "assistant", "новый ответ");
        database.set_active_leaf("chat", "22222222-2222-4222-8222-222222222222", None).unwrap();
        file.to_string_lossy().to_string()
    }

    #[test]
    fn export_copies_imported_messages_and_writes_only_new_ones() {
        let database = memory_database();
        let file = turn_with_imported_start(&database);
        let export = database.claude_code_export("chat").unwrap();
        assert_eq!(export.source_file.as_deref(), Some(file.as_str()));
        assert_eq!(export.copied_up_to_uuid.as_deref(), Some("a1"));
        let texts: Vec<&str> = export.turns.iter().map(|turn| turn.text.as_str()).collect();
        assert_eq!(texts, vec!["новый вопрос", "новый ответ"]);
    }

    #[test]
    fn fork_remembers_the_original_messages_for_export() {
        let database = memory_database();
        turn_with_imported_start(&database);
        let fork = database.fork_chat("chat", Some("cc:a1"), "fork").unwrap();
        assert!(fork.source_path.is_some());
        let export = database.claude_code_export("fork").unwrap();
        assert_eq!(export.copied_up_to_uuid.as_deref(), Some("a1"));
        assert!(export.turns.is_empty());
    }

    #[test]
    fn a_new_message_in_a_fork_goes_after_the_copied_part() {
        let database = memory_database();
        turn_with_imported_start(&database);
        database.fork_chat("chat", Some("cc:a1"), "fork").unwrap();
        let leaf = database.active_leaf("fork").unwrap();
        let attachments = json!([]);
        database
            .insert_message(&NewMessage {
                id: "33333333-3333-4333-8333-333333333333",
                chat_id: "fork",
                parent_id: leaf.as_deref(),
                role: "user",
                content: "Привет, ответь 1, 2, 3",
                attachments: &attachments,
                provider: None,
                model: None,
                status: "done",
                compare_group: None,
            })
            .unwrap();
        database.set_active_leaf("fork", "33333333-3333-4333-8333-333333333333", None).unwrap();
        let export = database.claude_code_export("fork").unwrap();
        assert_eq!(export.copied_up_to_uuid.as_deref(), Some("a1"));
        assert_eq!(export.turns[0].text, "Привет, ответь 1, 2, 3");
    }

    #[test]
    fn a_local_chat_is_exported_whole() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let export = database.claude_code_export("chat").unwrap();
        assert_eq!(export.source_file, None);
        assert_eq!(export.copied_up_to_uuid, None);
        assert_eq!(export.turns.len(), 4);
    }

    #[test]
    fn tool_calls_are_exported_as_readable_text() {
        let database = memory_database();
        chat_with_two_turns(&database);
        database
            .save_partial("a2", "ответ два", &json!([{ "type": "tool_call", "name": "write_file", "input": "{\"path\":\"a.txt\"}", "result": "Записан файл" }]))
            .unwrap();
        let export = database.claude_code_export("chat").unwrap();
        assert!(export.turns[3].text.contains("[write_file]") && export.turns[3].text.contains("Записан файл"));
    }

    #[test]
    fn marked_messages_stay_connected_after_getting_imported_ids() {
        let database = memory_database();
        turn_with_imported_start(&database);
        let local_ids = vec!["11111111-1111-4111-8111-111111111111".to_string(), "22222222-2222-4222-8222-222222222222".to_string()];
        database.mark_messages_as_imported("chat", &local_ids).unwrap();
        let ids = thread_ids(&database, "chat");
        assert_eq!(ids, vec!["cc:u1", "cc:a1", "cc:11111111-1111-4111-8111-111111111111", "cc:22222222-2222-4222-8222-222222222222"]);
    }

    fn temp_file(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("tarn-rewind-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        folder.join(name)
    }

    fn model_writes(database: &Database, message_id: &str, path: &Path, text: &str) {
        let before = file_history::read_before_change(path);
        std::fs::write(path, text).unwrap();
        let change = file_history::change_after_write(path, before);
        database.add_file_change(message_id, "chat", &change).unwrap();
    }

    #[test]
    fn rewind_to_a_question_returns_files_to_how_they_were_before_its_answer() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let path = temp_file("a.txt");
        std::fs::write(&path, "исходный").unwrap();
        model_writes(&database, "a1", &path, "после первого");
        model_writes(&database, "a2", &path, "после второго");
        assert_eq!(database.rewind_preview("u2").unwrap().changed_files, 1);
        let outcome = database.rewind_chat("u2", true).unwrap();
        assert_eq!(outcome.restored_files, 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "после первого");
    }

    #[test]
    fn rewind_to_the_first_question_undoes_every_answer() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let path = temp_file("a.txt");
        std::fs::write(&path, "исходный").unwrap();
        let created = temp_file("new.txt");
        model_writes(&database, "a1", &path, "после первого");
        model_writes(&database, "a2", &path, "после второго");
        model_writes(&database, "a2", &created, "новый");
        assert_eq!(database.rewind_preview("u1").unwrap().changed_files, 2);
        database.rewind_chat("u1", true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "исходный");
        assert!(!created.exists());
    }

    #[test]
    fn rewind_to_an_answer_keeps_what_that_answer_changed() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let path = temp_file("a.txt");
        model_writes(&database, "a1", &path, "после первого");
        assert_eq!(database.rewind_preview("a1").unwrap().changed_files, 0);
        let outcome = database.rewind_chat("a1", true).unwrap();
        assert_eq!(outcome.restored_files, 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "после первого");
    }

    #[test]
    fn several_writes_in_one_answer_return_to_the_state_before_the_first() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let path = temp_file("a.txt");
        std::fs::write(&path, "исходный").unwrap();
        model_writes(&database, "a2", &path, "первая запись");
        model_writes(&database, "a2", &path, "вторая запись");
        database.rewind_chat("u2", true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "исходный");
    }

    #[test]
    fn rewind_of_only_the_chat_leaves_files_alone() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let path = temp_file("a.txt");
        std::fs::write(&path, "исходный").unwrap();
        model_writes(&database, "a2", &path, "после второго");
        let outcome = database.rewind_chat("u2", false).unwrap();
        assert_eq!(outcome.restored_files, 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "после второго");
    }

    #[test]
    fn undo_puts_back_what_the_rewind_replaced_and_works_only_once() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let path = temp_file("a.txt");
        std::fs::write(&path, "исходный").unwrap();
        model_writes(&database, "a2", &path, "после второго");
        database.rewind_chat("u2", true).unwrap();
        assert!(database.undo_file_restore("chat").unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "после второго");
        std::fs::write(&path, "моя правка").unwrap();
        assert!(database.undo_file_restore("chat").unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "моя правка");
    }

    #[test]
    fn rewind_names_files_changed_after_the_model_and_keeps_them() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let path = temp_file("mine.txt");
        std::fs::write(&path, "исходный").unwrap();
        model_writes(&database, "a2", &path, "после второго");
        std::fs::write(&path, "моя правка").unwrap();
        let outcome = database.rewind_chat("u2", true).unwrap();
        assert_eq!(outcome.not_restored, vec!["mine.txt"]);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "моя правка");
    }

    #[test]
    fn another_branch_of_the_chat_does_not_count_as_later_changes() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let path = temp_file("a.txt");
        model_writes(&database, "a2", &path, "на старой ветке");
        database.rewind_chat("u2", false).unwrap();
        add_message(&database, "u2b", Some("a1"), "user", "другой вопрос");
        add_message(&database, "a2b", Some("u2b"), "assistant", "другой ответ");
        database.set_active_leaf("chat", "a2b", None).unwrap();
        assert_eq!(database.rewind_preview("u2b").unwrap().changed_files, 0);
    }

    #[test]
    fn preview_notices_commands_that_files_cannot_undo() {
        let database = memory_database();
        chat_with_two_turns(&database);
        assert!(!database.rewind_preview("u2").unwrap().commands_left_effects);
        database.save_partial("a2", "ответ два", &json!([{ "type": "tool_call", "changesBeyondFiles": true }])).unwrap();
        assert!(database.rewind_preview("u2").unwrap().commands_left_effects);
        assert!(!database.rewind_preview("a2").unwrap().commands_left_effects);
    }

    #[test]
    fn deleting_a_chat_forgets_its_saved_files() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let path = temp_file("a.txt");
        model_writes(&database, "a2", &path, "после второго");
        database.rewind_chat("u2", true).unwrap();
        database.delete_chat("chat").unwrap();
        let count_rows = |table: &str| -> i64 {
            database.connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0)).unwrap()
        };
        assert_eq!(count_rows("file_changes"), 0);
        assert_eq!(count_rows("file_undo"), 0);
    }

    #[test]
    fn rewind_keeps_later_messages_as_a_branch_that_can_be_restored() {
        let database = memory_database();
        chat_with_two_turns(&database);
        let outcome = database.rewind_chat("u2", false).unwrap();
        assert_eq!(outcome.previous_leaf.as_deref(), Some("a2"));
        assert_eq!(thread_ids(&database, "chat"), vec!["u1", "a1"]);
        add_message(&database, "u3", Some("a1"), "user", "другой");
        database.set_active_leaf("chat", "u3", None).unwrap();
        let page = database.thread_page("chat", None, 50, None).unwrap();
        assert_eq!(page.messages[2].sibling_ids, vec!["u2", "u3"]);
        database.set_active_leaf("chat", "a2", None).unwrap();
        assert_eq!(thread_ids(&database, "chat"), vec!["u1", "a1", "u2", "a2"]);
    }

    #[test]
    fn rewind_to_the_first_question_empties_the_thread_and_new_question_is_its_sibling() {
        let database = memory_database();
        chat_with_two_turns(&database);
        database.rewind_chat("u1", false).unwrap();
        assert!(thread_ids(&database, "chat").is_empty());
        add_message(&database, "u9", None, "user", "заново");
        database.set_active_leaf("chat", "u9", None).unwrap();
        let page = database.thread_page("chat", None, 50, None).unwrap();
        assert_eq!(page.messages[0].sibling_ids, vec!["u1", "u9"]);
    }

    #[test]
    fn pages_older_messages() {
        let database = memory_database();
        database.create_chat("chat", "Тест", None).unwrap();
        add_message(&database, "m1", None, "user", "1");
        add_message(&database, "m2", Some("m1"), "assistant", "2");
        add_message(&database, "m3", Some("m2"), "user", "3");
        database.set_active_leaf("chat", "m3", None).unwrap();
        let tail = database.thread_page("chat", None, 2, None).unwrap();
        assert!(tail.has_more);
        assert_eq!(tail.messages[0].id, "m2");
        let older = database.thread_page("chat", Some("m2"), 2, None).unwrap();
        assert_eq!(older.messages[0].id, "m1");
        assert!(!older.has_more);
    }

    #[test]
    fn search_finds_russian_words_by_prefix_and_ignores_deleted() {
        let database = memory_database();
        database.create_chat("chat", "Сборка проекта", None).unwrap();
        add_message(&database, "m1", None, "user", "Компоновщик не находит библиотеку");
        let hits = database.search("компоновщ", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message_id.as_deref(), Some("m1"));
        let by_title = database.search("Сборка", 10).unwrap();
        assert!(by_title[0].message_id.is_none());
        database.delete_chat("chat").unwrap();
        assert!(database.search("компоновщ", 10).unwrap().is_empty());
    }

    #[test]
    fn search_query_survives_special_characters() {
        assert_eq!(fts_query("\"a\" OR b*"), Some("\"a\"* \"OR\"* \"b\"*".to_string()));
        assert_eq!(fts_query("***"), None);
    }

    #[test]
    fn computes_cost_from_price_table() {
        let database = memory_database();
        database.set_price("m", 2.0, 10.0).unwrap();
        let cost = database.cost_for("m", Some(1_000_000), Some(1_000_000)).unwrap();
        assert!((cost - 12.0).abs() < 1e-9);
        assert!(database.cost_for("неизвестная", Some(1), Some(1)).is_none());
    }
}
