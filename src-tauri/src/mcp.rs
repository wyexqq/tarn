use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, Notify};

use crate::skills::McpServerConfig;
use crate::tools::hide_console_window;

const PROTOCOL_VERSION: &str = "2025-06-18";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_TOOL_NAME_LENGTH: usize = 64;
const MAX_RESULT_CHARS: usize = 60_000;
const SESSION_HEADER: &str = "mcp-session-id";

#[derive(Clone, Debug, PartialEq)]
pub struct McpTool {
    pub public_name: String,
    pub server: String,
    pub tool: String,
    pub description: String,
    pub schema: Value,
    pub read_only: bool,
}

enum Transport {
    Stdio {
        _process: Child,
        input: ChildStdin,
        output: BufReader<ChildStdout>,
    },
    Http {
        client: reqwest::Client,
        url: String,
        session_id: Option<String>,
    },
}

struct Connection {
    transport: Transport,
    next_id: i64,
}

fn reply_for_id(line: &str, id: i64) -> Option<Value> {
    let parsed: Value = serde_json::from_str(line.trim()).ok()?;
    (parsed["id"].as_i64() == Some(id)).then_some(parsed)
}

fn result_of_reply(reply: Value) -> Result<Value, String> {
    if let Some(message) = reply["error"]["message"].as_str() {
        return Err(message.to_string());
    }
    Ok(reply["result"].clone())
}

async fn read_http_reply(response: reqwest::Response, id: i64) -> Result<Value, String> {
    let is_event_stream = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("event-stream"));
    if !is_event_stream {
        let text = response.text().await.map_err(|error| error.to_string())?;
        return reply_for_id(&text, id).ok_or_else(|| "Сервер ответил непонятно".to_string());
    }
    let mut stream = response.bytes_stream();
    let mut pending: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        pending.extend_from_slice(&chunk.map_err(|error| error.to_string())?);
        while let Some(line_end) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=line_end).collect();
            let text = String::from_utf8_lossy(&line);
            if let Some(reply) = text.trim().strip_prefix("data:").and_then(|data| reply_for_id(data, id)) {
                return Ok(reply);
            }
        }
    }
    Err("Сервер закрыл соединение без ответа".to_string())
}

impl Connection {
    async fn send_line(input: &mut ChildStdin, message: &Value) -> Result<(), String> {
        let mut line = message.to_string();
        line.push('\n');
        input.write_all(line.as_bytes()).await.map_err(|error| format!("Сервер не принимает данные: {error}"))?;
        input.flush().await.map_err(|error| format!("Сервер не принимает данные: {error}"))
    }

