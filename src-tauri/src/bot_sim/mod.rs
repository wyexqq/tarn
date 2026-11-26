mod api;
mod http;
mod text;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

use crate::providers::ToolSpec;
use crate::tools::{spec, ToolRisk};
use api::{ApiError, Params, SimMessage, SimState, Upload};
pub use api::StoredFile;
pub use text::TextRun;

pub const TOOL_BOT_SIMULATOR: &str = "bot_simulator";
pub const CHANGED_EVENT: &str = "bot-sim-changed";
const HOST: &str = "127.0.0.1";
const PREFERRED_PORT: u16 = 8081;
const SIMULATOR_SERVER: &str = "simulator";
const MAX_POLL_SECONDS: i64 = 50;
const CONNECTED_WINDOW_MS: i64 = 65_000;
const QUIET_AFTER_REPLY_MS: i64 = 800;
const LONGEST_WAIT: Duration = Duration::from_secs(6);
const CHECK_EVERY: Duration = Duration::from_millis(150);
const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PHOTO_BYTES: u64 = 10 * 1024 * 1024;
const REPORT_MESSAGES: usize = 15;
const REPORT_LOG: usize = 12;

const TOOL_DESCRIPTION: &str = "Локальный симулятор Telegram для проверки настоящего бота пользователя. \
start запускает на этом компьютере поддельный сервер Bot API и показывает пользователю телефон с чатом прямо в твоём ответе. \
Бот должен ходить на адрес симулятора вместо api.telegram.org, токен любой вида 123456:ABC. \
aiogram 3: Bot(token, session=AiohttpSession(api=TelegramAPIServer.from_base(адрес))). \
python-telegram-bot: Application.builder().token(token).base_url(адрес + \"/bot\").base_file_url(адрес + \"/file/bot\"). \
pyTelegramBotAPI: telebot.apihelper.API_URL = адрес + \"/bot{0}/{1}\". \
grammY: new Bot(token, { client: { apiRoot: адрес } }). Telegraf: new Telegraf(token, { telegram: { apiRoot: адрес } }). \
Работают getUpdates и вебхук на локальный адрес. Симулятор отвечает ошибками как настоящий Telegram: неверная разметка, слишком длинный текст, message is not modified, просроченный callback. \
send пишет боту текст от лица пользователя, press нажимает кнопку по её тексту, read показывает переписку и запросы бота с ошибками, reset очищает чат. \
После send и press инструмент ждёт ответа бота до 6 секунд и возвращает, что изменилось.";

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

type ChangeNotifier = Arc<dyn Fn(SimView) + Send + Sync>;

fn window_notifier(app: &AppHandle) -> ChangeNotifier {
    let app = app.clone();
    Arc::new(move |view| {
        let _ = app.emit(CHANGED_EVENT, view);
    })
}

#[derive(Default)]
struct Shared {
    state: Mutex<SimState>,
    updates_arrived: Notify,
    notifier: Mutex<Option<ChangeNotifier>>,
    running: AtomicBool,
    webhook_client: OnceLock<reqwest::Client>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, SimState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn notify_changed(&self, address: Option<String>) {
        let notifier = self.notifier.lock().ok().and_then(|notifier| notifier.clone());
        if let Some(notifier) = notifier {
            let view = view_of(&self.lock(), address, self.is_running());
            notifier(view);
        }
    }

    fn webhook_client(&self) -> &reqwest::Client {
        self.webhook_client.get_or_init(|| {
            reqwest::Client::builder()
                .no_proxy()
                .timeout(WEBHOOK_TIMEOUT)
                .build()
                .unwrap_or_default()
        })
    }
}

struct RunningServer {
    port: u16,
    accept_task: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Default)]
