use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::time::{Duration, SystemTime};

use chrono::{SecondsFormat, TimeZone, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::importer::claude_code::is_visible_user_record;

const RECORD_VERSION_WHEN_UNKNOWN: &str = "2.1.0";
const MODEL_WHEN_UNKNOWN: &str = "tarn";
const RECENT_WRITE_MEANS_IN_USE: Duration = Duration::from_secs(30);
const CLIENT_CHAT_LISTS_FOLDER: &str = "claude-code-sessions";
const CLIENT_CHAT_FILE_PREFIX: &str = "local_";
const MODEL_FOR_CLIENT_WHEN_UNKNOWN: &str = "claude-sonnet-5-5";
const PERMISSION_MODE_FOR_CLIENT: &str = "acceptEdits";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, PartialEq)]
pub struct ImageBlock {
    pub mime: String,
    pub data_base64: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportTurn {
    pub uuid: String,
    pub role: String,
    pub text: String,
    pub images: Vec<ImageBlock>,
    pub model: Option<String>,
    pub created_at: i64,
}

pub struct SessionPlan<'a> {
    pub session_id: &'a str,
    pub cwd: &'a str,
    pub title: &'a str,
    pub copied_up_to_uuid: Option<&'a str>,
    pub new_turns: &'a [ExportTurn],
}

fn parent_of(record: &Value) -> Option<&str> {
    record["parentUuid"].as_str().or_else(|| record["logicalParentUuid"].as_str())
}

fn parse_records(lines: &[String]) -> Vec<Option<Value>> {
    lines.iter().map(|line| serde_json::from_str(line).ok()).collect()
}

fn uuid_of(record: &Value) -> Option<&str> {
    record["uuid"].as_str()
}

fn is_part_of_conversation(record: &Value) -> bool {
    uuid_of(record).is_some() && !record["isSidechain"].as_bool().unwrap_or(false)
}

fn last_uuid_of_turn(records: &[Option<Value>], start_uuid: &str) -> String {
    let conversation: Vec<&Value> = records.iter().flatten().filter(|record| is_part_of_conversation(record)).collect();
    let start_is_question = conversation
        .iter()
        .find(|record| uuid_of(record) == Some(start_uuid))
        .is_some_and(|record| is_visible_user_record(record));
    if start_is_question {
        return start_uuid.to_string();
    }
    let mut continuations: HashMap<&str, &str> = HashMap::new();
    for record in &conversation {
        if let (Some(parent), Some(uuid)) = (parent_of(record), uuid_of(record)) {
            if !is_visible_user_record(record) {
                continuations.insert(parent, uuid);
            }
        }
    }
    let mut current = start_uuid;
    while let Some(next) = continuations.get(current) {
        current = next;
    }
    current.to_string()
}

fn ancestors_including(records: &[Option<Value>], uuid: &str) -> HashSet<String> {
    let parents: HashMap<&str, &str> = records
        .iter()
        .flatten()
        .filter_map(|record| Some((uuid_of(record)?, parent_of(record)?)))
        .collect();
    let mut chain = HashSet::new();
    let mut current = uuid;
    loop {
        if !chain.insert(current.to_string()) {
            return chain;
        }
        match parents.get(current) {
            Some(parent) => current = parent,
            None => return chain,
        }
    }
}

