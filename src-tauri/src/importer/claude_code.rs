use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::{json, Value};

use super::{parse_timestamp, title_from_text, truncate_with_mark, ParsedChat, ParsedCompaction, ParsedMessage};
use crate::images::StoredImage;
use crate::types::Attachment;

const TOOL_INPUT_LIMIT: usize = 4_000;
const TOOL_RESULT_LIMIT: usize = 8_000;
const MAX_ANCESTOR_STEPS: usize = 10_000;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record<'a> {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    parent_uuid: Option<String>,
    #[serde(default)]
    logical_parent_uuid: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    is_sidechain: bool,
    #[serde(default)]
    is_meta: bool,
    #[serde(default)]
    is_compact_summary: bool,
    #[serde(default)]
    custom_title: Option<String>,
    #[serde(default)]
    ai_title: Option<String>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    compact_metadata: Option<CompactMetadata>,
    #[serde(borrow, default)]
    message: Option<&'a RawValue>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompactMetadata {
    #[serde(default)]
    preserved_segment: Option<PreservedSegment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreservedSegment {
    #[serde(default)]
    head_uuid: Option<String>,
}

const RESUME_INSTRUCTION_START: &str = "Continue the conversation from where it left off";

#[derive(Deserialize)]
struct RawMessage {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    content: Value,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    cache_read_input_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
}

struct Builder {
    messages: Vec<ParsedMessage>,
    index_by_uuid: HashMap<String, usize>,
    skipped_parents: HashMap<String, Option<String>>,
    has_child: Vec<bool>,
    last_index: Option<usize>,
    images: HashMap<String, StoredImage>,
}

fn keep_image(images: &mut HashMap<String, StoredImage>, block: &Value) -> Option<Attachment> {
    let image = StoredImage::from_anthropic_block(block)?;
    let attachment = image.default_attachment();
    images.entry(image.id.clone()).or_insert(image);
    Some(attachment)
}

fn keep_images_of_blocks(images: &mut HashMap<String, StoredImage>, content: &Value) -> Vec<Attachment> {
    let Some(blocks) = content.as_array() else {
        return Vec::new();
    };
    blocks.iter().filter_map(|block| keep_image(images, block)).collect()
}

impl Builder {
    fn new() -> Self {
        Self {
            images: HashMap::new(),
            messages: Vec::new(),
            index_by_uuid: HashMap::new(),
            skipped_parents: HashMap::new(),
            has_child: Vec::new(),
            last_index: None,
        }
    }

    fn resolve_parent(&self, parent: Option<&str>) -> Option<usize> {
        let mut current = parent.map(String::from);
        for _ in 0..MAX_ANCESTOR_STEPS {
            let uuid = current?;
            if let Some(index) = self.index_by_uuid.get(&uuid) {
                return Some(*index);
            }
            current = self.skipped_parents.get(&uuid).cloned().flatten();
        }
        None
    }

    fn push_message(&mut self, message: ParsedMessage, parent_index: Option<usize>, uuid: &str) -> usize {
        let index = self.messages.len();
        if let Some(parent) = parent_index {
            self.has_child[parent] = true;
        }
        self.messages.push(message);
        self.has_child.push(false);
        self.index_by_uuid.insert(uuid.to_string(), index);
        self.last_index = Some(index);
        index
    }

    fn parent_index_of(&self, index: usize) -> Option<usize> {
        let parent_id = self.messages[index].parent_id.as_deref()?;
        self.messages.iter().position(|message| message.id == parent_id)
    }

    fn last_summarized_message(&self, preserved_head: Option<&str>) -> Option<String> {
        let head_message = preserved_head.and_then(|head| self.index_by_uuid.get(head).copied());
        let Some(mut first_kept) = head_message else {
            let covered = preserved_head.and_then(|head| self.resolve_parent(Some(head))).or(self.last_index);
            return covered.map(|index| self.messages[index].id.clone());
        };
        if self.messages[first_kept].role != "user" {
            first_kept = self.parent_index_of(first_kept)?;
        }
        self.messages[first_kept].parent_id.clone()
    }

