use std::collections::HashMap;
use std::sync::Arc;

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::Notify;

use crate::types::{Attachment, ModelInfo};

const ANTHROPIC_VERSION: &str = "2026-10-21";
const ANTHROPIC_DEFAULT_MAX_TOKENS: i64 = 64000;

#[derive(Clone, Debug, PartialEq)]
pub enum Protocol {
    OpenAi,
    Anthropic,
}

impl Protocol {
    pub fn parse(value: &str) -> Protocol {
        if value == "anthropic" {
            Protocol::Anthropic
        } else {
            Protocol::OpenAi
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::OpenAi => "openai",
            Protocol::Anthropic => "anthropic",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub protocol: Protocol,
    pub base_url: String,
    pub requires_key: bool,
    pub is_local: bool,
    pub is_custom: bool,
}

fn builtin(id: &str, name: &str, protocol: Protocol, base_url: &str, requires_key: bool, is_local: bool) -> ProviderConfig {
    ProviderConfig {
        id: id.to_string(),
        name: name.to_string(),
        protocol,
        base_url: base_url.to_string(),
        requires_key,
        is_local,
        is_custom: false,
    }
}

pub fn builtin_providers() -> Vec<ProviderConfig> {
    vec![
        builtin("anthropic", "Anthropic", Protocol::Anthropic, "https://api.anthropic.com/v1", true, false),
        builtin("openai", "OpenAI", Protocol::OpenAi, "https://api.openai.com/v1", true, false),
        builtin("google", "Google Gemini", Protocol::OpenAi, "https://generativelanguage.googleapis.com/v1beta/openai", true, false),
        builtin("openrouter", "OpenRouter", Protocol::OpenAi, "https://openrouter.ai/api/v1", true, false),
        builtin("xai", "xAI", Protocol::OpenAi, "https://api.x.ai/v1", true, false),
        builtin("groq", "Groq", Protocol::OpenAi, "https://api.groq.com/openai/v1", true, false),
        builtin("deepseek", "DeepSeek", Protocol::OpenAi, "https://api.deepseek.com/v1", true, false),
        builtin("mistral", "Mistral", Protocol::OpenAi, "https://api.mistral.ai/v1", true, false),
        builtin("ollama", "Ollama", Protocol::OpenAi, "http://localhost:11434/v1", false, true),
        builtin("lmstudio", "LM Studio", Protocol::OpenAi, "http://localhost:1234/v1", false, true),
    ]
}

pub fn candidate_providers_for_key(key: &str) -> Vec<&'static str> {
    let trimmed = key.trim();
    if trimmed.starts_with("sk-ant-") {
        return vec!["anthropic"];
    }
    if trimmed.starts_with("sk-or-") {
        return vec!["openrouter"];
    }
    if trimmed.starts_with("AIza") || trimmed.starts_with("AQ.") {
        return vec!["google"];
    }
    if trimmed.starts_with("gsk_") {
        return vec!["groq"];
    }
    if trimmed.starts_with("xai-") {
        return vec!["xai"];
    }
    if trimmed.starts_with("sk-proj-") || trimmed.starts_with("sk-svcacct-") || trimmed.starts_with("sk-admin-") {
        return vec!["openai"];
    }
    if trimmed.starts_with("sk-") {
        return vec!["openai", "deepseek"];
    }
    if trimmed.len() >= 24 && trimmed.chars().all(|c| c.is_ascii_alphanumeric()) {
        return vec!["mistral"];
    }
    Vec::new()
}

pub fn split_model_ref(model_ref: &str) -> Option<(&str, &str)> {
    model_ref.split_once('/')
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input_json: String,
    pub provider_extra: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AssistantBlock {
    Text(String),
    Thinking { text: String, signature: Option<String> },
    RedactedThinking(String),
    Tool(ToolCall),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolOutcome {
    pub call_id: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

#[derive(Clone, Debug, Default)]
pub struct ContextMessage {
    pub role: String,
    pub text: String,
    pub images: Vec<Attachment>,
    pub blocks: Vec<AssistantBlock>,
    pub tool_outcomes: Vec<ToolOutcome>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    Text(String),
    Thinking(String),
    ThinkingSignature(String),
    RedactedThinking(String),
    ToolStart { stream_index: i64, id: String, name: String, provider_extra: Option<Value> },
    ToolInput { stream_index: i64, json: String },
    InputTokens(i64),
    OutputTokens(i64),
    Stop(String),
    Failure(String),
}

#[derive(Debug, Clone)]
pub struct ProviderError {
    pub status: Option<u16>,
    pub message: String,
}

impl ProviderError {
    pub fn is_context_overflow(&self) -> bool {
        const OVERFLOW_PHRASES: [&str; 6] = [
            "context length",
            "context_length",
            "context window",
            "prompt is too long",
            "too many tokens",
            "input is too long",
        ];
        let is_request_error = matches!(self.status, Some(400 | 413 | 422));
        let lowered = self.message.to_lowercase();
        is_request_error && (self.status == Some(413) || OVERFLOW_PHRASES.iter().any(|phrase| lowered.contains(phrase)))
    }

    fn is_request_rejected_mentioning(&self, phrases: &[&str]) -> bool {
        let lowered = self.message.to_lowercase();
        matches!(self.status, Some(400 | 404 | 422)) && phrases.iter().any(|phrase| lowered.contains(phrase))
    }

    pub fn is_effort_rejected(&self) -> bool {
        self.is_request_rejected_mentioning(&["effort", "reasoning"])
    }

    pub fn is_tools_rejected(&self) -> bool {
        self.is_request_rejected_mentioning(&["tool", "function"])
    }

    pub fn is_retryable_elsewhere(&self) -> bool {
        match self.status {
            Some(code) => code == 429 || code >= 500,
            None => true,
        }
    }
}

const REGION_BLOCK_PHRASES: [&str; 4] = [
    "request not allowed",
    "country, region",
    "unsupported_country",
    "user location is not supported",
];

pub fn describe_http_failure(status: u16, body: &str, provider_name: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value["error"]["message"]
                .as_str()
                .or_else(|| value["error"].as_str())
                .or_else(|| value["message"].as_str())
                .map(String::from)
        })
        .unwrap_or_default();
    let lowered_detail = detail.to_lowercase();
    let is_region_block = matches!(status, 400 | 403) && REGION_BLOCK_PHRASES.iter().any(|phrase| lowered_detail.contains(phrase));
    let summary = match status {
        _ if is_region_block => format!("{provider_name} не обслуживает запросы из этой сети или региона. Ключ тут ни при чём: включи VPN и попробуй снова"),
        401 | 403 => format!("{provider_name} не принял ключ. Проверь его в настройках"),
        402 => format!("На счёте {provider_name} закончились средства"),
        404 => format!("{provider_name} не нашёл эту модель. Выбери другую"),
        413 => "Запрос слишком большой для этой модели".to_string(),
        429 => format!("{provider_name} ограничил запросы. Подожди немного или выбери другую модель"),
        500..=599 => format!("{provider_name} сейчас не отвечает. Попробуй ещё раз позже"),
        _ => format!("{provider_name} вернул ошибку {status}"),
    };
    if detail.is_empty() {
        summary
    } else {
        format!("{summary}\n{detail}")
    }
}

fn parse_openai_tool_delta(tool_delta: &Value) -> Vec<ProviderEvent> {
    let stream_index = tool_delta["index"].as_i64().unwrap_or(0);
    let mut events = Vec::new();
    if let Some(name) = tool_delta["function"]["name"].as_str().filter(|name| !name.is_empty()) {
        events.push(ProviderEvent::ToolStart {
            stream_index,
            id: tool_delta["id"].as_str().unwrap_or_default().to_string(),
            name: name.to_string(),
            provider_extra: tool_delta.get("extra_content").cloned(),
        });
    }
    if let Some(json) = tool_delta["function"]["arguments"].as_str().filter(|json| !json.is_empty()) {
        events.push(ProviderEvent::ToolInput { stream_index, json: json.to_string() });
    }
    events
}

pub fn parse_openai_chunk(data: &str) -> Vec<ProviderEvent> {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };
    let mut events = Vec::new();
    if let Some(message) = value["error"]["message"].as_str() {
        events.push(ProviderEvent::Failure(message.to_string()));
        return events;
    }
    let choice = &value["choices"][0];
    let reasoning = choice["delta"]["reasoning_content"]
        .as_str()
        .or_else(|| choice["delta"]["reasoning"].as_str());
    if let Some(text) = reasoning {
        if !text.is_empty() {
            events.push(ProviderEvent::Thinking(text.to_string()));
        }
    }
    if let Some(text) = choice["delta"]["content"].as_str() {
        if !text.is_empty() {
            events.push(ProviderEvent::Text(text.to_string()));
        }
    }
    for tool_delta in choice["delta"]["tool_calls"].as_array().map(Vec::as_slice).unwrap_or_default() {
        events.extend(parse_openai_tool_delta(tool_delta));
    }
    if let Some(reason) = choice["finish_reason"].as_str() {
        events.push(ProviderEvent::Stop(reason.to_string()));
    }
    if let Some(tokens) = value["usage"]["prompt_tokens"].as_i64() {
        events.push(ProviderEvent::InputTokens(tokens));
    }
    if let Some(tokens) = value["usage"]["completion_tokens"].as_i64() {
        events.push(ProviderEvent::OutputTokens(tokens));
    }
    events
}

pub fn parse_anthropic_event(data: &str) -> Vec<ProviderEvent> {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };
    let mut events = Vec::new();
    match value["type"].as_str().unwrap_or_default() {
        "message_start" => {
            let usage = &value["message"]["usage"];
            let plain = usage["input_tokens"].as_i64().unwrap_or(0);
            let cache_read = usage["cache_read_input_tokens"].as_i64().unwrap_or(0);
            let cache_created = usage["cache_creation_input_tokens"].as_i64().unwrap_or(0);
            events.push(ProviderEvent::InputTokens(plain + cache_read + cache_created));
        }
        "content_block_start" => {
            let block = &value["content_block"];
            match block["type"].as_str().unwrap_or_default() {
                "tool_use" => events.push(ProviderEvent::ToolStart {
                    stream_index: value["index"].as_i64().unwrap_or(0),
                    id: block["id"].as_str().unwrap_or_default().to_string(),
                    name: block["name"].as_str().unwrap_or_default().to_string(),
                    provider_extra: None,
                }),
                "redacted_thinking" => {
                    if let Some(data) = block["data"].as_str() {
                        events.push(ProviderEvent::RedactedThinking(data.to_string()));
                    }
                }
                _ => {}
            }
        }
        "content_block_delta" => {
            let delta = &value["delta"];
            match delta["type"].as_str().unwrap_or_default() {
                "text_delta" => {
                    if let Some(text) = delta["text"].as_str() {
                        events.push(ProviderEvent::Text(text.to_string()));
                    }
                }
                "thinking_delta" => {
                    if let Some(text) = delta["thinking"].as_str() {
                        if !text.is_empty() {
                            events.push(ProviderEvent::Thinking(text.to_string()));
                        }
                    }
                }
                "signature_delta" => {
                    if let Some(signature) = delta["signature"].as_str() {
                        events.push(ProviderEvent::ThinkingSignature(signature.to_string()));
                    }
                }
                "input_json_delta" => {
                    if let Some(json) = delta["partial_json"].as_str().filter(|json| !json.is_empty()) {
                        events.push(ProviderEvent::ToolInput {
                            stream_index: value["index"].as_i64().unwrap_or(0),
                            json: json.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
        "message_delta" => {
            if let Some(reason) = value["delta"]["stop_reason"].as_str() {
                events.push(ProviderEvent::Stop(reason.to_string()));
            }
            if let Some(tokens) = value["usage"]["output_tokens"].as_i64() {
                events.push(ProviderEvent::OutputTokens(tokens));
            }
        }
        "error" => {
            let message = value["error"]["message"].as_str().unwrap_or("неизвестная ошибка");
            events.push(ProviderEvent::Failure(message.to_string()));
        }
        _ => {}
    }
    events
}

#[derive(Clone, Copy, Default)]
pub struct RequestExtras<'a> {
    pub effort: Option<&'a str>,
    pub tools: &'a [ToolSpec],
    pub eager_tool_input: bool,
}

#[derive(Default)]
pub struct StepAssembler {
    pub blocks: Vec<AssistantBlock>,
    tool_positions: HashMap<i64, usize>,
}

impl StepAssembler {
    pub fn accept(&mut self, event: &ProviderEvent) {
        match event {
            ProviderEvent::Text(text) => self.append_text(text),
            ProviderEvent::Thinking(text) => self.append_thinking(text),
            ProviderEvent::ThinkingSignature(signature) => self.set_signature(signature),
            ProviderEvent::RedactedThinking(data) => self.blocks.push(AssistantBlock::RedactedThinking(data.clone())),
            ProviderEvent::ToolStart { stream_index, id, name, provider_extra } => {
                self.tool_positions.insert(*stream_index, self.blocks.len());
                self.blocks.push(AssistantBlock::Tool(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    input_json: String::new(),
                    provider_extra: provider_extra.clone(),
                }));
            }
            ProviderEvent::ToolInput { stream_index, json } => self.append_tool_input(*stream_index, json),
            _ => {}
        }
    }

    fn append_text(&mut self, text: &str) {
        if let Some(AssistantBlock::Text(existing)) = self.blocks.last_mut() {
            existing.push_str(text);
        } else {
            self.blocks.push(AssistantBlock::Text(text.to_string()));
        }
    }

    fn append_thinking(&mut self, text: &str) {
        if let Some(AssistantBlock::Thinking { text: existing, signature: None }) = self.blocks.last_mut() {
            existing.push_str(text);
        } else {
            self.blocks.push(AssistantBlock::Thinking { text: text.to_string(), signature: None });
        }
    }

    fn set_signature(&mut self, new_signature: &str) {
        if let Some(AssistantBlock::Thinking { signature, .. }) = self.blocks.last_mut() {
            *signature = Some(new_signature.to_string());
        } else {
            self.blocks.push(AssistantBlock::Thinking { text: String::new(), signature: Some(new_signature.to_string()) });
        }
    }

    fn append_tool_input(&mut self, stream_index: i64, json: &str) {
        let Some(position) = self.tool_positions.get(&stream_index) else {
            return;
        };
        if let Some(AssistantBlock::Tool(call)) = self.blocks.get_mut(*position) {
            call.input_json.push_str(json);
        }
    }

    pub fn tool_calls(&self) -> Vec<ToolCall> {
        self.blocks
            .iter()
            .filter_map(|block| match block {
                AssistantBlock::Tool(call) => Some(call.clone()),
                _ => None,
            })
            .collect()
    }
}

fn openai_user_message(message: &ContextMessage) -> Value {
    if message.images.is_empty() {
        return json!({ "role": message.role, "content": message.text });
    }
    let mut content = Vec::new();
    if !message.text.is_empty() {
        content.push(json!({ "type": "text", "text": message.text }));
    }
    for image in &message.images {
        let data = image.data_base64.as_deref().unwrap_or_default();
        content.push(json!({
            "type": "image_url",
            "image_url": { "url": format!("data:{};base64,{}", image.mime, data) }
        }));
    }
    json!({ "role": message.role, "content": content })
}

fn openai_tool_call(call: &ToolCall) -> Value {
    let arguments = if call.input_json.is_empty() { "{}" } else { call.input_json.as_str() };
    let mut wire = json!({
        "id": call.id,
        "type": "function",
        "function": { "name": call.name, "arguments": arguments }
    });
    if let Some(extra) = &call.provider_extra {
        wire["extra_content"] = extra.clone();
    }
    wire
}

fn openai_assistant_step(provider_id: &str, message: &ContextMessage) -> Value {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls = Vec::new();
    for block in &message.blocks {
        match block {
            AssistantBlock::Text(part) => text.push_str(part),
            AssistantBlock::Thinking { text: part, .. } => reasoning.push_str(part),
            AssistantBlock::Tool(call) => calls.push(openai_tool_call(call)),
            AssistantBlock::RedactedThinking(_) => {}
        }
    }
    let mut wire = json!({ "role": "assistant", "content": if text.is_empty() { Value::Null } else { json!(text) } });
    if !calls.is_empty() {
        wire["tool_calls"] = json!(calls);
    }
    if provider_id == "deepseek" && !reasoning.is_empty() {
        wire["reasoning_content"] = json!(reasoning);
    }
    wire
}

fn openai_tools(tools: &[ToolSpec]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": { "name": tool.name, "description": tool.description, "parameters": tool.schema }
            })
        })
        .collect()
}