fn timestamp_text(millis: i64) -> String {
    Utc.timestamp_millis_opt(millis).single().unwrap_or_else(Utc::now).to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn user_content(turn: &ExportTurn) -> Value {
    if turn.images.is_empty() {
        return json!(turn.text);
    }
    let mut blocks: Vec<Value> = turn
        .images
        .iter()
        .map(|image| json!({ "type": "image", "source": { "type": "base64", "media_type": image.mime, "data": image.data_base64 } }))
        .collect();
    if !turn.text.is_empty() {
        blocks.push(json!({ "type": "text", "text": turn.text }));
    }
    json!(blocks)
}

fn message_of(turn: &ExportTurn) -> Value {
    if turn.role == "user" {
        return json!({ "role": "user", "content": user_content(turn) });
    }
    json!({
        "id": format!("msg_tarn_{}", turn.uuid),
        "type": "message",
        "role": "assistant",
        "model": turn.model.as_deref().unwrap_or(MODEL_WHEN_UNKNOWN),
        "content": [{ "type": "text", "text": turn.text }],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": { "input_tokens": 0, "output_tokens": 0 },
    })
}

fn record_of(turn: &ExportTurn, parent: Option<&str>, plan: &SessionPlan, version: &str) -> String {
    json!({
        "parentUuid": parent,
        "isSidechain": false,
        "userType": "external",
        "entrypoint": "cli",
        "cwd": plan.cwd,
        "sessionId": plan.session_id,
        "version": version,
        "type": turn.role,
        "message": message_of(turn),
        "uuid": turn.uuid,
        "timestamp": timestamp_text(turn.created_at),
    })
    .to_string()
}

pub fn build_session_lines(source_lines: &[String], plan: &SessionPlan) -> Vec<String> {
    let records = parse_records(source_lines);
    let last_copied = plan.copied_up_to_uuid.map(|uuid| last_uuid_of_turn(&records, uuid));
    let kept = last_copied.as_deref().map(|uuid| ancestors_including(&records, uuid)).unwrap_or_default();
    let mut lines = Vec::new();
    let mut version = RECORD_VERSION_WHEN_UNKNOWN.to_string();
    for record in records.iter().flatten() {
        let is_kept = uuid_of(record).is_some_and(|uuid| kept.contains(uuid));
        if !is_kept {
            continue;
        }
        if let Some(text) = record["version"].as_str() {
            version = text.to_string();
        }
        let mut copy = record.clone();
        copy["sessionId"] = json!(plan.session_id);
        lines.push(copy.to_string());
    }
    let mut parent = last_copied;
    for turn in plan.new_turns {
        lines.push(record_of(turn, parent.as_deref(), plan, &version));
        parent = Some(turn.uuid.clone());
    }
    lines.push(json!({ "type": "custom-title", "customTitle": plan.title, "sessionId": plan.session_id }).to_string());
    if let Some(last_question) = plan.new_turns.iter().rev().find(|turn| turn.role == "user") {
        lines.push(json!({ "type": "last-prompt", "lastPrompt": last_question.text, "leafUuid": parent, "sessionId": plan.session_id }).to_string());
    }
    lines
}

pub fn project_folder_name(cwd: &str) -> String {
    cwd.chars().map(|letter| if letter.is_ascii_alphanumeric() { letter } else { '-' }).collect()
}

pub fn read_lines(path: &Path) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("Не удалось прочитать чат Claude Code: {error}"))?;
    Ok(String::from_utf8_lossy(&bytes).lines().map(str::to_string).collect())
}

pub fn hash_of_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("Не удалось прочитать чат Claude Code: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn write_lines_atomically(path: &Path, lines: &[String]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("Не удалось создать папку: {error}"))?;
    }
    let unfinished = path.with_extension("tarn-writing");
    let text = format!("{}\n", lines.join("\n"));
    std::fs::write(&unfinished, text).map_err(|error| format!("Не удалось записать чат: {error}"))?;
    std::fs::rename(&unfinished, path).map_err(|error| format!("Не удалось заменить чат: {error}"))
}

fn process_is_alive(process_id: u64) -> bool {
    let mut command = Command::new("tasklist");
    command.args(["/FI", &format!("PID eq {process_id}"), "/NH", "/FO", "CSV"]);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(&format!("\"{process_id}\"")))
        .unwrap_or(true)
}

