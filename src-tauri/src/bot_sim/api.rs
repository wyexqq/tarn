use std::collections::{HashMap, VecDeque};

use serde_json::{json, Value};

use super::text::{self, TextRun};

pub const USER_ID: i64 = 100_001;
pub const BOT_ID: i64 = 200_002;
const USER_NAME: &str = "Тестер";
const USER_LOGIN: &str = "tester";
const BOT_NAME: &str = "Тестовый бот";
const BOT_LOGIN: &str = "tarn_test_bot";
const MAX_MESSAGES: usize = 300;
const MAX_LOG: usize = 200;
const MAX_FILES: usize = 200;
const MAX_TEXT_UNITS: usize = 4096;
const MAX_CAPTION_UNITS: usize = 1024;
const MAX_CALLBACK_DATA_BYTES: usize = 64;
const MAX_MEDIA_GROUP: usize = 10;
const TYPING_MS: i64 = 5_000;
const CALLBACK_LIFETIME_MS: i64 = 15 * 60 * 1000;
const DEFAULT_UPDATES_LIMIT: usize = 100;
const NOT_MODIFIED: &str = "message is not modified: specified new message content and reply markup are exactly the same as a current content and reply markup of the message";
const MEDIA_METHODS: [(&str, &str); 8] = [
    ("sendphoto", "photo"),
    ("senddocument", "document"),
    ("sendvideo", "video"),
    ("sendanimation", "animation"),
    ("sendaudio", "audio"),
    ("sendvoice", "voice"),
    ("sendsticker", "sticker"),
    ("sendvideonote", "video_note"),
];
const PLACEHOLDER_METHODS: [(&str, &str); 5] = [
    ("sendlocation", "📍 Геопозиция"),
    ("sendvenue", "📍 Место"),
    ("sendcontact", "👤 Контакт"),
    ("sendpoll", "📊 Опрос"),
    ("senddice", "🎲"),
];
const ALWAYS_TRUE_METHODS: [&str; 12] = [
    "logout",
    "close",
    "setchatmenubutton",
    "setmydescription",
    "setmyshortdescription",
    "setmyname",
    "pinchatmessage",
    "unpinchatmessage",
    "unpinallchatmessages",
    "setmessagereaction",
    "setmydefaultadministratorrights",
    "setchatpermissions",
];

#[derive(Clone)]
pub struct StoredFile {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub name: String,
    pub url: Option<String>,
}

pub struct Upload {
    pub bytes: Vec<u8>,
    pub name: String,
    pub mime: String,
}

#[derive(Default)]
pub struct Params {
    pub fields: HashMap<String, Value>,
    pub uploads: HashMap<String, Upload>,
}

impl Params {
    pub fn text(&self, name: &str) -> Option<String> {
        match self.fields.get(name)? {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            Value::Bool(flag) => Some(flag.to_string()),
            _ => None,
        }
    }