    async fn post(client: &reqwest::Client, url: &str, session_id: &Option<String>, message: &Value) -> Result<reqwest::Response, String> {
        let mut request = client
            .post(url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", PROTOCOL_VERSION)
            .body(message.to_string());
        if let Some(session) = session_id {
            request = request.header(SESSION_HEADER, session);
        }
        let response = request.send().await.map_err(|error| format!("Нет связи с сервером: {error}"))?;
        match response.status().as_u16() {
            401 | 403 => Err("Серверу нужен вход в аккаунт. Tarn пока не умеет входить в такие серверы".to_string()),
            status if status >= 400 => Err(format!("Сервер вернул ошибку {status}")),
            _ => Ok(response),
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        match &mut self.transport {
            Transport::Stdio { input, output, .. } => {
                Self::send_line(input, &message).await?;
                loop {
                    let mut line = String::new();
                    let count = output.read_line(&mut line).await.map_err(|error| format!("Сервер перестал отвечать: {error}"))?;
                    if count == 0 {
                        return Err("Сервер завершил работу".to_string());
                    }
                    if let Some(reply) = reply_for_id(&line, id) {
                        return result_of_reply(reply);
                    }
                }
            }
            Transport::Http { client, url, session_id } => {
                let response = Self::post(client, url, session_id, &message).await?;
                if let Some(session) = response.headers().get(SESSION_HEADER).and_then(|value| value.to_str().ok()) {
                    *session_id = Some(session.to_string());
                }
                result_of_reply(read_http_reply(response, id).await?)
            }
        }
    }

    async fn notify(&mut self, method: &str) -> Result<(), String> {
        let message = json!({ "jsonrpc": "2.0", "method": method });
        match &mut self.transport {
            Transport::Stdio { input, .. } => Self::send_line(input, &message).await,
            Transport::Http { client, url, session_id } => Self::post(client, url, session_id, &message).await.map(|_| ()),
        }
    }
}

fn spawn_stdio_process(command_name: &str, config: &McpServerConfig) -> std::io::Result<Child> {
    let prepare = |command: &mut Command| {
        command
            .envs(&config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        hide_console_window(command);
    };
    let mut direct = Command::new(command_name);
    direct.args(&config.args);
    prepare(&mut direct);
    match direct.spawn() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut through_shell = Command::new("cmd");
            through_shell.arg("/C").arg(command_name).args(&config.args);
            prepare(&mut through_shell);
            through_shell.spawn()
        }
        other => other,
    }
}

fn open_transport(config: &McpServerConfig, http: reqwest::Client) -> Result<Transport, String> {
    if let Some(url) = &config.url {
        return Ok(Transport::Http { client: http, url: url.clone(), session_id: None });
    }
    let command_name = config.command.as_deref().ok_or("У сервера не указана команда")?;
    let mut process = spawn_stdio_process(command_name, config).map_err(|error| format!("Не удалось запустить {command_name}: {error}"))?;
    let input = process.stdin.take().ok_or("Нет связи с процессом сервера")?;
    let output = process.stdout.take().ok_or("Нет связи с процессом сервера")?;
    Ok(Transport::Stdio { _process: process, input, output: BufReader::new(output) })
}

fn clean_name_part(text: &str) -> String {
    text.chars()
        .map(|symbol| if symbol.is_ascii_alphanumeric() || symbol == '_' || symbol == '-' { symbol } else { '_' })
        .collect()
}

pub fn public_tool_name(server: &str, tool: &str) -> String {
    let full = format!("mcp__{}__{}", clean_name_part(server), clean_name_part(tool));
    full.chars().take(MAX_TOOL_NAME_LENGTH).collect()
}

fn tools_from_listing(server: &str, listing: &Value) -> Vec<McpTool> {
    listing["tools"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| {
            let tool = entry["name"].as_str()?;
            Some(McpTool {
                public_name: public_tool_name(server, tool),
                server: server.to_string(),
                tool: tool.to_string(),
                description: entry["description"].as_str().unwrap_or_default().to_string(),
                schema: if entry["inputSchema"].is_object() { entry["inputSchema"].clone() } else { json!({ "type": "object", "properties": {} }) },
                read_only: entry["annotations"]["readOnlyHint"] == true,
            })
        })
        .collect()
}

async fn connect(config: &McpServerConfig, http: reqwest::Client) -> Result<(Connection, Vec<McpTool>), String> {
    let transport = open_transport(config, http)?;
    let mut connection = Connection { transport, next_id: 1 };
    connection
        .request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "Tarn", "version": env!("CARGO_PKG_VERSION") }
            }),
        )
        .await?;
    connection.notify("notifications/initialized").await?;
    let listing = connection.request("tools/list", json!({})).await?;
    let tools = tools_from_listing(&config.name, &listing);
    Ok((connection, tools))
}

fn text_of_call_result(result: &Value) -> String {
    let mut pieces = Vec::new();
    for block in result["content"].as_array().map(Vec::as_slice).unwrap_or_default() {
        match block["type"].as_str().unwrap_or_default() {
            "text" => pieces.push(block["text"].as_str().unwrap_or_default().to_string()),
            "image" => pieces.push("[изображение]".to_string()),
            other => pieces.push(format!("[{other}]")),
        }
    }
    let text = pieces.join("\n");
    match text.char_indices().nth(MAX_RESULT_CHARS) {
        Some((index, _)) => format!("{}\n… результат обрезан", &text[..index]),
        None => text,
    }
}