pub fn openai_body(provider_id: &str, model: &str, system: Option<&str>, messages: &[ContextMessage], include_usage: bool, extras: RequestExtras) -> Value {
    let mut wire_messages = Vec::new();
    if let Some(prompt) = system {
        wire_messages.push(json!({ "role": "system", "content": prompt }));
    }
    for message in messages {
        if !message.blocks.is_empty() {
            wire_messages.push(openai_assistant_step(provider_id, message));
        } else if !message.tool_outcomes.is_empty() {
            for outcome in &message.tool_outcomes {
                wire_messages.push(json!({ "role": "tool", "tool_call_id": outcome.call_id, "content": outcome.content }));
            }
        } else {
            wire_messages.push(openai_user_message(message));
        }
    }
    let mut body = json!({ "model": model, "stream": true, "messages": wire_messages });
    if include_usage {
        body["stream_options"] = json!({ "include_usage": true });
    }
    if let Some(effort) = extras.effort {
        if provider_id == "openrouter" {
            body["reasoning"] = json!({ "effort": effort });
        } else {
            body["reasoning_effort"] = json!(effort);
        }
    }
    if !extras.tools.is_empty() {
        body["tools"] = json!(openai_tools(extras.tools));
    }
    body
}

fn anthropic_block(block: &AssistantBlock) -> Option<Value> {
    match block {
        AssistantBlock::Text(text) => Some(json!({ "type": "text", "text": text })),
        AssistantBlock::Thinking { text, signature: Some(signature) } => Some(json!({ "type": "thinking", "thinking": text, "signature": signature })),
        AssistantBlock::Thinking { signature: None, .. } => None,
        AssistantBlock::RedactedThinking(data) => Some(json!({ "type": "redacted_thinking", "data": data })),
        AssistantBlock::Tool(call) => {
            let input: Value = serde_json::from_str(&call.input_json).unwrap_or_else(|_| json!({}));
            Some(json!({ "type": "tool_use", "id": call.id, "name": call.name, "input": input }))
        }
    }
}