pub fn session_listed_as_running(sessions_folder: &Path, session_id: &str, is_alive: impl Fn(u64) -> bool) -> bool {
    let Ok(entries) = std::fs::read_dir(sessions_folder) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let Ok(bytes) = std::fs::read(entry.path()) else {
            return false;
        };
        let Ok(record) = serde_json::from_slice::<Value>(&bytes) else {
            return false;
        };
        record["sessionId"] == session_id && record["pid"].as_u64().is_some_and(&is_alive)
    })
}

fn was_written_recently(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age < RECENT_WRITE_MEANS_IN_USE)
}

pub fn refuse_if_in_use(file: &Path, session_id: &str, claude_folder: &Path) -> Result<(), String> {
    let is_running = session_listed_as_running(&claude_folder.join("sessions"), session_id, process_is_alive);
    if is_running || was_written_recently(file) {
        return Err("Этот чат сейчас открыт в Claude Code. Закрой его и повтори".to_string());
    }
    Ok(())
}

pub fn backup_path(backups_folder: &Path, session_id: &str, millis: i64) -> PathBuf {
    backups_folder.join(format!("{session_id}-{millis}.jsonl"))
}

fn subfolders_of(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect()
}

fn is_client_chat_file(path: &Path) -> bool {
    let name = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
    name.starts_with(CLIENT_CHAT_FILE_PREFIX) && name.ends_with(".json")
}

fn newest_client_chat_file(folder: &Path) -> Option<(SystemTime, PathBuf)> {
    let entries = std::fs::read_dir(folder).ok()?;
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_client_chat_file(path))
        .filter_map(|path| Some((std::fs::metadata(&path).ok()?.modified().ok()?, path)))
        .max_by_key(|(modified, _)| *modified)
}

fn chat_list_folders_under(lists_folder: &Path) -> Vec<PathBuf> {
    subfolders_of(lists_folder).iter().flat_map(|account| subfolders_of(account)).collect()
}

pub fn find_client_chat_list(app_data_folders: &[PathBuf]) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for folder in app_data_folders {
        candidates.extend(chat_list_folders_under(&folder.join(CLIENT_CHAT_LISTS_FOLDER)));
        for inner in subfolders_of(folder) {
            candidates.extend(chat_list_folders_under(&inner.join(CLIENT_CHAT_LISTS_FOLDER)));
        }
    }
    candidates
        .into_iter()
        .filter_map(|folder| Some((newest_client_chat_file(&folder)?.0, folder)))
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, folder)| folder)
}

fn model_for_client(turns: &[ExportTurn], chat_list_folder: &Path) -> String {
    let last_claude_model = turns.iter().rev().filter_map(|turn| turn.model.as_deref()).find(|model| model.starts_with("claude-"));
    if let Some(model) = last_claude_model {
        return model.to_string();
    }
    newest_client_chat_file(chat_list_folder)
        .and_then(|(_, path)| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|record| record["model"].as_str().map(str::to_string))
        .unwrap_or_else(|| MODEL_FOR_CLIENT_WHEN_UNKNOWN.to_string())
}