    fn parent_for_new_message(&self, explicit: Option<usize>) -> Option<usize> {
        explicit.or(self.last_index)
    }

    fn parent_id_of(&self, parent_index: Option<usize>) -> Option<String> {
        parent_index.map(|index| self.messages[index].id.clone())
    }

    fn assistant_turn(&mut self, uuid: &str, parent_index: Option<usize>, created_at: i64) -> usize {
        let resolved = self.parent_for_new_message(parent_index);
        if let Some(index) = resolved {
            if self.messages[index].role == "assistant" && !self.has_child[index] {
                self.index_by_uuid.insert(uuid.to_string(), index);
                self.last_index = Some(index);
                return index;
            }
        }
        let message = ParsedMessage {
            id: format!("cc:{uuid}"),
            parent_id: self.parent_id_of(resolved),
            role: "assistant".to_string(),
            content: String::new(),
            parts: json!([]),
            attachments: Vec::new(),
            thinking: None,
            model: None,
            input_tokens: None,
            output_tokens: None,
            created_at,
        };
        self.push_message(message, resolved, uuid)
    }
}

fn append_text(target: &mut String, addition: &str) {
    if !target.is_empty() {
        target.push_str("\n\n");
    }
    target.push_str(addition);
}

fn remove_tagged_blocks(text: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut remaining = text;
    let mut output = String::new();
    while let Some(start) = remaining.find(&open) {
        output.push_str(&remaining[..start]);
        match remaining[start..].find(&close) {
            Some(end) => remaining = &remaining[start + end + close.len()..],
            None => {
                remaining = "";
            }
        }
    }
    output.push_str(remaining);
    output
}

fn tag_value<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    Some(text[start..end].trim())
}

pub fn clean_user_text(raw: &str) -> Option<String> {
    let without_reminders = remove_tagged_blocks(raw, "system-reminder");
    let trimmed = without_reminders.trim();
    if trimmed.is_empty() || trimmed.starts_with("<local-command") {
        return None;
    }
    if let Some(command) = tag_value(trimmed, "command-name") {
        let arguments = tag_value(trimmed, "command-args").unwrap_or_default();
        return Some(format!("{command} {arguments}").trim().to_string());
    }
    Some(trimmed.to_string())
}

fn tool_result_text(block: &Value) -> String {
    match &block["content"] {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item["text"].as_str().filter(|_| item["type"] == "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn attach_tool_result(message: &mut ParsedMessage, block: &Value, images: &mut HashMap<String, StoredImage>) {
    let tool_id = block["tool_use_id"].as_str().unwrap_or_default();
    let result = truncate_with_mark(&tool_result_text(block), TOOL_RESULT_LIMIT);
    let is_error = block["is_error"].as_bool().unwrap_or(false);
    let result_images = keep_images_of_blocks(images, &block["content"]);
    let Some(parts) = message.parts.as_array_mut() else {
        return;
    };
    for part in parts.iter_mut().rev() {
        if part["id"] == tool_id {
            part["result"] = json!(result);
            part["isError"] = json!(is_error);
            if !result_images.is_empty() {
                part["images"] = json!(result_images);
            }
            return;
        }
    }
}

pub fn is_visible_user_record(record: &Value) -> bool {
    let is_service_text = record["isMeta"].as_bool().unwrap_or(false) || record["isCompactSummary"].as_bool().unwrap_or(false);
    if record["type"] != "user" || is_service_text {
        return false;
    }
    let content = &record["message"]["content"];
    let has_image = content.as_array().is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "image"));
    has_image || user_text_from_content(content).is_some()
}

fn user_text_from_content(content: &Value) -> Option<String> {
    let raw = match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block["text"].as_str().filter(|_| block["type"] == "text"))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    };
    clean_user_text(&raw)
}