    pub fn int(&self, name: &str) -> Option<i64> {
        match self.fields.get(name)? {
            Value::Number(number) => number.as_i64(),
            Value::String(text) => text.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn flag(&self, name: &str) -> bool {
        matches!(self.text(name).as_deref(), Some("true") | Some("1"))
    }

    fn json(&self, name: &str) -> Result<Option<Value>, ApiError> {
        match self.fields.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) if text.trim().is_empty() => Ok(None),
            Some(Value::String(text)) => serde_json::from_str(text).map(Some).map_err(|_| bad_request(format!("can't parse {name} JSON object"))),
            Some(other) => Ok(Some(other.clone())),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct ApiError {
    pub code: u16,
    pub description: String,
}

fn bad_request(text: impl Into<String>) -> ApiError {
    ApiError { code: 400, description: format!("Bad Request: {}", text.into()) }
}

#[derive(Clone, PartialEq)]
pub struct Media {
    pub kind: String,
    pub file_id: String,
    pub name: String,
}

#[derive(Clone)]
pub struct SimMessage {
    pub id: i64,
    pub from_bot: bool,
    pub text: String,
    pub parse_mode: Option<String>,
    pub runs: Vec<TextRun>,
    pub media: Option<Media>,
    pub inline_keyboard: Option<Value>,
    pub forwarded: bool,
    pub date: i64,
    pub edited: bool,
    pub touched: u64,
}

pub struct LogEntry {
    pub serial: u64,
    pub at: i64,
    pub method: String,
    pub detail: String,
    pub problem: Option<String>,
    pub from_user: bool,
}

pub struct Notice {
    pub serial: u64,
    pub text: String,
    pub alert: bool,
}

pub struct BotCommand {
    pub command: String,
    pub description: String,
}

pub struct Webhook {
    pub url: String,
    pub secret: Option<String>,
}

#[derive(Default)]
pub struct SimState {
    pub messages: Vec<SimMessage>,
    pending: VecDeque<Value>,
    update_counter: i64,
    message_counter: i64,
    callback_counter: i64,
    file_counter: u64,
    callbacks: HashMap<String, i64>,
    files: HashMap<String, StoredFile>,
    file_order: VecDeque<String>,
    pub log: VecDeque<LogEntry>,
    pub serial: u64,
    pub commands: Vec<BotCommand>,
    pub keyboard: Option<Vec<Vec<String>>>,
    keyboard_one_time: bool,
    pub typing_until: i64,
    pub notice: Option<Notice>,
    pub last_bot_call_at: Option<i64>,
    pub webhook: Option<Webhook>,
}

fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

fn short(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or_default();
    let cut: String = first_line.chars().take(60).collect();
    if cut.chars().count() < text.chars().count() {
        format!("«{cut}…»")
    } else {
        format!("«{cut}»")
    }
}

fn user_json() -> Value {
    json!({ "id": USER_ID, "is_bot": false, "first_name": USER_NAME, "username": USER_LOGIN, "language_code": "ru" })
}

fn bot_json() -> Value {
    json!({ "id": BOT_ID, "is_bot": true, "first_name": BOT_NAME, "username": BOT_LOGIN })
}

fn chat_json() -> Value {
    json!({ "id": USER_ID, "type": "private", "first_name": USER_NAME, "username": USER_LOGIN })
}

fn command_entities(text: &str) -> Option<Value> {
    if !text.starts_with('/') {
        return None;
    }
    let command = text.split_whitespace().next().unwrap_or(text);
    Some(json!([{ "type": "bot_command", "offset": 0, "length": units(command) }]))
}

fn media_json(media: &Media, file: Option<&StoredFile>) -> Value {
    let size = file.map_or(0, |file| file.bytes.len());
    let unique_id = format!("u{}", media.file_id);
    match media.kind.as_str() {
        "photo" => json!([{ "file_id": media.file_id, "file_unique_id": unique_id, "width": 1280, "height": 960, "file_size": size }]),
        _ => json!({
            "file_id": media.file_id,
            "file_unique_id": unique_id,
            "file_name": media.name,
            "mime_type": file.map(|file| file.mime.clone()).unwrap_or_default(),
            "file_size": size,
        }),
    }
}

impl SimState {
    fn next_serial(&mut self) -> u64 {
        self.serial += 1;
        self.serial
    }

    fn next_message_id(&mut self) -> i64 {
        self.message_counter += 1;
        self.message_counter
    }

    fn next_update(&mut self, field: &str, payload: Value) -> Value {
        self.update_counter += 1;
        let mut update = json!({ "update_id": self.update_counter });
        update[field] = payload;
        update
    }

    pub fn file(&self, id: &str) -> Option<StoredFile> {
        self.files.get(id).cloned()
    }

    fn store_file(&mut self, file: StoredFile) -> String {
        self.file_counter += 1;
        let id = format!("AgAD{}{}", self.file_counter, uuid::Uuid::new_v4().simple());
        self.files.insert(id.clone(), file);
        self.file_order.push_back(id.clone());
        while self.file_order.len() > MAX_FILES {
            if let Some(oldest) = self.file_order.pop_front() {
                self.files.remove(&oldest);
            }
        }
        id
    }

    pub fn message_json(&self, message: &SimMessage) -> Value {
        let mut value = json!({
            "message_id": message.id,
            "from": if message.from_bot { bot_json() } else { user_json() },
            "chat": chat_json(),
            "date": message.date / 1000,
        });
        match &message.media {
            None => value["text"] = json!(message.text),
            Some(media) => {
                value[media.kind.as_str()] = media_json(media, self.files.get(&media.file_id));
                if !message.text.is_empty() {
                    value["caption"] = json!(message.text);
                }
            }
        }
        if !message.from_bot {
            if let Some(entities) = command_entities(&message.text) {
                value["entities"] = entities;
            }
        }
        if let Some(rows) = &message.inline_keyboard {
            value["reply_markup"] = json!({ "inline_keyboard": rows });
        }
        if message.edited {
            value["edit_date"] = json!(message.date / 1000);
        }
        value
    }

    fn push_message(&mut self, mut message: SimMessage) -> SimMessage {
        message.touched = self.next_serial();
        if message.from_bot {
            self.typing_until = 0;
        }
        self.messages.push(message.clone());
        if self.messages.len() > MAX_MESSAGES {
            let extra = self.messages.len() - MAX_MESSAGES;
            self.messages.drain(..extra);
        }
        message
    }

    pub fn add_log(&mut self, method: &str, detail: String, problem: Option<String>, from_user: bool, now: i64) {
        let serial = self.next_serial();
        self.log.push_back(LogEntry { serial, at: now, method: method.to_string(), detail, problem, from_user });
        while self.log.len() > MAX_LOG {
            self.log.pop_front();
        }
    }

    pub fn reset(&mut self) {
        self.messages.clear();
        self.pending.clear();
        self.callbacks.clear();
        self.log.clear();
        self.keyboard = None;
        self.notice = None;
        self.typing_until = 0;
        self.serial += 1;
    }

    pub fn queue(&mut self, update: Value) {
        self.pending.push_back(update);
    }

    pub fn take_updates(&mut self, offset: Option<i64>, limit: Option<i64>) -> Vec<Value> {
        if let Some(offset) = offset.filter(|offset| *offset > 0) {
            self.pending.retain(|update| update["update_id"].as_i64().unwrap_or(0) >= offset);
        }
        let limit = limit.filter(|limit| *limit > 0).map_or(DEFAULT_UPDATES_LIMIT, |limit| limit as usize);
        self.pending.iter().take(limit).cloned().collect()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    fn hide_one_time_keyboard(&mut self) {
        if self.keyboard_one_time {
            self.keyboard = None;
            self.keyboard_one_time = false;
        }
    }

    pub fn user_text(&mut self, text: &str, now: i64) -> Value {
        self.hide_one_time_keyboard();
        let id = self.next_message_id();
        let message = SimMessage {
            id,
            from_bot: false,
            text: text.to_string(),
            parse_mode: None,
            runs: text::plain(text),
            media: None,
            inline_keyboard: None,
            forwarded: false,
            date: now,
            edited: false,
            touched: 0,
        };
        let message = self.push_message(message);
        self.add_log("message", short(text), None, true, now);
        let payload = self.message_json(&message);
        self.next_update("message", payload)
    }

    pub fn user_photo(&mut self, upload: Upload, caption: &str, now: i64) -> Value {
        let name = upload.name.clone();
        let file_id = self.store_file(StoredFile { bytes: upload.bytes, mime: upload.mime, name: upload.name, url: None });
        let id = self.next_message_id();
        let message = SimMessage {
            id,
            from_bot: false,
            text: caption.to_string(),
            parse_mode: None,
            runs: text::plain(caption),
            media: Some(Media { kind: "photo".to_string(), file_id, name: name.clone() }),
            inline_keyboard: None,
            forwarded: false,
            date: now,
            edited: false,
            touched: 0,
        };
        let message = self.push_message(message);
        self.add_log("message", format!("фото {name}"), None, true, now);
        let payload = self.message_json(&message);
        self.next_update("message", payload)
    }

    pub fn user_press(&mut self, message_id: i64, data: &str, now: i64) -> Result<Value, String> {
        let message = self
            .messages
            .iter()
            .find(|message| message.id == message_id && message.from_bot)
            .cloned()
            .ok_or("Сообщения с этой кнопкой уже нет")?;
        self.callback_counter += 1;
        let query_id = format!("{}{:06}", now, self.callback_counter);
        self.callbacks.insert(query_id.clone(), now);
        self.add_log("callback_query", short(data), None, true, now);
        let payload = json!({
            "id": query_id,
            "from": user_json(),
            "message": self.message_json(&message),
            "chat_instance": format!("{USER_ID}"),
            "data": data,
        });
        Ok(self.next_update("callback_query", payload))
    }

    pub fn call(&mut self, method: &str, params: &Params, now: i64) -> Result<Value, ApiError> {
        self.last_bot_call_at = Some(now);
        let outcome = self.dispatch(&method.to_ascii_lowercase(), params, now);
        let detail = describe_call(params);
        let problem = outcome.as_ref().err().map(|error| error.description.clone());
        self.add_log(method, detail, problem, false, now);
        outcome
    }

    fn dispatch(&mut self, method: &str, params: &Params, now: i64) -> Result<Value, ApiError> {
        if let Some((_, kind)) = MEDIA_METHODS.iter().find(|(name, _)| *name == method) {
            return self.send_media(params, kind, now);
        }
        if let Some((_, label)) = PLACEHOLDER_METHODS.iter().find(|(name, _)| *name == method) {
            return self.send_placeholder(params, label, now);
        }
        if ALWAYS_TRUE_METHODS.contains(&method) {
            return Ok(json!(true));
        }
        match method {
            "getme" => Ok(json!({
                "id": BOT_ID, "is_bot": true, "first_name": BOT_NAME, "username": BOT_LOGIN,
                "can_join_groups": true, "can_read_all_group_messages": false, "supports_inline_queries": false
            })),
            "setwebhook" => self.set_webhook(params),
            "deletewebhook" => {
                self.webhook = None;
                if params.flag("drop_pending_updates") {
                    self.pending.clear();
                }
                Ok(json!(true))
            }
            "getwebhookinfo" => Ok(json!({
                "url": self.webhook.as_ref().map(|hook| hook.url.clone()).unwrap_or_default(),
                "has_custom_certificate": false,
                "pending_update_count": self.pending.len(),
            })),
            "sendmessage" => self.send_message(params, now),
            "sendmediagroup" => self.send_media_group(params, now),
            "editmessagetext" => self.edit_text(params, now),
            "editmessagecaption" => self.edit_caption(params, now),
            "editmessagemedia" => self.edit_media(params, now),
            "editmessagereplymarkup" => self.edit_markup(params, now),
            "deletemessage" => self.delete_message(params.int("message_id")),
            "deletemessages" => self.delete_messages(params),
            "answercallbackquery" => self.answer_callback(params, now),
            "sendchataction" => {
                self.typing_until = now + TYPING_MS;
                Ok(json!(true))
            }
            "setmycommands" => self.set_commands(params),
            "getmycommands" => Ok(Value::Array(
                self.commands.iter().map(|command| json!({ "command": command.command, "description": command.description })).collect(),
            )),
            "deletemycommands" => {
                self.commands.clear();
                Ok(json!(true))
            }
            "getchat" => Ok(chat_json()),
            "getchatmember" => Ok(json!({ "status": "member", "user": user_json() })),
            "getfile" => self.get_file(params),
            "copymessage" | "forwardmessage" => self.copy_message(params, method == "forwardmessage", now),
            _ => Ok(json!(true)),
        }
    }

    fn set_webhook(&mut self, params: &Params) -> Result<Value, ApiError> {
        let url = params.text("url").unwrap_or_default();
        if params.flag("drop_pending_updates") {
            self.pending.clear();
        }
        if url.trim().is_empty() {
            self.webhook = None;
            return Ok(json!(true));
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(bad_request("bad webhook: An HTTPS URL must be provided for webhook"));
        }
        self.webhook = Some(Webhook { url, secret: params.text("secret_token").filter(|secret| !secret.is_empty()) });
        Ok(json!(true))
    }

    fn targets_user(&self, params: &Params) -> Result<bool, ApiError> {
        let chat = params.text("chat_id").ok_or_else(|| bad_request("chat_id is empty"))?;
        let chat = chat.trim();
        Ok(chat == USER_ID.to_string() || chat.eq_ignore_ascii_case(&format!("@{USER_LOGIN}")))
    }

    fn other_chat_reply(&mut self, params: &Params, text: &str, now: i64) -> Value {
        let chat = params.text("chat_id").unwrap_or_default();
        let id = self.next_message_id();
        self.add_log("в другой чат", format!("{chat}: {}", short(text)), None, false, now);
        json!({ "message_id": id, "from": bot_json(), "chat": { "id": params.int("chat_id").unwrap_or(0), "type": "private" }, "date": now / 1000, "text": text })
    }

    fn read_inline_keyboard(&self, markup: &Value) -> Result<Option<Value>, ApiError> {
        let Some(rows) = markup.get("inline_keyboard") else {
            return Ok(None);
        };
        let rows = rows.as_array().ok_or_else(|| bad_request("field \"inline_keyboard\" of the InlineKeyboardMarkup must be an Array"))?;
        for row in rows {
            let buttons = row.as_array().ok_or_else(|| bad_request("can't parse inline keyboard button: InlineKeyboardButton must be an Object"))?;
            for button in buttons {
                if button["text"].as_str().is_none_or(str::is_empty) {
                    return Err(bad_request("can't parse inline keyboard button: Text must be non-empty"));
                }
                let has_action = ["callback_data", "url", "web_app", "switch_inline_query", "switch_inline_query_current_chat", "copy_text", "pay", "login_url"]
                    .iter()
                    .any(|field| !button[*field].is_null());
                if !has_action {
                    return Err(bad_request("can't parse inline keyboard button: Text buttons are unallowed in the inline keyboard"));
                }
                if button["callback_data"].as_str().is_some_and(|data| data.len() > MAX_CALLBACK_DATA_BYTES) {
                    return Err(bad_request("BUTTON_DATA_INVALID"));
                }
            }
        }
        Ok(Some(Value::Array(rows.clone())))
    }

    fn apply_reply_keyboard(&mut self, markup: &Value) {
        if markup["remove_keyboard"].as_bool() == Some(true) {
            self.keyboard = None;
            self.keyboard_one_time = false;
            return;
        }
        let Some(rows) = markup["keyboard"].as_array() else {
            return;
        };
        let labels = rows
            .iter()
            .filter_map(Value::as_array)
            .map(|row| row.iter().filter_map(|button| button.as_str().or_else(|| button["text"].as_str())).map(str::to_string).collect())
            .collect();
        self.keyboard = Some(labels);
        self.keyboard_one_time = markup["one_time_keyboard"].as_bool() == Some(true);
    }

    fn read_markup(&mut self, params: &Params, allow_reply_keyboard: bool) -> Result<Option<Value>, ApiError> {
        let Some(markup) = params.json("reply_markup")? else {
            return Ok(None);
        };
        let inline = self.read_inline_keyboard(&markup)?;
        if allow_reply_keyboard {
            self.apply_reply_keyboard(&markup);
        }
        Ok(inline)
    }

    fn checked_text(&self, text: &str, mode: Option<&str>, limit: usize, too_long: &str) -> Result<Vec<TextRun>, ApiError> {
        if units(text) > limit {
            return Err(bad_request(too_long));
        }
        text::parse(text, mode).map_err(|problem| bad_request(format!("can't parse entities: {problem}")))
    }

    fn send_message(&mut self, params: &Params, now: i64) -> Result<Value, ApiError> {
        let text = params.text("text").unwrap_or_default();
        if text.trim().is_empty() {
            return Err(bad_request("message text is empty"));
        }
        let parse_mode = params.text("parse_mode").filter(|mode| !mode.is_empty());
        let runs = self.checked_text(&text, parse_mode.as_deref(), MAX_TEXT_UNITS, "message is too long")?;
        if !self.targets_user(params)? {
            return Ok(self.other_chat_reply(params, &text, now));
        }
        let inline_keyboard = self.read_markup(params, true)?;
        let id = self.next_message_id();
        let message = SimMessage {
            id,
            from_bot: true,
            text,
            parse_mode,
            runs,
            media: None,
            inline_keyboard,
            forwarded: false,
            date: now,
            edited: false,
            touched: 0,
        };
        let message = self.push_message(message);
        Ok(self.message_json(&message))
    }

    fn media_from_upload(&mut self, kind: &str, upload: &Upload) -> Media {
        let name = upload.name.clone();
        let file = StoredFile { bytes: upload.bytes.clone(), mime: upload.mime.clone(), name: name.clone(), url: None };
        Media { kind: kind.to_string(), file_id: self.store_file(file), name }
    }

    fn media_from_reference(&mut self, kind: &str, reference: &str, params: &Params) -> Result<Media, ApiError> {
        if let Some(field) = reference.strip_prefix("attach://") {
            let upload = params.uploads.get(field).ok_or_else(|| bad_request(format!("file {field} not found in the request")))?;
            return Ok(self.media_from_upload(kind, upload));
        }
        if reference.starts_with("http://") || reference.starts_with("https://") {
            let name = reference.rsplit('/').next().unwrap_or(reference).to_string();
            let file = StoredFile { bytes: Vec::new(), mime: String::new(), name: name.clone(), url: Some(reference.to_string()) };
            return Ok(Media { kind: kind.to_string(), file_id: self.store_file(file), name });
        }
        let known = self.files.get(reference).ok_or_else(|| bad_request("wrong file identifier/HTTP URL specified"))?;
        Ok(Media { kind: kind.to_string(), file_id: reference.to_string(), name: known.name.clone() })
    }

    fn resolve_media(&mut self, params: &Params, kind: &str) -> Result<Media, ApiError> {
        if let Some(upload) = params.uploads.get(kind) {
            return Ok(self.media_from_upload(kind, upload));
        }
        let reference = params.text(kind).filter(|value| !value.is_empty()).ok_or_else(|| bad_request(format!("there is no {kind} in the request")))?;
        self.media_from_reference(kind, &reference, params)
    }

    fn send_media(&mut self, params: &Params, kind: &str, now: i64) -> Result<Value, ApiError> {
        let caption = params.text("caption").unwrap_or_default();
        let parse_mode = params.text("parse_mode").filter(|mode| !mode.is_empty());
        let runs = self.checked_text(&caption, parse_mode.as_deref(), MAX_CAPTION_UNITS, "message caption is too long")?;
        let media = self.resolve_media(params, kind)?;
        if !self.targets_user(params)? {
            return Ok(self.other_chat_reply(params, &format!("[{kind}] {caption}"), now));
        }
        let inline_keyboard = self.read_markup(params, true)?;
        let id = self.next_message_id();
        let message = SimMessage {
            id,
            from_bot: true,
            text: caption,
            parse_mode,
            runs,
            media: Some(media),
            inline_keyboard,
            forwarded: false,
            date: now,
            edited: false,
            touched: 0,
        };
        let message = self.push_message(message);
        Ok(self.message_json(&message))
    }

    fn send_media_group(&mut self, params: &Params, now: i64) -> Result<Value, ApiError> {
        let items = params.json("media")?.and_then(|value| value.as_array().cloned()).ok_or_else(|| bad_request("parameter \"media\" is required"))?;
        if items.len() < 2 || items.len() > MAX_MEDIA_GROUP {
            return Err(bad_request("wrong number of media in the album, must be 2-10"));
        }
        let mut sent = Vec::new();
        for item in items {
            let kind = item["type"].as_str().unwrap_or("photo").to_string();
            let reference = item["media"].as_str().unwrap_or_default().to_string();
            let caption = item["caption"].as_str().unwrap_or_default().to_string();
            let parse_mode = item["parse_mode"].as_str().map(str::to_string);
            let runs = self.checked_text(&caption, parse_mode.as_deref(), MAX_CAPTION_UNITS, "message caption is too long")?;
            let media = self.media_from_reference(&kind, &reference, params)?;
            let id = self.next_message_id();
            let message = SimMessage {
                id,
                from_bot: true,
                text: caption,
                parse_mode,
                runs,
                media: Some(media),
                inline_keyboard: None,
                forwarded: false,
                date: now,
                edited: false,
                touched: 0,
            };
            let message = self.push_message(message);
            sent.push(self.message_json(&message));
        }
        Ok(Value::Array(sent))
    }

    fn send_placeholder(&mut self, params: &Params, label: &str, now: i64) -> Result<Value, ApiError> {
        if !self.targets_user(params)? {
            return Ok(self.other_chat_reply(params, label, now));
        }
        let question = params.text("question").map(|question| format!("{label}: {question}")).unwrap_or_else(|| label.to_string());
        let inline_keyboard = self.read_markup(params, true)?;
        let id = self.next_message_id();
        let message = SimMessage {
            id,
            from_bot: true,
            text: question.clone(),
            parse_mode: None,
            runs: text::plain(&question),
            media: None,
            inline_keyboard,
            forwarded: false,
            date: now,
            edited: false,
            touched: 0,
        };
        let message = self.push_message(message);
        Ok(self.message_json(&message))
    }

    fn bot_message_index(&self, params: &Params) -> Result<usize, ApiError> {
        if params.text("inline_message_id").is_some() {
            return Err(bad_request("inline messages are not supported by the simulator"));
        }
        let id = params.int("message_id").ok_or_else(|| bad_request("message identifier is not specified"))?;
        let index = self.messages.iter().position(|message| message.id == id).ok_or_else(|| bad_request("message to edit not found"))?;
        if !self.messages[index].from_bot {
            return Err(bad_request("message can't be edited"));
        }
        Ok(index)
    }

    fn finish_edit(&mut self, index: usize, now: i64) -> Value {
        let serial = self.next_serial();
        let message = &mut self.messages[index];
        message.edited = true;
        message.date = now;
        message.touched = serial;
        let snapshot = message.clone();
        self.message_json(&snapshot)
    }

    fn edit_text(&mut self, params: &Params, now: i64) -> Result<Value, ApiError> {
        let index = self.bot_message_index(params)?;
        let text = params.text("text").unwrap_or_default();
        if text.trim().is_empty() {
            return Err(bad_request("message text is empty"));
        }
        let parse_mode = params.text("parse_mode").filter(|mode| !mode.is_empty());
        let runs = self.checked_text(&text, parse_mode.as_deref(), MAX_TEXT_UNITS, "message is too long")?;
        let inline_keyboard = self.read_markup(params, false)?;
        let message = &self.messages[index];
        if message.media.is_some() {
            return Err(bad_request("there is no text in the message to edit"));
        }
        if message.text == text && message.parse_mode == parse_mode && message.inline_keyboard == inline_keyboard {
            return Err(bad_request(NOT_MODIFIED));
        }
        let message = &mut self.messages[index];
        message.text = text;
        message.parse_mode = parse_mode;
        message.runs = runs;
        message.inline_keyboard = inline_keyboard;
        Ok(self.finish_edit(index, now))
    }

    fn edit_caption(&mut self, params: &Params, now: i64) -> Result<Value, ApiError> {
        let index = self.bot_message_index(params)?;
        let caption = params.text("caption").unwrap_or_default();
        let parse_mode = params.text("parse_mode").filter(|mode| !mode.is_empty());
        let runs = self.checked_text(&caption, parse_mode.as_deref(), MAX_CAPTION_UNITS, "message caption is too long")?;
        let inline_keyboard = self.read_markup(params, false)?;
        let message = &self.messages[index];
        if message.media.is_none() {
            return Err(bad_request("there is no caption in the message to edit"));
        }
        if message.text == caption && message.inline_keyboard == inline_keyboard {
            return Err(bad_request(NOT_MODIFIED));
        }
        let message = &mut self.messages[index];
        message.text = caption;
        message.parse_mode = parse_mode;
        message.runs = runs;
        message.inline_keyboard = inline_keyboard;
        Ok(self.finish_edit(index, now))
    }

    fn edit_media(&mut self, params: &Params, now: i64) -> Result<Value, ApiError> {
        let index = self.bot_message_index(params)?;
        let item = params.json("media")?.ok_or_else(|| bad_request("parameter \"media\" is required"))?;
        let kind = item["type"].as_str().unwrap_or("photo").to_string();
        let reference = item["media"].as_str().unwrap_or_default().to_string();
        let caption = item["caption"].as_str().unwrap_or_default().to_string();
        let parse_mode = item["parse_mode"].as_str().map(str::to_string);
        let runs = self.checked_text(&caption, parse_mode.as_deref(), MAX_CAPTION_UNITS, "message caption is too long")?;
        let media = self.media_from_reference(&kind, &reference, params)?;
        let inline_keyboard = self.read_markup(params, false)?;
        let message = &mut self.messages[index];
        message.media = Some(media);
        message.text = caption;
        message.parse_mode = parse_mode;
        message.runs = runs;
        message.inline_keyboard = inline_keyboard;
        Ok(self.finish_edit(index, now))
    }

    fn edit_markup(&mut self, params: &Params, now: i64) -> Result<Value, ApiError> {
        let index = self.bot_message_index(params)?;
        let inline_keyboard = self.read_markup(params, false)?;
        if self.messages[index].inline_keyboard == inline_keyboard {
            return Err(bad_request(NOT_MODIFIED));
        }
        self.messages[index].inline_keyboard = inline_keyboard;
        Ok(self.finish_edit(index, now))
    }

    fn delete_message(&mut self, message_id: Option<i64>) -> Result<Value, ApiError> {
        let id = message_id.ok_or_else(|| bad_request("message identifier is not specified"))?;
        let before = self.messages.len();
        self.messages.retain(|message| message.id != id);
        if self.messages.len() == before {
            return Err(bad_request("message to delete not found"));
        }
        self.serial += 1;
        Ok(json!(true))
    }

    fn delete_messages(&mut self, params: &Params) -> Result<Value, ApiError> {
        let ids = params.json("message_ids")?.and_then(|value| value.as_array().cloned()).unwrap_or_default();
        for id in ids.iter().filter_map(Value::as_i64) {
            let _ = self.delete_message(Some(id));
        }
        Ok(json!(true))
    }

    fn answer_callback(&mut self, params: &Params, now: i64) -> Result<Value, ApiError> {
        let query_id = params.text("callback_query_id").unwrap_or_default();
        let created = self.callbacks.remove(&query_id);
        let is_alive = created.is_some_and(|created| now - created <= CALLBACK_LIFETIME_MS);
        if !is_alive {
            return Err(bad_request("query is too old and response timeout expired or query ID is invalid"));
        }
        let text = params.text("text").unwrap_or_default();
        if !text.is_empty() {
            let serial = self.next_serial();
            self.notice = Some(Notice { serial, text, alert: params.flag("show_alert") });
        }
        Ok(json!(true))
    }

    fn set_commands(&mut self, params: &Params) -> Result<Value, ApiError> {
        let list = params.json("commands")?.and_then(|value| value.as_array().cloned()).ok_or_else(|| bad_request("parameter \"commands\" is required"))?;
        let mut commands = Vec::new();
        for item in list {
            let command = item["command"].as_str().unwrap_or_default().trim_start_matches('/').to_string();
            let is_valid = !command.is_empty()
                && command.len() <= 32
                && command.chars().all(|letter| letter.is_ascii_lowercase() || letter.is_ascii_digit() || letter == '_');
            if !is_valid {
                return Err(bad_request("BOT_COMMAND_INVALID"));
            }
            let description = item["description"].as_str().unwrap_or_default().to_string();
            commands.push(BotCommand { command, description });
        }
        self.commands = commands;
        self.serial += 1;
        Ok(json!(true))
    }

    fn get_file(&self, params: &Params) -> Result<Value, ApiError> {
        let id = params.text("file_id").unwrap_or_default();
        let file = self.files.get(&id).ok_or_else(|| bad_request("invalid file_id"))?;
        Ok(json!({ "file_id": id, "file_unique_id": format!("u{id}"), "file_size": file.bytes.len(), "file_path": format!("files/{id}") }))
    }

    pub fn file_by_path(&self, path: &str) -> Option<StoredFile> {
        let id = path.strip_prefix("files/")?;
        self.file(id)
    }

    fn copy_message(&mut self, params: &Params, is_forward: bool, now: i64) -> Result<Value, ApiError> {
        let source_id = params.int("message_id").ok_or_else(|| bad_request("message identifier is not specified"))?;
        let source = self.messages.iter().find(|message| message.id == source_id).cloned().ok_or_else(|| bad_request("message to copy not found"))?;
        if !self.targets_user(params)? {
            let reply = self.other_chat_reply(params, &source.text, now);
            return Ok(if is_forward { reply } else { json!({ "message_id": reply["message_id"] }) });
        }
        let id = self.next_message_id();
        let copy = SimMessage { id, from_bot: true, forwarded: is_forward, date: now, edited: false, touched: 0, ..source };
        let copy = self.push_message(copy);
        Ok(if is_forward { self.message_json(&copy) } else { json!({ "message_id": id }) })
    }
}

fn describe_call(params: &Params) -> String {
    if let Some(text) = params.text("text").filter(|text| !text.is_empty()) {
        return short(&text);
    }
    if let Some(caption) = params.text("caption").filter(|caption| !caption.is_empty()) {
        return short(&caption);
    }
    if let Some(id) = params.int("message_id") {
        return format!("сообщение {id}");
    }
    if let Some(action) = params.text("action") {
        return action;
    }
    if let Some(url) = params.text("url") {
        return url;
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(fields: Value) -> Params {
        let Value::Object(map) = fields else { panic!("нужен объект") };
        Params { fields: map.into_iter().collect(), uploads: HashMap::new() }
    }

    #[test]
    fn user_text_becomes_an_update_with_command_entity() {
        let mut state = SimState::default();
        let update = state.user_text("/start ref", 1_000);
        assert_eq!(update["update_id"], 1);
        assert_eq!(update["message"]["text"], "/start ref");
        assert_eq!(update["message"]["entities"][0]["length"], 6);
        assert_eq!(update["message"]["chat"]["id"], USER_ID);
    }

    #[test]
    fn updates_are_confirmed_by_offset() {
        let mut state = SimState::default();
        for text in ["a", "b", "c"] {
            let update = state.user_text(text, 0);
            state.queue(update);
        }
        assert_eq!(state.take_updates(None, None).len(), 3);
        let rest = state.take_updates(Some(3), None);
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0]["message"]["text"], "c");
        assert!(state.take_updates(Some(4), None).is_empty());
    }

    #[test]
    fn bot_message_with_buttons_can_be_pressed() {
        let mut state = SimState::default();
        let markup = r#"{"inline_keyboard":[[{"text":"Да","callback_data":"yes"}]]}"#;
        let sent = state.call("sendMessage", &params(json!({ "chat_id": USER_ID, "text": "Записать?", "reply_markup": markup })), 0).unwrap();
        let message_id = sent["message_id"].as_i64().unwrap();
        let update = state.user_press(message_id, "yes", 10).unwrap();
        assert_eq!(update["callback_query"]["data"], "yes");
        let query_id = update["callback_query"]["id"].as_str().unwrap().to_string();
        let answered = state.call("answerCallbackQuery", &params(json!({ "callback_query_id": query_id, "text": "Готово" })), 20);
        assert!(answered.is_ok());
        assert_eq!(state.notice.as_ref().unwrap().text, "Готово");
        let again = state.call("answerCallbackQuery", &params(json!({ "callback_query_id": query_id })), 30);
        assert!(again.unwrap_err().description.contains("query is too old"));
    }

    #[test]
    fn telegram_limits_are_enforced() {
        let mut state = SimState::default();
        let empty = state.call("sendMessage", &params(json!({ "chat_id": USER_ID, "text": "  " })), 0);
        assert_eq!(empty.unwrap_err().description, "Bad Request: message text is empty");
        let long_data = format!(r#"{{"inline_keyboard":[[{{"text":"x","callback_data":"{}"}}]]}}"#, "a".repeat(65));
        let too_long = state.call("sendMessage", &params(json!({ "chat_id": USER_ID, "text": "x", "reply_markup": long_data })), 0);
        assert_eq!(too_long.unwrap_err().description, "Bad Request: BUTTON_DATA_INVALID");
        let bad_html = state.call("sendMessage", &params(json!({ "chat_id": USER_ID, "text": "a<br>b", "parse_mode": "HTML" })), 0);
        assert!(bad_html.unwrap_err().description.starts_with("Bad Request: can't parse entities: Unsupported start tag"));
        assert_eq!(state.log.back().unwrap().problem.as_deref(), Some("Bad Request: can't parse entities: Unsupported start tag \"br\" at byte offset 1"));
    }

    #[test]
    fn editing_to_the_same_text_is_not_modified() {
        let mut state = SimState::default();
        let sent = state.call("sendMessage", &params(json!({ "chat_id": USER_ID, "text": "1 из 3" })), 0).unwrap();
        let id = sent["message_id"].clone();
        let same = state.call("editMessageText", &params(json!({ "chat_id": USER_ID, "message_id": id, "text": "1 из 3" })), 0);
        assert!(same.unwrap_err().description.contains("message is not modified"));
        let changed = state.call("editMessageText", &params(json!({ "chat_id": USER_ID, "message_id": id, "text": "2 из 3" })), 0).unwrap();
        assert_eq!(changed["text"], "2 из 3");
        assert_eq!(state.messages.len(), 1);
        assert!(state.messages[0].edited);
    }

    #[test]
    fn messages_to_other_chats_do_not_appear_in_the_phone() {
        let mut state = SimState::default();
        let reply = state.call("sendMessage", &params(json!({ "chat_id": 555, "text": "Новая заявка" })), 0).unwrap();
        assert_eq!(reply["chat"]["id"], 555);
        assert!(state.messages.is_empty());
        assert!(state.log.iter().any(|entry| entry.method == "в другой чат"));
    }

    #[test]
    fn reply_keyboard_shows_and_hides() {
        let mut state = SimState::default();
        let keyboard = r#"{"keyboard":[["Записаться"],[{"text":"Цены"}]],"one_time_keyboard":true}"#;
        state.call("sendMessage", &params(json!({ "chat_id": USER_ID, "text": "Меню", "reply_markup": keyboard })), 0).unwrap();
        assert_eq!(state.keyboard, Some(vec![vec!["Записаться".to_string()], vec!["Цены".to_string()]]));
        state.user_text("Цены", 0);
        assert_eq!(state.keyboard, None);
    }

    #[test]
    fn uploaded_photo_can_be_sent_again_by_file_id() {
        let mut state = SimState::default();
        let mut request = params(json!({ "chat_id": USER_ID, "caption": "Френч" }));
        request.uploads.insert("photo".to_string(), Upload { bytes: vec![1, 2, 3], name: "a.jpg".to_string(), mime: "image/jpeg".to_string() });
        let sent = state.call("sendPhoto", &request, 0).unwrap();
        let file_id = sent["photo"][0]["file_id"].as_str().unwrap().to_string();
        let again = state.call("sendPhoto", &params(json!({ "chat_id": USER_ID, "photo": file_id })), 0).unwrap();
        assert_eq!(again["photo"][0]["file_id"], sent["photo"][0]["file_id"]);
        let missing = state.call("sendPhoto", &params(json!({ "chat_id": USER_ID, "photo": "nope" })), 0);
        assert_eq!(missing.unwrap_err().description, "Bad Request: wrong file identifier/HTTP URL specified");
        let file = state.call("getFile", &params(json!({ "file_id": file_id })), 0).unwrap();
        let path = file["file_path"].as_str().unwrap();
        assert_eq!(state.file_by_path(path).unwrap().bytes, vec![1, 2, 3]);
    }
}