fn anthropic_message_content(message: &ContextMessage) -> Vec<Value> {
    if !message.blocks.is_empty() {
        return message.blocks.iter().filter_map(anthropic_block).collect();
    }
    if !message.tool_outcomes.is_empty() {
        return message
            .tool_outcomes
            .iter()
            .map(|outcome| json!({ "type": "tool_result", "tool_use_id": outcome.call_id, "content": outcome.content, "is_error": outcome.is_error }))
            .collect();
    }
    let mut content = Vec::new();
    for image in &message.images {
        content.push(json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": image.mime,
                "data": image.data_base64.as_deref().unwrap_or_default()
            }
        }));
    }
    if !message.text.is_empty() {
        content.push(json!({ "type": "text", "text": message.text }));
    }
    content
}

fn anthropic_tool(tool: &ToolSpec, eager_input: bool) -> Value {
    let mut wire = json!({ "name": tool.name, "description": tool.description, "input_schema": tool.schema });
    if eager_input {
        wire["eager_input_streaming"] = json!(true);
    }
    wire
}

pub fn anthropic_body(model: &str, system: Option<&str>, messages: &[ContextMessage], max_output: Option<i64>, extras: RequestExtras) -> Value {
    let wire_messages: Vec<Value> = messages
        .iter()
        .map(|message| json!({ "role": message.role, "content": anthropic_message_content(message) }))
        .collect();
    let max_tokens = max_output
        .map(|limit| limit.min(ANTHROPIC_DEFAULT_MAX_TOKENS))
        .unwrap_or(ANTHROPIC_DEFAULT_MAX_TOKENS);
    let mut body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "stream": true,
        "messages": wire_messages
    });
    if let Some(prompt) = system {
        body["system"] = json!(prompt);
    }
    if let Some(effort) = extras.effort {
        body["output_config"] = json!({ "effort": effort });
    }
    if !extras.tools.is_empty() {
        let tools: Vec<Value> = extras.tools.iter().map(|tool| anthropic_tool(tool, extras.eager_tool_input)).collect();
        body["tools"] = json!(tools);
    }
    body
}

