use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::{json, Value};

use super::{parse_timestamp, title_from_text, truncate_with_mark, ParsedChat, ParsedCompaction, ParsedMessage};
use crate::images::StoredImage;
use crate::types::Attachment;

const TOOL_INPUT_LIMIT: usize = 4_000;
const TOOL_RESULT_LIMIT: usize = 8_000;
const WRAPPER_PREFIXES: [&str; 5] = [
    "<environment_context>",
    "<user_instructions>",
    "<INSTRUCTIONS>",
    "<permissions instructions>",
    "# AGENTS.md",
];

fn is_wrapper_text(text: &str) -> bool {
    let trimmed = text.trim_start();
    WRAPPER_PREFIXES.iter().any(|prefix| trimmed.starts_with(prefix))
}

fn message_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item["type"].as_str() {
                Some("input_text") | Some("output_text") | Some("text") => item["text"].as_str().map(String::from),
                Some("input_image") | Some("image") => Some("[изображение]".to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

fn inline_image_of_item(item: &Value) -> Option<StoredImage> {
    if item["type"] != "input_image" {
        return None;
    }
    StoredImage::from_data_url(item["image_url"].as_str()?)
}

fn text_without_inline_images(content: &Value) -> String {
    let Some(items) = content.as_array() else {
        return message_text(content);
    };
    let remaining: Vec<Value> = items.iter().filter(|item| inline_image_of_item(item).is_none()).cloned().collect();
    message_text(&Value::Array(remaining))
}

fn output_text(output: &Value) -> String {
    match output {
        Value::String(text) => text.clone(),
        Value::Array(_) => message_text(output),
        Value::Object(map) => map
            .get("content")
            .or_else(|| map.get("output"))
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| output.to_string()),
        _ => output.to_string(),
    }
}

struct Parser {
    session_id: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    messages: Vec<ParsedMessage>,
    images: HashMap<String, StoredImage>,
    first_time: i64,
    last_time: i64,
    file_id: String,
    ordinal: usize,
    compactions: Vec<ParsedCompaction>,
}

impl Parser {
    fn next_id(&mut self) -> String {
        self.ordinal += 1;
        let session = self.session_id.clone().unwrap_or_else(|| self.file_id.clone());
        format!("cx:{session}:{}", self.ordinal)
    }

    fn push(&mut self, role: &str, content: String, created_at: i64) {
        let id = self.next_id();
        let parent_id = self.messages.last().map(|message| message.id.clone());
        self.messages.push(ParsedMessage {
            id,
            parent_id,
            role: role.to_string(),
            content,
            parts: json!([]),
            attachments: Vec::new(),
            thinking: None,
            model: if role == "assistant" { self.model.clone() } else { None },
            input_tokens: None,
            output_tokens: None,
            created_at,
        });
    }

    fn current_turn(&mut self, created_at: i64) -> &mut ParsedMessage {
        let needs_new = self.messages.last().is_none_or(|message| message.role != "assistant");
        if needs_new {
            self.push("assistant", String::new(), created_at);
        }
        self.messages.last_mut().expect("сообщение только что добавлено")
    }

    fn handle_item(&mut self, item: &Value, created_at: i64) {
        match item["type"].as_str().unwrap_or_default() {
            "message" => self.handle_message(item, created_at),
            "reasoning" => self.handle_reasoning(item, created_at),
            "function_call" | "custom_tool_call" | "local_shell_call" | "web_search_call" | "tool_search_call" | "image_generation_call" => {
                self.handle_call(item, created_at)
            }
            "function_call_output" | "custom_tool_call_output" | "local_shell_call_output" | "tool_search_output" => self.handle_output(item),
            _ => {}
        }
    }

    fn keep_inline_images(&mut self, content: &Value) -> Vec<Attachment> {
        let items = content.as_array().map(Vec::as_slice).unwrap_or_default();
        let mut attachments = Vec::new();
        for image in items.iter().filter_map(inline_image_of_item) {
            attachments.push(image.default_attachment());
            self.images.entry(image.id.clone()).or_insert(image);
        }
        attachments
    }

    fn handle_user_message(&mut self, content: &Value, created_at: i64) {
        let text = text_without_inline_images(content);
        if is_wrapper_text(&text) {
            return;
        }
        let attachments = self.keep_inline_images(content);
        if text.trim().is_empty() && attachments.is_empty() {
            return;
        }
        self.push("user", text, created_at);
        if let Some(message) = self.messages.last_mut() {
            message.attachments = attachments;
        }
    }

    fn handle_message(&mut self, item: &Value, created_at: i64) {
        let role = item["role"].as_str().unwrap_or_default();
        if role == "user" {
            self.handle_user_message(&item["content"], created_at);
            return;
        }
        let text = message_text(&item["content"]);
        if text.trim().is_empty() {
            return;
        }
        match role {
            "assistant" => {
                let model = self.model.clone();
                let turn = self.current_turn(created_at);
                if turn.model.is_none() {
                    turn.model = model;
                }
                if !turn.content.is_empty() {
                    turn.content.push_str("\n\n");
                }
                turn.content.push_str(&text);
                if let Some(parts) = turn.parts.as_array_mut() {
                    parts.push(json!({ "type": "text", "text": text }));
                }
            }
            _ => {}
        }
    }

    fn handle_reasoning(&mut self, item: &Value, created_at: i64) {
        let Some(summary) = item["summary"].as_array() else {
            return;
        };
        let text = summary
            .iter()
            .filter_map(|entry| entry["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        if text.is_empty() {
            return;
        }
        let turn = self.current_turn(created_at);
        let mut thinking = turn.thinking.take().unwrap_or_default();
        if !thinking.is_empty() {
            thinking.push_str("\n\n");
        }
        thinking.push_str(&text);
        turn.thinking = Some(thinking);
    }

    fn handle_call(&mut self, item: &Value, created_at: i64) {
        let name = item["name"]
            .as_str()
            .or_else(|| item["type"].as_str())
            .unwrap_or("инструмент")
            .to_string();
        let raw_input = item
            .get("arguments")
            .or_else(|| item.get("input"))
            .or_else(|| item.get("action"))
            .or_else(|| item.get("revised_prompt"))
            .cloned()
            .unwrap_or(Value::Null);
        let input_text = match &raw_input {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let call_id = item["call_id"].clone();
        let turn = self.current_turn(created_at);
        if let Some(parts) = turn.parts.as_array_mut() {
            parts.push(json!({
                "type": "tool_call",
                "id": call_id,
                "name": name,
                "input": truncate_with_mark(&input_text, TOOL_INPUT_LIMIT),
            }));
        }
    }

    fn handle_output(&mut self, item: &Value) {
        let call_id = &item["call_id"];
        let result = truncate_with_mark(&output_text(&item["output"]), TOOL_RESULT_LIMIT);
        for message in self.messages.iter_mut().rev() {
            let Some(parts) = message.parts.as_array_mut() else {
                continue;
            };
            for part in parts.iter_mut().rev() {
                if part["type"] == "tool_call" && &part["id"] == call_id {
                    part["result"] = json!(result);
                    return;
                }
            }
        }
    }

    fn handle_token_count(&mut self, payload: &Value) {
        let usage = &payload["info"]["last_token_usage"];
        let Some(output) = usage["output_tokens"].as_i64() else {
            return;
        };
        let input = usage["input_tokens"].as_i64();
        if let Some(message) = self.messages.iter_mut().rev().find(|message| message.role == "assistant") {
            message.input_tokens = input;
            message.output_tokens = Some(message.output_tokens.unwrap_or(0) + output);
        }
    }

    fn handle_compacted(&mut self, payload: &Value) {
        let summary = payload["message"].as_str().unwrap_or_default().trim();
        let Some(last_message) = self.messages.last() else {
            return;
        };
        if !summary.is_empty() {
            self.compactions.push(ParsedCompaction { up_to_message_id: last_message.id.clone(), summary: summary.to_string() });
        }
    }

    fn handle_session_meta(&mut self, payload: &Value) {
        if self.session_id.is_none() {
            self.session_id = payload["id"].as_str().map(String::from);
        }
        if self.cwd.is_none() {
            self.cwd = payload["cwd"].as_str().map(String::from);
        }
        if let Some(model) = payload["model"].as_str() {
            self.model = Some(model.to_string());
        }
    }

    fn handle_record(&mut self, record: &Value) {
        let created_at = parse_timestamp(record["timestamp"].as_str());
        if created_at != 0 {
            if self.first_time == 0 {
                self.first_time = created_at;
            }
            self.last_time = self.last_time.max(created_at);
        }
        let payload = &record["payload"];
        match record["type"].as_str().unwrap_or_default() {
            "session_meta" => self.handle_session_meta(payload),
            "turn_context" => {
                if let Some(model) = payload["model"].as_str() {
                    self.model = Some(model.to_string());
                }
                if self.cwd.is_none() {
                    self.cwd = payload["cwd"].as_str().map(String::from);
                }
            }
            "response_item" => self.handle_item(payload, created_at),
            "event_msg" => {
                if payload["type"] == "token_count" {
                    self.handle_token_count(payload);
                }
            }
            "compacted" => self.handle_compacted(payload),
            _ => {
                if record.get("id").is_some() && record.get("instructions").is_some() {
                    self.handle_session_meta(record);
                } else {
                    self.handle_item(record, created_at);
                }
            }
        }
    }
}

pub fn parse_lines(lines: impl Iterator<Item = String>, file_id: &str) -> ParsedChat {
    let mut parser = Parser {
        session_id: None,
        cwd: None,
        model: None,
        messages: Vec::new(),
        images: HashMap::new(),
        first_time: 0,
        last_time: 0,
        file_id: file_id.to_string(),
        ordinal: 0,
        compactions: Vec::new(),
    };
    for line in lines {
        if let Ok(record) = serde_json::from_str::<Value>(&line) {
            parser.handle_record(&record);
        }
    }
    let title = parser
        .messages
        .iter()
        .find(|message| message.role == "user" && !message.content.trim().is_empty())
        .map(|message| title_from_text(&message.content))
        .unwrap_or_else(|| "Чат Codex".to_string());
    let identifier = parser.session_id.clone().unwrap_or_else(|| file_id.to_string());
    ParsedChat {
        id: format!("cx:{identifier}"),
        title,
        cwd: parser.cwd.clone(),
        created_at: parser.first_time,
        updated_at: parser.last_time,
        leaf_id: parser.messages.last().map(|message| message.id.clone()),
        messages: parser.messages,
        images: parser.images.into_values().collect(),
        compactions: parser.compactions,
    }
}

pub fn parse_file(path: &Path) -> std::io::Result<ParsedChat> {
    let file = File::open(path)?;
    let reader = BufReader::with_capacity(256 * 1024, file);
    let file_id = path.file_stem().map(|stem| stem.to_string_lossy().to_string()).unwrap_or_default();
    let lines = reader.split(b'\n').filter_map(|line| {
        let bytes = line.ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    });
    Ok(parse_lines(lines, &file_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(source: &[&str]) -> impl Iterator<Item = String> {
        source.iter().map(|line| line.to_string()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn parses_rollout_with_payload_wrapper() {
        let chat = parse_lines(
            lines(&[
                r#"{"timestamp":"2026-02-01T10:00:00Z","type":"session_meta","payload":{"id":"sess","cwd":"I:\\dev"}}"#,
                r#"{"timestamp":"2026-02-01T10:00:00Z","type":"turn_context","payload":{"model":"gpt-5-codex"}}"#,
                r#"{"timestamp":"2026-02-01T10:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}"#,
                r#"{"timestamp":"2026-02-01T10:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Почини тест"}]}}"#,
                r#"{"timestamp":"2026-02-01T10:00:03Z","type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"cmd\":\"ls\"}","call_id":"c1"}}"#,
                r#"{"timestamp":"2026-02-01T10:00:04Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"a.txt"}}"#,
                r#"{"timestamp":"2026-02-01T10:00:05Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Готово"}]}}"#,
                r#"{"timestamp":"2026-02-01T10:00:06Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":50,"output_tokens":7}}}}"#,
            ]),
            "rollout",
        );
        assert_eq!(chat.id, "cx:sess");
        assert_eq!(chat.title, "Почини тест");
        assert_eq!(chat.cwd.as_deref(), Some("I:\\dev"));
        assert_eq!(chat.messages.len(), 2);
        let assistant = &chat.messages[1];
        assert_eq!(assistant.content, "Готово");
        assert_eq!(assistant.parts[0]["name"], "shell");
        assert_eq!(assistant.parts[0]["result"], "a.txt");
        assert_eq!(assistant.output_tokens, Some(7));
        assert_eq!(assistant.parent_id, Some(chat.messages[0].id.clone()));
    }

    #[test]
    fn compacted_record_becomes_a_summary_of_everything_before_it() {
        let chat = parse_lines(
            lines(&[
                r#"{"timestamp":"2026-02-01T10:00:00Z","type":"session_meta","payload":{"id":"sess","cwd":"I:\\dev"}}"#,
                r#"{"timestamp":"2026-02-01T10:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Старое"}]}}"#,
                r#"{"timestamp":"2026-02-01T10:00:02Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Ответ"}]}}"#,
                r#"{"timestamp":"2026-02-01T10:00:03Z","type":"compacted","payload":{"message":"Сводка старого"}}"#,
                r#"{"timestamp":"2026-02-01T10:00:04Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Новое"}]}}"#,
            ]),
            "rollout",
        );
        assert_eq!(chat.compactions.len(), 1);
        assert_eq!(chat.compactions[0].up_to_message_id, chat.messages[1].id);
        assert_eq!(chat.compactions[0].summary, "Сводка старого");
    }

    #[test]
    fn reads_output_given_as_content_items_and_tool_search() {
        let chat = parse_lines(
            lines(&[
                r#"{"timestamp":"2026-02-01T10:00:00Z","type":"session_meta","payload":{"id":"sess","cwd":"I:\\dev"}}"#,
                r#"{"timestamp":"2026-02-01T10:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Найди инструмент"}]}}"#,
                r#"{"timestamp":"2026-02-01T10:00:02Z","type":"response_item","payload":{"type":"function_call","name":"view","arguments":"{}","call_id":"c1"}}"#,
                r#"{"timestamp":"2026-02-01T10:00:03Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":[{"type":"input_text","text":"содержимое файла"}]}}"#,
                r#"{"timestamp":"2026-02-01T10:00:04Z","type":"response_item","payload":{"type":"tool_search_call","call_id":"c2","arguments":{"query":"x"}}}"#,
                r#"{"timestamp":"2026-02-01T10:00:05Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Готово"}]}}"#,
            ]),
            "rollout",
        );
        let assistant = &chat.messages[1];
        assert_eq!(assistant.parts[0]["result"], "содержимое файла");
        assert_eq!(assistant.parts[1]["name"], "tool_search_call");
    }

    #[test]
    fn parses_older_flat_format_and_ignores_garbage() {
        let chat = parse_lines(
            lines(&[
                r#"{"id":"old","timestamp":"2025-05-01T10:00:00Z","instructions":""}"#,
                r#"{"type":"message","role":"user","content":[{"type":"input_text","text":"Привет"}]}"#,
                "мусор",
                r#"{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Здравствуй"}]}"#,
            ]),
            "rollout",
        );
        assert_eq!(chat.id, "cx:old");
        assert_eq!(chat.messages.len(), 2);
    }

    #[test]
    fn empty_file_gives_no_messages() {
        let chat = parse_lines(lines(&[]), "rollout");
        assert!(chat.messages.is_empty());
        assert_eq!(chat.title, "Чат Codex");
    }
}