pub struct BotSimulator {
    shared: Arc<Shared>,
    server: Mutex<Option<RunningServer>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MediaView {
    kind: String,
    file_id: String,
    name: String,
    url: Option<String>,
    has_bytes: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ButtonView {
    text: String,
    data: Option<String>,
    url: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    id: i64,
    from_bot: bool,
    runs: Vec<TextRun>,
    media: Option<MediaView>,
    buttons: Vec<Vec<ButtonView>>,
    date: i64,
    edited: bool,
    forwarded: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LogView {
    serial: u64,
    at: i64,
    method: String,
    detail: String,
    problem: Option<String>,
    from_user: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NoticeView {
    serial: u64,
    text: String,
    alert: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CommandView {
    command: String,
    description: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SimView {
    running: bool,
    address: Option<String>,
    last_call_at: Option<i64>,
    webhook: Option<String>,
    pending: usize,
    messages: Vec<MessageView>,
    keyboard: Option<Vec<Vec<String>>>,
    commands: Vec<CommandView>,
    typing_until: i64,
    notice: Option<NoticeView>,
    log: Vec<LogView>,
}

fn buttons_of(rows: Option<&Value>) -> Vec<Vec<ButtonView>> {
    let Some(rows) = rows.and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(Value::as_array)
        .map(|row| {
            row.iter()
                .map(|button| ButtonView {
                    text: button["text"].as_str().unwrap_or_default().to_string(),
                    data: button["callback_data"].as_str().map(str::to_string),
                    url: button["url"].as_str().map(str::to_string),
                })
                .collect()
        })
        .collect()
}

fn message_view(state: &SimState, message: &SimMessage) -> MessageView {
    let media = message.media.as_ref().map(|media| {
        let file = state.file(&media.file_id);
        MediaView {
            kind: media.kind.clone(),
            file_id: media.file_id.clone(),
            name: media.name.clone(),
            url: file.as_ref().and_then(|file| file.url.clone()),
            has_bytes: file.is_some_and(|file| !file.bytes.is_empty()),
        }
    });
    MessageView {
        id: message.id,
        from_bot: message.from_bot,
        runs: message.runs.clone(),
        media,
        buttons: buttons_of(message.inline_keyboard.as_ref()),
        date: message.date,
        edited: message.edited,
        forwarded: message.forwarded,
    }
}

fn view_of(state: &SimState, address: Option<String>, running: bool) -> SimView {
    SimView {
        running,
        address,
        last_call_at: state.last_bot_call_at,
        webhook: state.webhook.as_ref().map(|hook| hook.url.clone()),
        pending: state.pending_count(),
        messages: state.messages.iter().map(|message| message_view(state, message)).collect(),
        keyboard: state.keyboard.clone(),
        commands: state.commands.iter().map(|command| CommandView { command: command.command.clone(), description: command.description.clone() }).collect(),
        typing_until: state.typing_until,
        notice: state.notice.as_ref().map(|notice| NoticeView { serial: notice.serial, text: notice.text.clone(), alert: notice.alert }),
        log: state
            .log
            .iter()
            .map(|entry| LogView {
                serial: entry.serial,
                at: entry.at,
                method: entry.method.clone(),
                detail: entry.detail.clone(),
                problem: entry.problem.clone(),
                from_user: entry.from_user,
            })
            .collect(),
    }
}

struct Reply {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

fn json_reply(outcome: Result<Value, ApiError>) -> Reply {
    let (status, body) = match outcome {
        Ok(result) => (200, json!({ "ok": true, "result": result })),
        Err(error) => (error.code, json!({ "ok": false, "error_code": error.code, "description": error.description })),
    };
    Reply { status, content_type: "application/json".to_string(), body: body.to_string().into_bytes() }
}

fn not_found_reply() -> Reply {
    json_reply(Err(ApiError { code: 404, description: "Not Found".to_string() }))
}

fn file_reply(shared: &Shared, path: &str) -> Reply {
    let file_path = path.split_once('/').map(|(_, file)| file).unwrap_or_default();
    match shared.lock().file_by_path(&http::percent_decode(file_path)) {
        Some(file) if !file.bytes.is_empty() => Reply {
            status: 200,
            content_type: if file.mime.is_empty() { "application/octet-stream".to_string() } else { file.mime },
            body: file.bytes,
        },
        _ => not_found_reply(),
    }
}

fn was_connected(state: &SimState, now: i64) -> bool {
    state.last_bot_call_at.is_some_and(|at| now - at <= CONNECTED_WINDOW_MS)
}

async fn get_updates(shared: &Shared, params: &Params, address: Option<String>) -> Result<Value, ApiError> {
    let wait_seconds = params.int("timeout").unwrap_or(0).clamp(0, MAX_POLL_SECONDS) as u64;
    let deadline = Instant::now() + Duration::from_secs(wait_seconds);
    loop {
        let mut arrived = std::pin::pin!(shared.updates_arrived.notified());
        arrived.as_mut().enable();
        let now = now_millis();
        let (updates, has_webhook, just_connected) = {
            let mut state = shared.lock();
            let just_connected = !was_connected(&state, now);
            state.last_bot_call_at = Some(now);
            let updates = state.take_updates(params.int("offset"), params.int("limit"));
            (updates, state.webhook.is_some(), just_connected)
        };
        if just_connected {
            shared.notify_changed(address.clone());
        }
        if has_webhook {
            return Err(ApiError {
                code: 409,
                description: "Conflict: can't use getUpdates method while webhook is active; use deleteWebhook to delete the webhook first".to_string(),
            });
        }
        let time_left = deadline.saturating_duration_since(Instant::now());
        if !updates.is_empty() || time_left.is_zero() || !shared.is_running() {
            return Ok(Value::Array(updates));
        }
        let _ = tokio::time::timeout(time_left, arrived).await;
    }
}

async fn handle_request(shared: &Shared, request: http::Request, address: Option<String>) -> Reply {
    let path = request.path.trim_start_matches('/');
    if let Some(rest) = path.strip_prefix("file/bot") {
        return file_reply(shared, rest);
    }
    let Some((_, method)) = path.strip_prefix("bot").and_then(|rest| rest.split_once('/')) else {
        return not_found_reply();
    };
    let method = method.trim_end_matches('/');
    let params = match http::params_of(&request) {
        Ok(params) => params,
        Err(problem) => return json_reply(Err(ApiError { code: 400, description: format!("Bad Request: {problem}") })),
    };
    if method.eq_ignore_ascii_case("getUpdates") {
        return json_reply(get_updates(shared, &params, address).await);
    }
    let outcome = shared.lock().call(method, &params, now_millis());
    shared.notify_changed(address);
    json_reply(outcome)
}

async fn serve_connection(shared: Arc<Shared>, mut stream: TcpStream, address: Option<String>) {
    let mut buffer = Vec::new();
    while shared.is_running() {
        let Ok(Some(request)) = http::read_request(&mut stream, &mut buffer).await else {
            return;
        };
        let keep_alive = !request.wants_close();
        let reply = handle_request(&shared, request, address.clone()).await;
        let bytes = http::response(reply.status, &reply.content_type, &reply.body, keep_alive);
        if stream.write_all(&bytes).await.is_err() || !keep_alive {
            return;
        }
    }
}

async fn accept_loop(shared: Arc<Shared>, listener: TcpListener, address: String) {
    while let Ok((stream, _)) = listener.accept().await {
        let _ = stream.set_nodelay(true);
        tauri::async_runtime::spawn(serve_connection(shared.clone(), stream, Some(address.clone())));
    }
}

fn params_from_object(object: &Value) -> Params {
    let fields = object.as_object().map(|map| map.clone().into_iter().collect()).unwrap_or_default();
    Params { fields, uploads: Default::default() }
}

async fn post_webhook(shared: Arc<Shared>, url: String, secret: Option<String>, update: Value, address: Option<String>) {
    let mut request = shared.webhook_client().post(&url).json(&update);
    if let Some(secret) = secret {
        request = request.header("X-Telegram-Bot-Api-Secret-Token", secret);
    }
    let now = now_millis();
    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.bytes().await.unwrap_or_default();
            let answer: Option<Value> = serde_json::from_slice(&body).ok().filter(|value: &Value| value["method"].is_string());
            let mut state = shared.lock();
            state.last_bot_call_at = Some(now);
            let problem = (status >= 300).then(|| format!("вебхук ответил {status}"));
            state.add_log("webhook", status.to_string(), problem, false, now);
            if let Some(answer) = answer {
                let method = answer["method"].as_str().unwrap_or_default().to_string();
                let _ = state.call(&method, &params_from_object(&answer), now);
            }
        }
        Err(error) => {
            let problem = format!("вебхук недоступен: {error}");
            shared.lock().add_log("webhook", url, Some(problem), false, now);
        }
    }
    shared.notify_changed(address);
}

fn mime_of_image(path: &Path) -> &'static str {
    let extension = path.extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default();
    match extension.as_str() {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
}

fn plain_text_of(runs: &[TextRun]) -> String {
    runs.iter().map(|run| run.text.as_str()).collect()
}

fn describe_message(message: &MessageView) -> String {
    let who = if message.from_bot { "бот" } else { "пользователь" };
    let mut line = format!("[{who} #{}]", message.id);
    if message.edited {
        line.push_str(" (изменено)");
    }
    if let Some(media) = &message.media {
        line.push_str(&format!(" [{} {}]", media.kind, media.name));
    }
    let text = plain_text_of(&message.runs);
    if !text.is_empty() {
        line.push(' ');
        line.push_str(&text);
    }
    let labels: Vec<String> = message.buttons.iter().flatten().map(|button| format!("[{}]", button.text)).collect();
    if !labels.is_empty() {
        line.push_str(&format!("\n  кнопки: {}", labels.join(" ")));
    }
    line
}

fn describe_log(entry: &LogView) -> String {
    let outcome = match &entry.problem {
        Some(problem) => format!(" → ошибка: {problem}"),
        None => String::new(),
    };
    let who = if entry.from_user { "пользователь" } else { "бот" };
    format!("{who}: {} {}{outcome}", entry.method, entry.detail)
}

fn connection_line(view: &SimView, now: i64) -> String {
    let Some(address) = &view.address else {
        return "Симулятор выключен. Вызови action start".to_string();
    };
    if let Some(webhook) = &view.webhook {
        return format!("Симулятор {address}, бот на вебхуке {webhook}");
    }
    match view.last_call_at {
        Some(at) if now - at <= CONNECTED_WINDOW_MS => format!("Симулятор {address}, бот подключён"),
        Some(_) => format!("Симулятор {address}, бот давно не обращался: возможно, он остановлен"),
        None => format!("Симулятор {address}, бот ещё ни разу не подключался"),
    }
}

struct Mark {
    serial: u64,
}

impl BotSimulator {
    fn address(&self) -> Option<String> {
        let server = self.server.lock().ok()?;
        server.as_ref().map(|running| format!("http://{HOST}:{}", running.port))
    }

    fn changed(&self) {
        self.shared.notify_changed(self.address());
    }

    pub fn view(&self) -> SimView {
        view_of(&self.shared.lock(), self.address(), self.shared.is_running())
    }

    pub fn file(&self, id: &str) -> Option<StoredFile> {
        self.shared.lock().file(id)
    }

    pub async fn start(&self, app: &AppHandle) -> Result<String, String> {
        if let Some(address) = self.address() {
            return Ok(address);
        }
        let listener = match TcpListener::bind((HOST, PREFERRED_PORT)).await {
            Ok(listener) => listener,
            Err(_) => TcpListener::bind((HOST, 0)).await.map_err(|error| format!("Не удалось открыть порт: {error}"))?,
        };
        let port = listener.local_addr().map_err(|error| error.to_string())?.port();
        let address = format!("http://{HOST}:{port}");
        {
            let mut server = self.server.lock().map_err(|_| "симулятор недоступен".to_string())?;
            if let Some(running) = server.as_ref() {
                return Ok(format!("http://{HOST}:{}", running.port));
            }
            if let Ok(mut notifier) = self.shared.notifier.lock() {
                *notifier = Some(window_notifier(app));
            }
            self.shared.running.store(true, Ordering::SeqCst);
            let accept_task = tauri::async_runtime::spawn(accept_loop(self.shared.clone(), listener, address.clone()));
            *server = Some(RunningServer { port, accept_task });
        }
        self.changed();
        Ok(address)
    }

    pub fn stop(&self) {
        let running = self.server.lock().ok().and_then(|mut server| server.take());
        if let Some(running) = running {
            running.accept_task.abort();
        }
        self.shared.running.store(false, Ordering::SeqCst);
        self.shared.updates_arrived.notify_waiters();
        self.changed();
    }

    fn deliver(&self, update: Value) {
        let webhook = self.shared.lock().webhook.as_ref().map(|hook| (hook.url.clone(), hook.secret.clone()));
        match webhook {
            Some((url, secret)) => {
                tauri::async_runtime::spawn(post_webhook(self.shared.clone(), url, secret, update, self.address()));
            }
            None => {
                self.shared.lock().queue(update);
                self.shared.updates_arrived.notify_waiters();
            }
        }
        self.changed();
    }

    pub fn send_text(&self, text: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Пустое сообщение".to_string());
        }
        let update = self.shared.lock().user_text(text, now_millis());
        self.deliver(update);
        Ok(())
    }

    pub fn press(&self, message_id: i64, data: &str) -> Result<(), String> {
        let update = self.shared.lock().user_press(message_id, data, now_millis())?;
        self.deliver(update);
        Ok(())
    }

    pub fn send_photo(&self, path: &Path, caption: &str) -> Result<(), String> {
        let size = std::fs::metadata(path).map_err(|error| format!("Файл не открылся: {error}"))?.len();
        if size > MAX_PHOTO_BYTES {
            return Err("Фото больше 10 МБ".to_string());
        }
        let bytes = std::fs::read(path).map_err(|error| format!("Файл не прочитался: {error}"))?;
        let name = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_else(|| "photo.jpg".to_string());
        let upload = Upload { bytes, name, mime: mime_of_image(path).to_string() };
        let update = self.shared.lock().user_photo(upload, caption.trim(), now_millis());
        self.deliver(update);
        Ok(())
    }

    pub fn reset(&self) {
        self.shared.lock().reset();
        self.changed();
    }

    fn mark(&self) -> Mark {
        Mark { serial: self.shared.lock().serial }
    }

    fn bot_finished_answering(&self, mark: &Mark) -> bool {
        let state = self.shared.lock();
        let now = now_millis();
        let last_answer = state.log.iter().filter(|entry| entry.serial > mark.serial && !entry.from_user).map(|entry| entry.at).max();
        let is_typing = state.typing_until > now;
        last_answer.is_some_and(|at| now - at >= QUIET_AFTER_REPLY_MS) && !is_typing
    }

    async fn wait_for_bot(&self, mark: &Mark) {
        let started = Instant::now();
        while started.elapsed() < LONGEST_WAIT {
            tokio::time::sleep(CHECK_EVERY).await;
            if self.bot_finished_answering(mark) {
                return;
            }
        }
    }

    fn report_since(&self, mark: &Mark) -> String {
        let state = self.shared.lock();
        let now = now_millis();
        let view = view_of(&state, self.address(), self.shared.is_running());
        let changed: Vec<String> = state
            .messages
            .iter()
            .filter(|message| message.touched > mark.serial && message.from_bot)
            .map(|message| describe_message(&message_view(&state, message)))
            .collect();
        let calls: Vec<String> = view.log.iter().filter(|entry| entry.serial > mark.serial && !entry.from_user).map(describe_log).collect();
        let mut report = vec![connection_line(&view, now)];
        if changed.is_empty() && calls.is_empty() {
            report.push(format!("Бот ничего не ответил за {} с", LONGEST_WAIT.as_secs()));
        }
        if !changed.is_empty() {
            report.push(format!("Новые и изменённые сообщения бота:\n{}", changed.join("\n")));
        }
        if !calls.is_empty() {
            report.push(format!("Запросы бота:\n{}", calls.join("\n")));
        }
        if let Some(keyboard) = &view.keyboard {
            let labels: Vec<String> = keyboard.iter().flatten().map(|label| format!("[{label}]")).collect();
            report.push(format!("Клавиатура: {}", labels.join(" ")));
        }
        if let Some(notice) = &view.notice {
            report.push(format!("Последнее всплывающее уведомление: {}", notice.text));
        }
        report.join("\n\n")
    }

    fn report_all(&self) -> String {
        let view = self.view();
        let now = now_millis();
        let messages: Vec<String> = view.messages.iter().rev().take(REPORT_MESSAGES).rev().map(describe_message).collect();
        let calls: Vec<String> = view.log.iter().rev().take(REPORT_LOG).rev().map(describe_log).collect();
        let mut report = vec![connection_line(&view, now)];
        report.push(if messages.is_empty() { "Чат пуст".to_string() } else { format!("Переписка:\n{}", messages.join("\n")) });
        if !calls.is_empty() {
            report.push(format!("Последние события:\n{}", calls.join("\n")));
        }
        if view.pending > 0 {
            report.push(format!("Бот ещё не забрал сообщений: {}", view.pending));
        }
        report.join("\n\n")
    }

    fn find_button(&self, label: &str) -> Result<ButtonTarget, String> {
        let wanted = label.trim().to_lowercase();
        let view = self.view();
        for message in view.messages.iter().rev().filter(|message| message.from_bot) {
            for button in message.buttons.iter().flatten() {
                if button.text.trim().to_lowercase() != wanted {
                    continue;
                }
                return match (&button.data, &button.url) {
                    (Some(data), _) => Ok(ButtonTarget::Callback { message_id: message.id, data: data.clone() }),
                    (None, Some(url)) => Err(format!("Это кнопка-ссылка на {url}, бот её не получает")),
                    _ => Err("Эта кнопка ничего не отправляет боту".to_string()),
                };
            }
        }
        let on_keyboard = view.keyboard.iter().flatten().flatten().any(|text| text.trim().to_lowercase() == wanted);
        if on_keyboard {
            return Ok(ButtonTarget::Keyboard(label.trim().to_string()));
        }
        Err(format!("Кнопки «{label}» нет в сообщениях бота и на клавиатуре"))
    }

    pub async fn run_tool(&self, app: &AppHandle, input: &Value) -> Result<String, String> {
        let action = input["action"].as_str().unwrap_or_default();
        if action == "read" {
            return Ok(self.report_all());
        }
        let address = self.start(app).await?;
        match action {
            "start" => Ok(format!(
                "Симулятор работает: {address}. Пользователь видит телефон с чатом в твоём ответе.\n\n{}",
                self.report_all()
            )),
            "reset" => {
                self.reset();
                Ok("Чат очищен".to_string())
            }
            "send" => {
                let text = input["text"].as_str().unwrap_or_default();
                let mark = self.mark();
                self.send_text(text)?;
                self.wait_for_bot(&mark).await;
                Ok(self.report_since(&mark))
            }
            "press" => {
                let label = input["button"].as_str().unwrap_or_default();
                let target = self.find_button(label)?;
                let mark = self.mark();
                match target {
                    ButtonTarget::Callback { message_id, data } => self.press(message_id, &data)?,
                    ButtonTarget::Keyboard(text) => self.send_text(&text)?,
                }
                self.wait_for_bot(&mark).await;
                Ok(self.report_since(&mark))
            }
            other => Err(format!("Неизвестное действие «{other}». Можно: start, send, press, read, reset")),
        }
    }
}

enum ButtonTarget {
    Callback { message_id: i64, data: String },
    Keyboard(String),
}

pub fn tool_spec() -> ToolSpec {
    spec(
        TOOL_BOT_SIMULATOR,
        TOOL_DESCRIPTION,
        json!({ "type": "object", "properties": {
            "action": { "type": "string", "enum": ["start", "send", "press", "read", "reset"] },
            "text": { "type": "string", "description": "Для send: текст от пользователя, например /start" },
            "button": { "type": "string", "description": "Для press: текст кнопки" }
        }, "required": ["action"] }),
    )
}

pub fn risk_of(input: &Value) -> ToolRisk {
    if input["action"].as_str() == Some("read") {
        ToolRisk::Read
    } else {
        ToolRisk::External { server: SIMULATOR_SERVER.to_string(), read_only: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_lists_buttons_and_errors() {
        let mut state = SimState::default();
        let markup = r#"{"inline_keyboard":[[{"text":"Записаться","callback_data":"book"}]]}"#;
        let fields = json!({ "chat_id": api::USER_ID, "text": "Привет", "reply_markup": markup });
        state.call("sendMessage", &params_from_object(&fields), 0).unwrap();
        let broken = json!({ "chat_id": api::USER_ID, "text": "<br>", "parse_mode": "HTML" });
        let _ = state.call("sendMessage", &params_from_object(&broken), 0);
        let view = view_of(&state, Some("http://127.0.0.1:8081".to_string()), true);
        let message = describe_message(&view.messages[0]);
        assert!(message.contains("Привет"));
        assert!(message.contains("[Записаться]"));
        let failed = describe_log(view.log.last().unwrap());
        assert!(failed.contains("ошибка: Bad Request: can't parse entities"));
    }

    #[tokio::test]
    async fn bot_talks_to_the_simulator_over_http() {
        let shared = Arc::new(Shared::default());
        shared.running.store(true, Ordering::SeqCst);
        let listener = TcpListener::bind((HOST, 0)).await.unwrap();
        let address = format!("http://{HOST}:{}", listener.local_addr().unwrap().port());
        tokio::spawn(accept_loop(shared.clone(), listener, address.clone()));
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let poll = tokio::spawn(client.get(format!("{address}/bot1:abc/getUpdates?timeout=5")).send());
        tokio::time::sleep(Duration::from_millis(200)).await;
        let update = shared.lock().user_text("/start", now_millis());
        shared.lock().queue(update);
        shared.updates_arrived.notify_waiters();
        let polled: Value = poll.await.unwrap().unwrap().json().await.unwrap();
        assert_eq!(polled["ok"], true);
        assert_eq!(polled["result"][0]["message"]["text"], "/start");

        let sent = client
            .post(format!("{address}/bot1:abc/sendMessage"))
            .json(&json!({ "chat_id": api::USER_ID, "text": "Привет" }))
            .send()
            .await
            .unwrap();
        assert_eq!(sent.status(), 200);
        assert_eq!(shared.lock().messages.iter().filter(|message| message.from_bot).count(), 1);

        let broken = client
            .post(format!("{address}/bot1:abc/sendMessage"))
            .form(&[("chat_id", "100001"), ("text", "a<br>b"), ("parse_mode", "HTML")])
            .send()
            .await
            .unwrap();
        assert_eq!(broken.status(), 400);
        let answer: Value = broken.json().await.unwrap();
        assert_eq!(answer["ok"], false);
        assert!(answer["description"].as_str().unwrap().contains("Unsupported start tag"));
    }

    #[test]
    fn read_needs_no_permission_but_actions_do() {
        assert!(matches!(risk_of(&json!({ "action": "read" })), ToolRisk::Read));
        assert!(matches!(risk_of(&json!({ "action": "send" })), ToolRisk::External { .. }));
    }
}