fn handle_user_record(builder: &mut Builder, record: &Record, raw: RawMessage, created_at: i64) {
    let uuid = record.uuid.as_deref().unwrap_or_default();
    let parent_index = builder.resolve_parent(record.parent_uuid.as_deref().or(record.logical_parent_uuid.as_deref()));
    let tool_results: Vec<&Value> = raw
        .content
        .as_array()
        .map(|blocks| blocks.iter().filter(|block| block["type"] == "tool_result").collect())
        .unwrap_or_default();
    if !tool_results.is_empty() {
        let target = parent_index.or(builder.last_index);
        if let Some(index) = target {
            for block in &tool_results {
                attach_tool_result(&mut builder.messages[index], block, &mut builder.images);
            }
            builder.index_by_uuid.insert(uuid.to_string(), index);
        }
    }
    let attachments = keep_images_of_blocks(&mut builder.images, &raw.content);
    let text = user_text_from_content(&raw.content);
    if text.is_none() && attachments.is_empty() {
        if tool_results.is_empty() {
            builder.skipped_parents.insert(uuid.to_string(), record.parent_uuid.clone());
        }
        return;
    }
    let resolved = builder.parent_for_new_message(parent_index);
    let message = ParsedMessage {
        id: format!("cc:{uuid}"),
        parent_id: builder.parent_id_of(resolved),
        role: "user".to_string(),
        content: text.unwrap_or_default(),
        parts: json!([]),
        attachments,
        thinking: None,
        model: None,
        input_tokens: None,
        output_tokens: None,
        created_at,
    };
    builder.push_message(message, resolved, uuid);
}

fn handle_assistant_record(builder: &mut Builder, record: &Record, raw: RawMessage, created_at: i64) {
    let uuid = record.uuid.as_deref().unwrap_or_default();
    let parent_index = builder.resolve_parent(record.parent_uuid.as_deref().or(record.logical_parent_uuid.as_deref()));
    let index = builder.assistant_turn(uuid, parent_index, created_at);
    let message = &mut builder.messages[index];
    if message.model.is_none() {
        message.model = raw.model.filter(|model| !model.starts_with('<'));
    }
    if let Some(usage) = raw.usage {
        message.input_tokens = Some(usage.input_tokens + usage.cache_read_input_tokens + usage.cache_creation_input_tokens);
        message.output_tokens = Some(message.output_tokens.unwrap_or(0).max(usage.output_tokens));
    }
    let Some(blocks) = raw.content.as_array() else {
        return;
    };
    for block in blocks {
        match block["type"].as_str() {
            Some("text") => {
                let text = block["text"].as_str().unwrap_or_default();
                if text.trim().is_empty() {
                    continue;
                }
                append_text(&mut message.content, text);
                if let Some(parts) = message.parts.as_array_mut() {
                    parts.push(json!({ "type": "text", "text": text }));
                }
            }
            Some("thinking") => {
                let text = block["thinking"].as_str().unwrap_or_default();
                if !text.is_empty() {
                    let mut thinking = message.thinking.take().unwrap_or_default();
                    append_text(&mut thinking, text);
                    message.thinking = Some(thinking);
                }
            }
            Some("tool_use") => {
                let input = truncate_with_mark(&block["input"].to_string(), TOOL_INPUT_LIMIT);
                if let Some(parts) = message.parts.as_array_mut() {
                    parts.push(json!({
                        "type": "tool_call",
                        "id": block["id"],
                        "name": block["name"],
                        "input": input,
                    }));
                }
            }
            _ => {}
        }
    }
}

fn summary_text(content: &Value) -> String {
    let raw = match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block["text"].as_str().filter(|_| block["type"] == "text"))
            .collect::<Vec<_>>()
            .join("

"),
        _ => String::new(),
    };
    raw.lines()
        .filter(|line| !line.trim_start().starts_with(RESUME_INSTRUCTION_START))
        .collect::<Vec<_>>()
        .join("
")
        .trim()
        .to_string()
}

fn summary_of_record(record: &Record) -> String {
    record
        .message
        .and_then(|raw| serde_json::from_str::<RawMessage>(raw.get()).ok())
        .map(|raw| summary_text(&raw.content))
        .unwrap_or_default()
}