pub fn add_to_client_chat_list(chat_list_folder: &Path, plan: &SessionPlan, millis: i64) -> Result<PathBuf, String> {
    let client_id = format!("{CLIENT_CHAT_FILE_PREFIX}{}", uuid::Uuid::new_v4());
    let record = json!({
        "sessionId": client_id,
        "cliSessionId": plan.session_id,
        "cwd": plan.cwd,
        "originCwd": plan.cwd,
        "createdAt": millis,
        "lastActivityAt": millis,
        "lastFocusedAt": millis,
        "model": model_for_client(plan.new_turns, chat_list_folder),
        "isArchived": false,
        "permissionMode": PERMISSION_MODE_FOR_CLIENT,
        "title": plan.title,
        "titleSource": "auto",
        "alwaysAllowedReasons": [],
        "sessionPermissionUpdates": [],
        "remoteMcpServersConfig": [],
        "classifierSummaryEnabled": true,
        "spawnSeed": {},
    });
    let path = chat_list_folder.join(format!("{client_id}.json"));
    write_lines_atomically(&path, &[record.to_string()])?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::claude_code::parse_lines;

    fn question(uuid: &str, parent: Option<&str>, text: &str) -> String {
        json!({ "type": "user", "uuid": uuid, "parentUuid": parent, "sessionId": "old", "version": "2.1.265", "timestamp": "2026-11-06T10:00:00.000Z", "cwd": "I:\\dev", "message": { "role": "user", "content": text } }).to_string()
    }

    fn answer(uuid: &str, parent: &str, block: Value) -> String {
        json!({ "type": "assistant", "uuid": uuid, "parentUuid": parent, "sessionId": "old", "version": "2.1.265", "timestamp": "2026-11-06T10:00:01.000Z", "message": { "role": "assistant", "model": "claude-haiku", "content": [block] } }).to_string()
    }

    fn tool_result(uuid: &str, parent: &str) -> String {
        json!({ "type": "user", "uuid": uuid, "parentUuid": parent, "sessionId": "old", "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "t1", "content": "длинный результат" }] } }).to_string()
    }

    fn source_with_tools() -> Vec<String> {
        vec![
            question("q1", None, "первый"),
            answer("a1", "q1", json!({ "type": "tool_use", "id": "t1", "name": "Read", "input": {} })),
            tool_result("r1", "a1"),
            answer("a2", "r1", json!({ "type": "text", "text": "ответ один" })),
            question("q2", Some("a2"), "второй"),
            answer("a3", "q2", json!({ "type": "text", "text": "ответ два" })),
            json!({ "type": "bridge-session", "sessionId": "old", "bridgeSessionId": "b" }).to_string(),
        ]
    }

    fn turn(uuid: &str, role: &str, text: &str) -> ExportTurn {
        ExportTurn { uuid: uuid.to_string(), role: role.to_string(), text: text.to_string(), images: Vec::new(), model: Some("claude-haiku".to_string()), created_at: 1_793_990_000_000 }
    }

    fn plan<'a>(copied_up_to_uuid: Option<&'a str>, new_turns: &'a [ExportTurn]) -> SessionPlan<'a> {
        SessionPlan { session_id: "new-session", cwd: "I:\\dev", title: "Форк: тест", copied_up_to_uuid, new_turns }
    }

    fn parsed(lines: Vec<String>) -> crate::importer::ParsedChat {
        parse_lines(lines.into_iter(), "new-session")
    }

    #[test]
    fn new_turns_continue_after_the_whole_copied_answer() {
        let turns = [turn("n1", "user", "Привет, ответь 1, 2, 3"), turn("n2", "assistant", "1, 2, 3")];
        let lines = build_session_lines(&source_with_tools(), &plan(Some("a3"), &turns));
        let chat = parsed(lines);
        let texts: Vec<&str> = chat.messages.iter().map(|message| message.content.as_str()).collect();
        assert_eq!(texts, vec!["первый", "ответ один", "второй", "ответ два", "Привет, ответь 1, 2, 3", "1, 2, 3"]);
        assert_eq!(chat.leaf_id.as_deref(), Some("cc:n2"));
        assert_eq!(chat.title, "Форк: тест");
    }

    #[test]
    fn copy_ends_on_the_last_record_of_a_turn_with_tools() {
        let turns = [turn("n1", "user", "дальше")];
        let lines = build_session_lines(&source_with_tools(), &plan(Some("a1"), &turns));
        let new_record: Value = serde_json::from_str(lines.iter().find(|line| line.contains("дальше")).unwrap()).unwrap();
        assert_eq!(new_record["parentUuid"], "a2");
        assert!(!lines.iter().any(|line| line.contains("второй")));
    }

    #[test]
    fn copy_up_to_a_question_leaves_out_its_answer() {
        let lines = build_session_lines(&source_with_tools(), &plan(Some("q2"), &[]));
        assert!(lines.iter().any(|line| line.contains("второй")));
        assert!(!lines.iter().any(|line| line.contains("ответ два")));
    }

    #[test]
    fn copied_records_keep_their_full_content_and_get_the_new_session() {
        let lines = build_session_lines(&source_with_tools(), &plan(Some("a3"), &[]));
        assert!(lines.iter().any(|line| line.contains("длинный результат")));
        assert!(!lines.iter().any(|line| line.contains("\"sessionId\":\"old\"")));
    }

    #[test]
    fn service_records_of_the_old_session_are_not_copied() {
        let lines = build_session_lines(&source_with_tools(), &plan(Some("a3"), &[]));
        assert!(!lines.iter().any(|line| line.contains("bridge-session")));
    }

    #[test]
    fn copy_passes_through_a_compaction_point() {
        let source = vec![
            question("q1", None, "до сжатия"),
            answer("a1", "q1", json!({ "type": "text", "text": "ответ" })),
            json!({ "type": "system", "subtype": "compact_boundary", "uuid": "b1", "parentUuid": null, "logicalParentUuid": "a1", "sessionId": "old" }).to_string(),
            question("q2", Some("b1"), "после сжатия"),
        ];
        let lines = build_session_lines(&source, &plan(Some("q2"), &[]));
        assert!(lines.iter().any(|line| line.contains("до сжатия")));
        assert!(lines.iter().any(|line| line.contains("compact_boundary")));
    }

    #[test]
    fn a_chat_with_no_original_file_starts_a_new_conversation() {
        let turns = [turn("n1", "user", "вопрос"), turn("n2", "assistant", "ответ")];
        let lines = build_session_lines(&[], &plan(None, &turns));
        let first: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(first["parentUuid"], Value::Null);
        assert_eq!(parsed(lines).messages.len(), 2);
    }

    #[test]
    fn images_are_written_as_image_blocks() {
        let mut with_image = turn("n1", "user", "что тут");
        with_image.images.push(ImageBlock { mime: "image/png".to_string(), data_base64: "QUJD".to_string() });
        let lines = build_session_lines(&[], &plan(None, std::slice::from_ref(&with_image)));
        let record: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(record["message"]["content"][0]["source"]["data"], "QUJD");
        assert_eq!(record["message"]["content"][1]["text"], "что тут");
    }

    #[test]
    #[ignore]
    fn writes_a_session_for_a_check_with_the_real_claude_code() {
        let source = std::env::var("TARN_EXPORT_CHECK_SOURCE").unwrap();
        let target = std::env::var("TARN_EXPORT_CHECK_TARGET").unwrap();
        let session_id = std::path::Path::new(&target).file_stem().unwrap().to_string_lossy().to_string();
        let source_lines = read_lines(Path::new(&source)).unwrap();
        let parsed = parse_lines(source_lines.clone().into_iter(), "source");
        let first_answer = parsed.messages.iter().find(|message| message.role == "assistant").unwrap();
        let turns = [
            turn(&uuid::Uuid::new_v4().to_string(), "user", "Привет, ответь 1, 2, 3"),
            turn(&uuid::Uuid::new_v4().to_string(), "assistant", "1, 2, 3"),
        ];
        let plan = SessionPlan {
            session_id: &session_id,
            cwd: "I:\\dev",
            title: "Проверка Tarn: 1, 2, 3",
            copied_up_to_uuid: first_answer.id.strip_prefix("cc:"),
            new_turns: &turns,
        };
        write_lines_atomically(Path::new(&target), &build_session_lines(&source_lines, &plan)).unwrap();
    }

    fn client_data_with_one_chat() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("tarn-client-{}", uuid::Uuid::new_v4()));
        let list = root.join("claude_app").join(CLIENT_CHAT_LISTS_FOLDER).join("account").join("organization");
        std::fs::create_dir_all(&list).unwrap();
        let existing = json!({ "sessionId": "local_old", "model": "claude-opus-5-5" }).to_string();
        std::fs::write(list.join("local_old.json"), existing).unwrap();
        (root, list)
    }

    #[test]
    fn finds_the_chat_list_of_the_client() {
        let (root, list) = client_data_with_one_chat();
        assert_eq!(find_client_chat_list(&[root.clone()]), Some(list));
        std::fs::create_dir_all(root.join("empty").join(CLIENT_CHAT_LISTS_FOLDER).join("a").join("b")).unwrap();
        let only_empty = root.join("empty");
        assert_eq!(find_client_chat_list(&[only_empty]), None);
        assert_eq!(find_client_chat_list(&[root.join("нет такой папки")]), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adds_a_separate_entry_and_leaves_other_chats_alone() {
        let (root, list) = client_data_with_one_chat();
        let before = std::fs::read(list.join("local_old.json")).unwrap();
        let turns = [ExportTurn { model: Some("claude-haiku-5-5".to_string()), ..turn("t1", "assistant", "1, 2, 3") }];
        let plan = SessionPlan { session_id: "cli-id", cwd: "I:\\dev", title: "Проверка", copied_up_to_uuid: None, new_turns: &turns };
        let path = add_to_client_chat_list(&list, &plan, 1_000).unwrap();

        let record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let file_stem = path.file_stem().unwrap().to_string_lossy().to_string();
        assert!(file_stem.starts_with(CLIENT_CHAT_FILE_PREFIX));
        assert_eq!(record["sessionId"], file_stem);
        assert_eq!(record["cliSessionId"], "cli-id");
        assert_eq!(record["cwd"], "I:\\dev");
        assert_eq!(record["originCwd"], "I:\\dev");
        assert_eq!(record["title"], "Проверка");
        assert_eq!(record["model"], "claude-haiku-5-5");
        assert_eq!(record["permissionMode"], PERMISSION_MODE_FOR_CLIENT);
        assert_eq!(record["isArchived"], false);
        assert_eq!(std::fs::read(list.join("local_old.json")).unwrap(), before);
        let unfinished_left = std::fs::read_dir(&list).unwrap().flatten().any(|entry| entry.path().extension().is_some_and(|extension| extension == "tarn-writing"));
        assert!(!unfinished_left);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_chat_of_another_provider_gets_the_model_used_last_in_the_client() {
        let (root, list) = client_data_with_one_chat();
        let turns = [ExportTurn { model: Some("gpt-5".to_string()), ..turn("t1", "assistant", "ответ") }];
        assert_eq!(model_for_client(&turns, &list), "claude-opus-5-5");
        std::fs::remove_file(list.join("local_old.json")).unwrap();
        assert_eq!(model_for_client(&turns, &list), MODEL_FOR_CLIENT_WHEN_UNKNOWN);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_folder_name_matches_claude_code() {
        assert_eq!(project_folder_name("I:\\dev"), "I--dev");
    }

    #[test]
    fn a_session_is_running_only_when_its_process_is_alive() {
        let folder = std::env::temp_dir().join(format!("tarn-sessions-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("100.json"), json!({ "pid": 100, "sessionId": "s1" }).to_string()).unwrap();
        assert!(session_listed_as_running(&folder, "s1", |_| true));
        assert!(!session_listed_as_running(&folder, "s1", |_| false));
        assert!(!session_listed_as_running(&folder, "s2", |_| true));
    }

    #[test]
    fn a_freshly_written_file_counts_as_in_use() {
        let file = std::env::temp_dir().join(format!("tarn-fresh-{}.jsonl", uuid::Uuid::new_v4()));
        std::fs::write(&file, "x").unwrap();
        let empty_claude_folder = std::env::temp_dir().join(format!("tarn-claude-{}", uuid::Uuid::new_v4()));
        assert!(refuse_if_in_use(&file, "s1", &empty_claude_folder).is_err());
    }
}