fn authorize(request: reqwest::RequestBuilder, config: &ProviderConfig, key: Option<&str>) -> reqwest::RequestBuilder {
    match config.protocol {
        Protocol::Anthropic => {
            let with_version = request.header("anthropic-version", ANTHROPIC_VERSION);
            match key {
                Some(value) => with_version.header("x-api-key", value),
                None => with_version,
            }
        }
        Protocol::OpenAi => match key {
            Some(value) => request.bearer_auth(value),
            None => request,
        },
    }
}

fn endpoint(config: &ProviderConfig, path: &str) -> String {
    format!("{}{}", config.base_url.trim_end_matches('/'), path)
}

pub fn host_of(url: &str) -> String {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    without_scheme.split('/').next().unwrap_or(without_scheme).to_string()
}

pub struct StreamRequest<'a> {
    pub config: &'a ProviderConfig,
    pub key: Option<&'a str>,
    pub model: &'a str,
    pub system: Option<&'a str>,
    pub messages: &'a [ContextMessage],
    pub max_output: Option<i64>,
    pub extras: RequestExtras<'a>,
}

#[derive(Default, Debug)]
pub struct StreamStats {
    pub bytes_sent: i64,
    pub bytes_received: i64,
    pub cancelled: bool,
}

pub async fn stream_completion(
    http: &reqwest::Client,
    request: StreamRequest<'_>,
    cancel: Arc<Notify>,
    stats: &mut StreamStats,
    mut on_event: impl FnMut(ProviderEvent),
) -> Result<(), ProviderError> {
    let config = request.config;
    let (path, first_body) = match config.protocol {
        Protocol::Anthropic => (
            "/messages",
            anthropic_body(request.model, request.system, request.messages, request.max_output, request.extras),
        ),
        Protocol::OpenAi => (
            "/chat/completions",
            openai_body(&config.id, request.model, request.system, request.messages, true, request.extras),
        ),
    };
    let url = endpoint(config, path);
    let mut response = send_json(http, &url, config, request.key, &first_body, stats).await?;

    if response.status().as_u16() == 400 && config.protocol == Protocol::OpenAi {
        let text = response.text().await.unwrap_or_default();
        if !text.contains("stream_options") {
            return Err(ProviderError {
                status: Some(400),
                message: describe_http_failure(400, &text, &config.name),
            });
        }
        let retry_body = openai_body(&config.id, request.model, request.system, request.messages, false, request.extras);
        response = send_json(http, &url, config, request.key, &retry_body, stats).await?;
    }

    let status = response.status().as_u16();
    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        stats.bytes_received += text.len() as i64;
        return Err(ProviderError {
            status: Some(status),
            message: describe_http_failure(status, &text, &config.name),
        });
    }

    let mut stream = response.bytes_stream();
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let next = tokio::select! {
            _ = cancel.notified() => {
                stats.cancelled = true;
                return Ok(());
            }
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = next else {
            break;
        };
        let bytes = chunk.map_err(|error| ProviderError {
            status: None,
            message: format!("Соединение с {} оборвалось: {}", config.name, full_error_text(&error)),
        })?;
        stats.bytes_received += bytes.len() as i64;
        pending.extend_from_slice(&bytes);
        while let Some(line_end) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=line_end).collect();
            let Ok(text) = std::str::from_utf8(&line) else {
                continue;
            };
            let Some(data) = text.trim().strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                continue;
            }
            let events = match config.protocol {
                Protocol::Anthropic => parse_anthropic_event(data),
                Protocol::OpenAi => parse_openai_chunk(data),
            };
            for event in events {
                if let ProviderEvent::Failure(message) = &event {
                    return Err(ProviderError {
                        status: None,
                        message: format!("{}: {message}", config.name),
                    });
                }
                on_event(event);
            }
        }
    }
    Ok(())
}