pub fn parse_lines(lines: impl Iterator<Item = String>, fallback_id: &str) -> ParsedChat {
    let mut builder = Builder::new();
    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut custom_title: Option<String> = None;
    let mut ai_title: Option<String> = None;
    let mut last_time = 0i64;
    let mut first_time = 0i64;
    let mut compactions: Vec<ParsedCompaction> = Vec::new();
    let mut preserved_head: Option<String> = None;

    for line in lines {
        let Ok(record) = serde_json::from_str::<Record>(&line) else {
            continue;
        };
        if record.is_sidechain {
            continue;
        }
        let kind = record.kind.as_deref().unwrap_or_default();
        if let Some(title) = &record.custom_title {
            custom_title = Some(title.clone());
        }
        if let Some(title) = &record.ai_title {
            ai_title = Some(title.clone());
        }
        if record.subtype.as_deref() == Some("compact_boundary") {
            preserved_head = record
                .compact_metadata
                .as_ref()
                .and_then(|metadata| metadata.preserved_segment.as_ref())
                .and_then(|segment| segment.head_uuid.clone());
        }
        if kind != "user" && kind != "assistant" {
            if let Some(uuid) = &record.uuid {
                let parent = record.parent_uuid.clone().or_else(|| record.logical_parent_uuid.clone());
                builder.skipped_parents.insert(uuid.clone(), parent);
            }
            continue;
        }
        let Some(uuid) = record.uuid.clone() else {
            continue;
        };
        if record.is_compact_summary {
            let summary = summary_of_record(&record);
            let last_summarized = builder.last_summarized_message(preserved_head.take().as_deref());
            if let Some(up_to_message_id) = last_summarized.filter(|_| !summary.is_empty()) {
                compactions.push(ParsedCompaction { up_to_message_id, summary });
            }
        }
        if record.is_meta || record.is_compact_summary {
            let parent = record.parent_uuid.clone().or_else(|| record.logical_parent_uuid.clone());
            builder.skipped_parents.insert(uuid, parent);
            continue;
        }
        let Some(raw_value) = record.message else {
            continue;
        };
        let Ok(raw) = serde_json::from_str::<RawMessage>(raw_value.get()) else {
            continue;
        };
        if session_id.is_none() {
            session_id = record.session_id.clone();
        }
        if cwd.is_none() {
            cwd = record.cwd.clone();
        }
        let created_at = parse_timestamp(record.timestamp.as_deref());
        if first_time == 0 {
            first_time = created_at;
        }
        last_time = last_time.max(created_at);
        if kind == "user" {
            handle_user_record(&mut builder, &record, raw, created_at);
        } else {
            handle_assistant_record(&mut builder, &record, raw, created_at);
        }
    }

    let leaf_id = builder.last_index.map(|index| builder.messages[index].id.clone());
    let first_user_text = builder
        .messages
        .iter()
        .find(|message| message.role == "user" && !message.content.trim().is_empty())
        .map(|message| title_from_text(&message.content));
    let title = custom_title
        .or(ai_title)
        .or(first_user_text)
        .unwrap_or_else(|| "Чат Claude Code".to_string());
    let identifier = session_id.unwrap_or_else(|| fallback_id.to_string());
    ParsedChat {
        id: format!("cc:{identifier}:{fallback_id}"),
        title,
        cwd,
        created_at: first_time,
        updated_at: last_time,
        leaf_id,
        messages: builder.messages,
        images: builder.images.into_values().collect(),
        compactions,
    }
}