struct LiveServer {
    connection: Mutex<Connection>,
    tools: Vec<McpTool>,
}

#[derive(Default)]
pub struct McpPool {
    servers: Mutex<HashMap<String, Arc<LiveServer>>>,
}

impl McpPool {
    pub async fn tools_of(&self, config: &McpServerConfig, http: reqwest::Client) -> Result<Vec<McpTool>, String> {
        let mut servers = self.servers.lock().await;
        if let Some(live) = servers.get(&config.name) {
            return Ok(live.tools.clone());
        }
        let (connection, tools) = tokio::time::timeout(CONNECT_TIMEOUT, connect(config, http))
            .await
            .map_err(|_| "Сервер не ответил за 30 с".to_string())??;
        servers.insert(config.name.clone(), Arc::new(LiveServer { connection: Mutex::new(connection), tools: tools.clone() }));
        Ok(tools)
    }

    pub async fn running_tools_of(&self, server: &str) -> Option<Vec<McpTool>> {
        self.servers.lock().await.get(server).map(|live| live.tools.clone())
    }

    pub async fn forget(&self, server: &str) {
        self.servers.lock().await.remove(server);
    }

    pub async fn call(&self, tool: &McpTool, input: &Value, cancel: &Arc<Notify>) -> Result<String, String> {
        let live = self.servers.lock().await.get(&tool.server).cloned().ok_or("Сервер не запущен")?;
        let params = json!({ "name": tool.tool, "arguments": input });
        let outcome = tokio::select! {
            result = async {
                let mut connection = live.connection.lock().await;
                tokio::time::timeout(CALL_TIMEOUT, connection.request("tools/call", params)).await
            } => result,
            _ = cancel.notified() => {
                cancel.notify_one();
                self.forget(&tool.server).await;
                return Err("Вызов остановлен пользователем".to_string());
            }
        };
        let Ok(reply) = outcome else {
            self.forget(&tool.server).await;
            return Err("Сервер не ответил за 5 минут".to_string());
        };
        let result = match reply {
            Ok(result) => result,
            Err(message) => {
                self.forget(&tool.server).await;
                return Err(message);
            }
        };
        let text = text_of_call_result(&result);
        if result["isError"] == true {
            Err(text)
        } else {
            Ok(text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_safe_and_short() {
        assert_eq!(public_tool_name("my server", "read.file"), "mcp__my_server__read_file");
        assert!(public_tool_name(&"s".repeat(50), &"t".repeat(50)).len() <= MAX_TOOL_NAME_LENGTH);
    }

    #[test]
    fn reads_tools_with_read_only_hint_from_listing() {
        let listing = json!({ "tools": [
            { "name": "find", "description": "Ищет", "annotations": { "readOnlyHint": true }, "inputSchema": { "type": "object" } },
            { "name": "write" },
        ] });
        let tools = tools_from_listing("github", &listing);
        assert!(tools[0].read_only);
        assert!(!tools[1].read_only);
        assert_eq!(tools[1].schema["type"], "object");
    }

    #[test]
    fn joins_call_result_blocks() {
        let result = json!({ "content": [{ "type": "text", "text": "первое" }, { "type": "image", "data": "x" }] });
        assert_eq!(text_of_call_result(&result), "первое\n[изображение]");
    }

    #[test]
    fn stops_with_error_when_server_answers_with_error() {
        assert!(result_of_reply(json!({ "error": { "message": "нет" } })).is_err());
        assert_eq!(result_of_reply(json!({ "result": { "a": 1 } })).unwrap()["a"], 1);
    }
}