async fn send_json(
    http: &reqwest::Client,
    url: &str,
    config: &ProviderConfig,
    key: Option<&str>,
    body: &Value,
    stats: &mut StreamStats,
) -> Result<reqwest::Response, ProviderError> {
    let payload = serde_json::to_vec(body).map_err(|error| ProviderError {
        status: Some(400),
        message: error.to_string(),
    })?;
    stats.bytes_sent += payload.len() as i64;
    let request = http.post(url).header("content-type", "application/json").body(payload);
    authorize(request, config, key).send().await.map_err(|error| ProviderError {
        status: None,
        message: describe_connection_failure(config, &error),
    })
}

fn full_error_text(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

fn describe_connection_failure(config: &ProviderConfig, error: &reqwest::Error) -> String {
    if error.is_timeout() {
        return format!("{} не ответил вовремя. Проверь соединение и повтори", config.name);
    }
    if config.is_local && error.is_connect() {
        return format!("{} не запущен на этом ПК. Запусти его и повтори", config.name);
    }
    if error.is_connect() {
        return format!("Нет соединения с {}. Проверь интернет и повтори", config.name);
    }
    format!("Запрос к {} не прошёл: {}", config.name, full_error_text(error))
}

const NON_CHAT_MODEL_MARKERS: [&str; 12] = [
    "embedding", "embed", "whisper", "tts", "dall-e", "moderation", "transcribe", "image", "audio", "realtime", "sora", "aqa",
];

fn is_chat_model(id: &str) -> bool {
    let lowered = id.to_lowercase();
    !NON_CHAT_MODEL_MARKERS.iter().any(|marker| lowered.contains(marker))
}

const ANTHROPIC_EFFORT_ORDER: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
const REASONING_EFFORT_LEVELS: [&str; 3] = ["low", "medium", "high"];

fn anthropic_effort_levels(entry: &Value) -> Vec<String> {
    let effort = &entry["capabilities"]["effort"];
    if effort["supported"] != true {
        return Vec::new();
    }
    ANTHROPIC_EFFORT_ORDER
        .iter()
        .filter(|level| effort[**level]["supported"] == true)
        .map(|level| level.to_string())
        .collect()
}

fn has_reasoning_effort(provider_id: &str, model_id: &str, entry: &Value) -> bool {
    let lowered = model_id.to_lowercase();
    match provider_id {
        "openai" => ["o1", "o3", "o4", "gpt-5"].iter().any(|prefix| lowered.starts_with(prefix)),
        "google" => lowered.contains("gemini-2.5") || lowered.contains("gemini-3"),
        "openrouter" => entry["supported_parameters"]
            .as_array()
            .is_some_and(|parameters| parameters.iter().any(|parameter| parameter == "reasoning")),
        _ => false,
    }
}

fn effort_levels_of(config: &ProviderConfig, model_id: &str, entry: &Value) -> Vec<String> {
    if config.protocol == Protocol::Anthropic {
        return anthropic_effort_levels(entry);
    }
    if has_reasoning_effort(&config.id, model_id, entry) {
        return REASONING_EFFORT_LEVELS.iter().map(|level| level.to_string()).collect();
    }
    Vec::new()
}

pub struct FetchedModels {
    pub models: Vec<ModelInfo>,
    pub prices: Vec<(String, f64, f64)>,
}

pub fn parse_models(config: &ProviderConfig, body: &Value) -> FetchedModels {
    let mut models = Vec::new();
    let mut prices = Vec::new();
    let entries = body["data"].as_array().cloned().unwrap_or_default();
    for entry in entries {
        let Some(raw_id) = entry["id"].as_str() else {
            continue;
        };
        let id = raw_id.strip_prefix("models/").unwrap_or(raw_id).to_string();
        if !is_chat_model(&id) {
            continue;
        }
        let name = entry["display_name"]
            .as_str()
            .or_else(|| entry["name"].as_str())
            .unwrap_or(&id)
            .to_string();
        let effort_levels = effort_levels_of(config, &id, &entry);
        models.push(ModelInfo {
            effort_levels,
            provider: config.id.clone(),
            id: id.clone(),
            name,
            max_output: entry["max_tokens"].as_i64(),
            max_input: entry["max_input_tokens"].as_i64().or_else(|| entry["context_length"].as_i64()),
        });
        let prompt_price = entry["pricing"]["prompt"].as_str().and_then(|text| text.parse::<f64>().ok());
        let completion_price = entry["pricing"]["completion"].as_str().and_then(|text| text.parse::<f64>().ok());
        if let (Some(input), Some(output)) = (prompt_price, completion_price) {
            prices.push((id, input * 1_000_000.0, output * 1_000_000.0));
        }
    }
    models.sort_by(|left, right| right.id.cmp(&left.id));
    FetchedModels { models, prices }
}

pub async fn fetch_models(http: &reqwest::Client, config: &ProviderConfig, key: Option<&str>) -> Result<FetchedModels, ProviderError> {
    let path = match config.protocol {
        Protocol::Anthropic => "/models?limit=1000",
        Protocol::OpenAi => "/models",
    };
    let request = http.get(endpoint(config, path)).timeout(std::time::Duration::from_secs(15));
    let response = authorize(request, config, key).send().await.map_err(|error| ProviderError {
        status: None,
        message: describe_connection_failure(config, &error),
    })?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(ProviderError {
            status: Some(status),
            message: describe_http_failure(status, &text, &config.name),
        });
    }
    let body: Value = serde_json::from_str(&text).map_err(|_| ProviderError {
        status: Some(status),
        message: format!("{} вернул непонятный список моделей", config.name),
    })?;
    Ok(parse_models(config, &body))
}