pub fn parse_file(path: &Path) -> std::io::Result<ParsedChat> {
    let file = File::open(path)?;
    let reader = BufReader::with_capacity(256 * 1024, file);
    let fallback_id = path.file_stem().map(|stem| stem.to_string_lossy().to_string()).unwrap_or_default();
    let lines = reader.split(b'\n').filter_map(|line| {
        let bytes = line.ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    });
    Ok(parse_lines(lines, &fallback_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(source: &[&str]) -> impl Iterator<Item = String> {
        source.iter().map(|line| line.to_string()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn merges_agent_steps_into_one_assistant_message() {
        let chat = parse_lines(
            lines(&[
                r#"{"type":"user","uuid":"u1","parentUuid":null,"sessionId":"s","cwd":"I:\\dev","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"Собери проект"}}"#,
                r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"role":"assistant","model":"claude-sonnet-5-5","content":[{"type":"text","text":"Запускаю"}],"usage":{"input_tokens":5,"cache_read_input_tokens":100,"output_tokens":3}}}"#,
                r#"{"type":"assistant","uuid":"a2","parentUuid":"a1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo build"}}]}}"#,
                r#"{"type":"user","uuid":"r1","parentUuid":"a2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
                r#"{"type":"assistant","uuid":"a3","parentUuid":"r1","message":{"role":"assistant","content":[{"type":"text","text":"Готово"}]}}"#,
            ]),
            "file",
        );
        assert_eq!(chat.messages.len(), 2);
        let assistant = &chat.messages[1];
        assert_eq!(assistant.id, "cc:a1");
        assert_eq!(assistant.parent_id.as_deref(), Some("cc:u1"));
        assert_eq!(assistant.content, "Запускаю\n\nГотово");
        assert_eq!(assistant.input_tokens, Some(105));
        assert_eq!(assistant.parts[1]["name"], "Bash");
        assert_eq!(assistant.parts[1]["result"], "ok");
        assert_eq!(chat.leaf_id.as_deref(), Some("cc:a1"));
        assert_eq!(chat.cwd.as_deref(), Some("I:\\dev"));
        assert_eq!(chat.title, "Собери проект");
    }

    #[test]
    fn skips_sidechains_meta_and_service_records_but_keeps_links() {
        let chat = parse_lines(
            lines(&[
                r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"role":"user","content":"Первый"}}"#,
                r#"{"type":"attachment","uuid":"x1","parentUuid":"u1"}"#,
                r#"{"type":"user","uuid":"meta","parentUuid":"x1","isMeta":true,"message":{"role":"user","content":"служебное"}}"#,
                r#"{"type":"assistant","uuid":"side","parentUuid":"u1","isSidechain":true,"message":{"role":"assistant","content":[{"type":"text","text":"субагент"}]}}"#,
                r#"{"type":"assistant","uuid":"a1","parentUuid":"meta","message":{"role":"assistant","content":[{"type":"text","text":"Ответ"}]}}"#,
                r#"{"type":"api-request-blob","hash":"h","message":{"content":[{"type":"text","text":"мусор"}]}}"#,
                "not json at all",
            ]),
            "file",
        );
        assert_eq!(chat.messages.len(), 2);
        assert_eq!(chat.messages[1].parent_id.as_deref(), Some("cc:u1"));
    }

    fn chat_compacted_with_preserved_head(head: &str) -> ParsedChat {
        let boundary = format!(
            r#"{{"type":"system","uuid":"b1","parentUuid":null,"logicalParentUuid":"a2","subtype":"compact_boundary","compactMetadata":{{"preservedSegment":{{"headUuid":"{head}","tailUuid":"a2"}}}}}}"#
        );
        parse_lines(
            vec![
                r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"role":"user","content":"Первое"}}"#.to_string(),
                r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"role":"assistant","content":[{"type":"text","text":"Ответ один"}]}}"#.to_string(),
                r#"{"type":"user","uuid":"u2","parentUuid":"a1","message":{"role":"user","content":"Второе"}}"#.to_string(),
                r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","message":{"role":"assistant","content":[{"type":"text","text":"Ответ два"}]}}"#.to_string(),
                boundary,
                r#"{"type":"user","uuid":"c1","parentUuid":"b1","isCompactSummary":true,"message":{"role":"user","content":"Сводка\nContinue the conversation from where it left off without asking the user any further questions."}}"#.to_string(),
                r#"{"type":"user","uuid":"u3","parentUuid":"c1","message":{"role":"user","content":"Третье"}}"#.to_string(),
            ]
            .into_iter(),
            "file",
        )
    }

    #[test]
    fn messages_kept_verbatim_by_claude_code_stay_outside_the_summary() {
        let chat = chat_compacted_with_preserved_head("u2");
        assert_eq!(chat.compactions.len(), 1);
        assert_eq!(chat.compactions[0].up_to_message_id, "cc:a1");
        assert_eq!(chat.compactions[0].summary, "Сводка");
        assert_eq!(chat.messages.len(), 5);
    }

    #[test]
    fn kept_part_starting_inside_an_answer_begins_with_its_question() {
        let chat = chat_compacted_with_preserved_head("a2");
        assert_eq!(chat.compactions[0].up_to_message_id, "cc:a1");
    }

    #[test]
    fn keeps_branches_from_edited_prompts() {
        let chat = parse_lines(
            lines(&[
                r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"role":"user","content":"Вопрос"}}"#,
                r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"role":"assistant","content":[{"type":"text","text":"Ответ"}]}}"#,
                r#"{"type":"user","uuid":"u2","parentUuid":"a1","message":{"role":"user","content":"Вариант один"}}"#,
                r#"{"type":"user","uuid":"u3","parentUuid":"a1","message":{"role":"user","content":"Вариант два"}}"#,
            ]),
            "file",
        );
        assert_eq!(chat.messages[2].parent_id.as_deref(), Some("cc:a1"));
        assert_eq!(chat.messages[3].parent_id.as_deref(), Some("cc:a1"));
        assert_eq!(chat.leaf_id.as_deref(), Some("cc:u3"));
    }

    #[test]
    fn continues_across_compaction_boundary() {
        let chat = parse_lines(
            lines(&[
                r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"role":"user","content":"До сжатия"}}"#,
                r#"{"type":"system","uuid":"b1","parentUuid":null,"logicalParentUuid":"u1","subtype":"compact_boundary"}"#,
                r#"{"type":"user","uuid":"c1","parentUuid":"b1","isCompactSummary":true,"message":{"role":"user","content":"Сводка"}}"#,
                r#"{"type":"user","uuid":"u2","parentUuid":"c1","message":{"role":"user","content":"После сжатия"}}"#,
            ]),
            "file",
        );
        assert_eq!(chat.messages.len(), 2);
        assert_eq!(chat.messages[1].parent_id.as_deref(), Some("cc:u1"));
        assert_eq!(chat.compactions, [ParsedCompaction { up_to_message_id: "cc:u1".into(), summary: "Сводка".into() }]);
    }

    #[test]
    fn keeps_pictures_of_user_messages_and_tool_results() {
        let chat = parse_lines(
            lines(&[
                r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGVsbG8="}}]}}"#,
                r#"{"type":"user","uuid":"u2","parentUuid":"u1","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGVsbG8="}},{"type":"text","text":"Что тут?"}]}}"#,
                r#"{"type":"assistant","uuid":"a1","parentUuid":"u2","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"a.png"}}]}}"#,
                r#"{"type":"user","uuid":"r1","parentUuid":"a1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"d29ybGQ="}}]}]}}"#,
            ]),
            "file",
        );
        assert_eq!(chat.messages.len(), 3);
        assert_eq!(chat.messages[0].content, "");
        assert_eq!(chat.messages[0].attachments.len(), 1);
        assert_eq!(chat.messages[1].content, "Что тут?");
        assert_eq!(chat.messages[0].attachments[0].image_id, chat.messages[1].attachments[0].image_id);
        assert_eq!(chat.title, "Что тут?");
        let tool_images = chat.messages[2].parts[0]["images"].as_array().unwrap();
        assert_eq!(tool_images.len(), 1);
        assert_eq!(tool_images[0]["mime"], "image/jpeg");
        assert_eq!(chat.images.len(), 2);
    }

    #[test]
    fn prefers_custom_title_and_cleans_wrappers() {
        let chat = parse_lines(
            lines(&[
                r#"{"type":"custom-title","customTitle":"Мой чат","sessionId":"s"}"#,
                r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"role":"user","content":"<system-reminder>скрыто</system-reminder>Видно"}}"#,
            ]),
            "file",
        );
        assert_eq!(chat.title, "Мой чат");
        assert_eq!(chat.messages[0].content, "Видно");
        assert_eq!(clean_user_text("<command-name>/foo</command-name><command-args>bar</command-args>").as_deref(), Some("/foo bar"));
        assert_eq!(clean_user_text("<local-command-stdout>x</local-command-stdout>"), None);
    }
}