pub fn known_prices() -> Vec<(&'static str, f64, f64)> {
    vec![
        ("claude-fable-5-1", 10.0, 50.0),
        ("claude-fable-5", 10.0, 50.0),
        ("claude-opus-5-5", 4.0, 20.0),
        ("claude-opus-5", 5.0, 25.0),
        ("claude-opus-4-8", 5.0, 25.0),
        ("claude-opus-4-7", 5.0, 25.0),
        ("claude-opus-4-6", 5.0, 25.0),
        ("claude-sonnet-5-5", 2.0, 10.0),
        ("claude-sonnet-5", 2.0, 10.0),
        ("claude-opus-4-5", 5.0, 25.0),
        ("claude-sonnet-4-6", 3.0, 15.0),
        ("claude-sonnet-4-5", 3.0, 15.0),
        ("claude-haiku-5-5", 0.1, 0.5),
        ("claude-haiku-4-5", 1.0, 5.0),
    ]
}

pub fn model_without_date_suffix(model: &str) -> &str {
    match model.rsplit_once('-') {
        Some((base, suffix)) if suffix.len() == 8 && suffix.chars().all(|symbol| symbol.is_ascii_digit()) => base,
        _ => model,
    }
}

pub fn long_prompt_price_multiplier(model: &str, input_tokens: i64) -> f64 {
    const LONG_PROMPT_TOKENS: i64 = 100_000;
    const HAIKU_5_5_LONG_PROMPT_MULTIPLIER: f64 = 5.0;
    if model_without_date_suffix(model) == "claude-haiku-5-5" && input_tokens > LONG_PROMPT_TOKENS {
        HAIKU_5_5_LONG_PROMPT_MULTIPLIER
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_block_is_not_reported_as_a_bad_key() {
        let blocked = describe_http_failure(403, r#"{"error":{"type":"forbidden","message":"Request not allowed"}}"#, "Anthropic");
        assert!(blocked.contains("включи VPN"));
        assert!(!blocked.contains("не принял ключ"));
        let bad_key = describe_http_failure(401, r#"{"error":{"message":"invalid x-api-key"}}"#, "Anthropic");
        assert!(bad_key.contains("не принял ключ"));
    }

    #[test]
    fn date_suffix_is_removed_only_when_it_is_a_date() {
        assert_eq!(model_without_date_suffix("claude-haiku-4-5-20251001"), "claude-haiku-4-5");
        assert_eq!(model_without_date_suffix("claude-haiku-5-5"), "claude-haiku-5-5");
        assert_eq!(model_without_date_suffix("gpt-4o-mini"), "gpt-4o-mini");
    }

    #[test]
    fn known_prices_cover_all_listed_anthropic_families() {
        let prices = known_prices();
        for model in ["claude-sonnet-4-5-20250929", "claude-opus-4-5-20251101", "claude-haiku-4-5-20251001"] {
            let base = model_without_date_suffix(model);
            assert!(prices.iter().any(|(name, _, _)| *name == base), "нет цены для {model}");
        }
    }

    #[test]
    fn haiku_5_5_costs_more_only_above_the_long_prompt_limit() {
        assert_eq!(long_prompt_price_multiplier("claude-haiku-5-5", 100_000), 1.0);
        assert_eq!(long_prompt_price_multiplier("claude-haiku-5-5", 100_001), 5.0);
        assert_eq!(long_prompt_price_multiplier("claude-sonnet-5-5", 500_000), 1.0);
    }

    fn user_message(text: String) -> Vec<ContextMessage> {
        vec![ContextMessage { role: "user".to_string(), text, ..Default::default() }]
    }

    async fn collect_stream(
        http: &reqwest::Client,
        config: &ProviderConfig,
        key: &str,
        model: &ModelInfo,
        messages: &[ContextMessage],
    ) -> (Result<(), ProviderError>, Vec<ProviderEvent>) {
        let mut events = Vec::new();
        let mut stats = StreamStats::default();
        let request = StreamRequest { config, key: Some(key), model: &model.id, system: None, messages, max_output: model.max_output, extras: RequestExtras::default() };
        let result = stream_completion(http, request, Arc::new(Notify::new()), &mut stats, |event| events.push(event)).await;
        (result, events)
    }

    #[test]
    #[ignore = "нужен настоящий ключ в TARN_LIVE_ANTHROPIC_KEY"]
    fn live_anthropic_haiku() {
        let key = std::env::var("TARN_LIVE_ANTHROPIC_KEY").expect("нет ключа в окружении");
        tauri::async_runtime::block_on(async {
            let http = crate::network::HttpClient::new().expect("клиент").current();
            let config = builtin_providers().into_iter().find(|item| item.id == "anthropic").expect("нет провайдера");

            let fetched = fetch_models(&http, &config, Some(&key)).await.map_err(|error| error.message).expect("список моделей");
            for model in &fetched.models {
                println!("модель {} | {} | вход {:?} | выход {:?}", model.id, model.name, model.max_input, model.max_output);
            }
            let haiku = fetched.models.iter().find(|model| model.id.contains("haiku")).expect("нет Haiku в списке").clone();
            println!("выбрана {}", haiku.id);

            let question = user_message("Ответь одним словом: столица Франции?".to_string());
            for attempt in 1..=3 {
                let started = std::time::Instant::now();
                let mut first_text_at = None;
                let mut stats = StreamStats::default();
                let request = StreamRequest { config: &config, key: Some(&key), model: &haiku.id, system: None, messages: &question, max_output: haiku.max_output, extras: RequestExtras::default() };
                let timed = stream_completion(&http, request, Arc::new(Notify::new()), &mut stats, |event| {
                    if matches!(event, ProviderEvent::Text(_)) && first_text_at.is_none() {
                        first_text_at = Some(started.elapsed());
                    }
                })
                .await;
                println!("замер {attempt}: первый текст {first_text_at:?}, всего {:?}, успех {}", started.elapsed(), timed.is_ok());
            }
            let (result, events) = collect_stream(&http, &config, &key, &haiku, &question).await;
            println!("события: {events:?}");
            result.map_err(|error| error.message).expect("потоковый ответ");
            assert!(events.iter().any(|event| matches!(event, ProviderEvent::Text(_))));
            assert!(events.iter().any(|event| matches!(event, ProviderEvent::Stop(_))));
            assert!(events.iter().any(|event| matches!(event, ProviderEvent::OutputTokens(_))));

            let long_question = user_message("Напиши подряд числа от 1 до 60 словами, каждое с новой строки.".to_string());
            let (long_result, long_events) = collect_stream(&http, &config, &key, &haiku, &long_question).await;
            long_result.map_err(|error| error.message).expect("длинный ответ");
            let text_chunks = long_events.iter().filter(|event| matches!(event, ProviderEvent::Text(_))).count();
            println!("длинный ответ пришёл частями: {text_chunks}");
            assert!(text_chunks > 1);

            let (wrong_key_result, _) = collect_stream(&http, &config, "sk-ant-wrong-key-for-test", &haiku, &question).await;
            let wrong_key_error = wrong_key_result.expect_err("неверный ключ должен быть отклонён");
            println!("неверный ключ: {:?} | {}", wrong_key_error.status, wrong_key_error.message);
            assert_eq!(wrong_key_error.status, Some(401));

            if let Some(max_input) = haiku.max_input {
                let oversized = user_message("word ".repeat(max_input as usize * 3 / 2));
                let (overflow_result, _) = collect_stream(&http, &config, &key, &haiku, &oversized).await;
                let overflow_error = overflow_result.expect_err("слишком большой запрос должен быть отклонён");
                println!("переполнение: {:?} | {}", overflow_error.status, overflow_error.message);
                assert!(overflow_error.is_context_overflow());
            } else {
                println!("max_input неизвестен, переполнение не проверено");
            }
        });
    }

    #[test]
    fn detects_provider_from_key_prefix() {
        assert_eq!(candidate_providers_for_key("sk-ant-api03-abc"), vec!["anthropic"]);
        assert_eq!(candidate_providers_for_key("sk-or-v1-abc"), vec!["openrouter"]);
        assert_eq!(candidate_providers_for_key("AIzaSyAbc"), vec!["google"]);
        assert_eq!(candidate_providers_for_key("gsk_abc"), vec!["groq"]);
        assert_eq!(candidate_providers_for_key("xai-abc"), vec!["xai"]);
        assert_eq!(candidate_providers_for_key("sk-proj-abc"), vec!["openai"]);
        assert_eq!(candidate_providers_for_key("sk-abcdef"), vec!["openai", "deepseek"]);
        assert_eq!(candidate_providers_for_key("короткий"), Vec::<&str>::new());
    }

    #[test]
    fn parses_openai_stream_chunk() {
        let events = parse_openai_chunk(r#"{"choices":[{"delta":{"content":"Привет"},"finish_reason":null}]}"#);
        assert_eq!(events, vec![ProviderEvent::Text("Привет".to_string())]);
        let usage = parse_openai_chunk(r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":3}}"#);
        assert_eq!(usage, vec![ProviderEvent::InputTokens(10), ProviderEvent::OutputTokens(3)]);
    }

    #[test]
    fn parses_anthropic_stream_events() {
        let start = parse_anthropic_event(r#"{"type":"message_start","message":{"usage":{"input_tokens":5,"cache_read_input_tokens":7}}}"#);
        assert_eq!(start, vec![ProviderEvent::InputTokens(12)]);
        let delta = parse_anthropic_event(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Да"}}"#);
        assert_eq!(delta, vec![ProviderEvent::Text("Да".to_string())]);
        let stop = parse_anthropic_event(r#"{"type":"message_delta","delta":{"stop_reason":"refusal"},"usage":{"output_tokens":2}}"#);
        assert_eq!(stop, vec![ProviderEvent::Stop("refusal".to_string()), ProviderEvent::OutputTokens(2)]);
    }

    #[test]
    fn anthropic_body_has_no_sampling_parameters() {
        let messages = vec![ContextMessage { role: "user".into(), text: "Привет".into(), ..Default::default() }];
        let body = anthropic_body("claude-opus-5-5", None, &messages, Some(128000), RequestExtras::default());
        assert_eq!(body["max_tokens"], 64000);
        assert!(body.get("temperature").is_none());
        assert_eq!(body["messages"][0]["content"][0]["text"], "Привет");
    }

    #[test]
    fn image_without_text_sends_no_empty_text_block() {
        let picture = Attachment {
            name: "a".into(),
            mime: "image/png".into(),
            kind: "image".into(),
            size: 1,
            text: None,
            data_base64: Some("aGVsbG8=".into()),
            image_id: None,
        };
        let messages = vec![ContextMessage { role: "user".into(), images: vec![picture], ..Default::default() }];
        let anthropic = anthropic_body("m", None, &messages, None, RequestExtras::default());
        assert_eq!(anthropic["messages"][0]["content"].as_array().unwrap().len(), 1);
        assert_eq!(anthropic["messages"][0]["content"][0]["type"], "image");
        let openai = openai_body("openai", "m", None, &messages, false, RequestExtras::default());
        assert_eq!(openai["messages"][0]["content"].as_array().unwrap().len(), 1);
        assert_eq!(openai["messages"][0]["content"][0]["type"], "image_url");
    }

    #[test]
    fn effort_goes_where_each_provider_expects_it() {
        let messages = user_message("Привет".to_string());
        let extras = RequestExtras { effort: Some("high"), ..Default::default() };
        assert_eq!(anthropic_body("m", None, &messages, None, extras)["output_config"]["effort"], "high");
        assert_eq!(openai_body("openai", "m", None, &messages, false, extras)["reasoning_effort"], "high");
        assert_eq!(openai_body("openrouter", "m", None, &messages, false, extras)["reasoning"]["effort"], "high");
        assert!(anthropic_body("m", None, &messages, None, RequestExtras::default()).get("output_config").is_none());
    }

    #[test]
    fn effort_levels_come_from_model_capabilities() {
        let anthropic = builtin("anthropic", "Anthropic", Protocol::Anthropic, "https://x", true, false);
        let body = json!({ "data": [
            { "id": "new", "capabilities": { "effort": { "supported": true, "low": { "supported": true }, "xhigh": { "supported": false }, "max": { "supported": true } } } },
            { "id": "old", "capabilities": { "effort": { "supported": false } } },
        ] });
        let parsed = parse_models(&anthropic, &body);
        let levels_of = |id: &str| parsed.models.iter().find(|model| model.id == id).unwrap().effort_levels.clone();
        assert_eq!(levels_of("new"), vec!["low", "max"]);
        assert!(levels_of("old").is_empty());
        let openai = builtin("openai", "OpenAI", Protocol::OpenAi, "https://x", true, false);
        let listed = parse_models(&openai, &json!({ "data": [{ "id": "gpt-5" }, { "id": "gpt-4o" }] }));
        assert_eq!(listed.models.iter().find(|model| model.id == "gpt-5").unwrap().effort_levels.len(), 3);
        assert!(listed.models.iter().find(|model| model.id == "gpt-4o").unwrap().effort_levels.is_empty());
    }

    #[test]
    fn assembles_anthropic_tool_call_with_signed_thinking() {
        let mut step = StepAssembler::default();
        let lines = [
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Думаю"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"read_file"}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"a\"}"}}"#,
        ];
        for line in lines {
            for event in parse_anthropic_event(line) {
                step.accept(&event);
            }
        }
        assert_eq!(step.tool_calls()[0].input_json, r#"{"path":"a"}"#);
        let messages = vec![
            ContextMessage { role: "assistant".into(), blocks: step.blocks.clone(), ..Default::default() },
            ContextMessage {
                role: "user".into(),
                tool_outcomes: vec![ToolOutcome { call_id: "t1".into(), content: "готово".into(), is_error: false }],
                ..Default::default()
            },
        ];
        let body = anthropic_body("m", None, &messages, None, RequestExtras::default());
        assert_eq!(body["messages"][0]["content"][0]["signature"], "sig");
        assert_eq!(body["messages"][0]["content"][1]["input"]["path"], "a");
        assert_eq!(body["messages"][1]["content"][0]["tool_use_id"], "t1");
    }

    #[test]
    fn assembles_openai_tool_calls_and_sends_results_back() {
        let mut step = StepAssembler::default();
        let lines = [
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"read_file","arguments":""},"extra_content":{"google":{"thought_signature":"x"}}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":\"a\"}"}}]}}]}"#,
        ];
        for line in lines {
            for event in parse_openai_chunk(line) {
                step.accept(&event);
            }
        }
        let messages = vec![
            ContextMessage { role: "assistant".into(), blocks: step.blocks.clone(), ..Default::default() },
            ContextMessage {
                role: "user".into(),
                tool_outcomes: vec![ToolOutcome { call_id: "c1".into(), content: "готово".into(), is_error: false }],
                ..Default::default()
            },
        ];
        let body = openai_body("google", "m", None, &messages, false, RequestExtras::default());
        let call = &body["messages"][0]["tool_calls"][0];
        assert_eq!(call["function"]["arguments"], r#"{"path":"a"}"#);
        assert_eq!(call["extra_content"]["google"]["thought_signature"], "x");
        assert_eq!(body["messages"][1]["role"], "tool");
        assert_eq!(body["messages"][1]["tool_call_id"], "c1");
    }

    #[test]
    fn filters_non_chat_models_and_strips_google_prefix() {
        let config = builtin("google", "Google", Protocol::OpenAi, "https://x", true, false);
        let body = json!({ "data": [{ "id": "models/gemini-pro" }, { "id": "text-embedding-3" }] });
        let parsed = parse_models(&config, &body);
        assert_eq!(parsed.models.len(), 1);
        assert_eq!(parsed.models[0].id, "gemini-pro");
    }

    #[test]
    fn extracts_host_without_path() {
        assert_eq!(host_of("https://api.openai.com/v1/chat"), "api.openai.com");
        assert_eq!(host_of("http://localhost:11434/v1"), "localhost:11434");
    }
}
