use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tokio::sync::{oneshot, Notify};

use crate::background::BackgroundTasks;
use crate::bot_sim::BotSimulator;
use crate::browser::Browser;
use crate::db::Database;
use crate::keep_awake::KeepAwake;
use crate::mcp::McpPool;
use crate::network::HttpClient;
use crate::user_terminal::UserTerminals;
use crate::widget_pages::WidgetPages;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolAnswer {
    Once,
    InThisChat,
    Deny,
}

pub struct AppState {
    pub db: Mutex<Database>,
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
    pub http: HttpClient,
    pub cancellations: Mutex<HashMap<String, Arc<Notify>>>,
    pub import_running: Arc<AtomicBool>,
    pub voice_installing: AtomicBool,
    pub speech: crate::voice::SpeechService,
    pub mcp: McpPool,
    pub keep_awake: KeepAwake,
    pub background: BackgroundTasks,
    pub terminals: UserTerminals,
    pub browser: Browser,
    pub widget_pages: WidgetPages,
    pub bot_simulator: BotSimulator,
    waiting_tools: Mutex<HashMap<String, oneshot::Sender<ToolAnswer>>>,
    allowed_in_chat: Mutex<HashSet<(String, String)>>,
}

fn waiting_tool_key(message_id: &str, call_id: &str) -> String {
    format!("{message_id}:{call_id}")
}

impl AppState {
    pub fn open(data_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let db_path = data_dir.join("tarn.db");
        let db = Database::open(&db_path)?;
        db.mark_interrupted_messages()?;
        let keep_awake = KeepAwake::start(db.get_setting("keepAwake").as_deref() != Some("false"));
        Ok(Self {
            db: Mutex::new(db),
            db_path,
            data_dir: data_dir.to_path_buf(),
            http: HttpClient::new()?,
            cancellations: Mutex::new(HashMap::new()),
            import_running: Arc::new(AtomicBool::new(false)),
            voice_installing: AtomicBool::new(false),
            speech: crate::voice::SpeechService::default(),
            mcp: McpPool::default(),
            keep_awake,
            background: BackgroundTasks::default(),
            terminals: UserTerminals::default(),
            browser: Browser::default(),
            widget_pages: WidgetPages::default(),
            bot_simulator: BotSimulator::default(),
            waiting_tools: Mutex::new(HashMap::new()),
            allowed_in_chat: Mutex::new(HashSet::new()),
        })
    }

    pub fn with_db<T>(&self, action: impl FnOnce(&Database) -> Result<T, String>) -> Result<T, String> {
        let guard = self.db.lock().map_err(|_| "база данных недоступна".to_string())?;
        action(&guard)
    }

    pub fn register_cancellation(&self, message_id: &str) -> Arc<Notify> {
        let notify = Arc::new(Notify::new());
        if let Ok(mut map) = self.cancellations.lock() {
            map.insert(message_id.to_string(), notify.clone());
        }
        notify
    }

    pub fn drop_cancellation(&self, message_id: &str) {
        if let Ok(mut map) = self.cancellations.lock() {
            map.remove(message_id);
        }
    }

    pub fn add_waiting_tool(&self, message_id: &str, call_id: &str, sender: oneshot::Sender<ToolAnswer>) {
        if let Ok(mut waiting) = self.waiting_tools.lock() {
            waiting.insert(waiting_tool_key(message_id, call_id), sender);
        }
    }

    pub fn remove_waiting_tool(&self, message_id: &str, call_id: &str) {
        if let Ok(mut waiting) = self.waiting_tools.lock() {
            waiting.remove(&waiting_tool_key(message_id, call_id));
        }
    }

    pub fn answer_waiting_tool(&self, message_id: &str, call_id: &str, answer: ToolAnswer) -> bool {
        let sender = self
            .waiting_tools
            .lock()
            .ok()
            .and_then(|mut waiting| waiting.remove(&waiting_tool_key(message_id, call_id)));
        sender.is_some_and(|sender| sender.send(answer).is_ok())
    }

    pub fn is_allowed_in_chat(&self, chat_id: &str, scope: &str) -> bool {
        self.allowed_in_chat
            .lock()
            .is_ok_and(|allowed| allowed.contains(&(chat_id.to_string(), scope.to_string())))
    }

    pub fn allow_in_chat(&self, chat_id: &str, scope: &str) {
        if let Ok(mut allowed) = self.allowed_in_chat.lock() {
            allowed.insert((chat_id.to_string(), scope.to_string()));
        }
    }

    pub fn forget_chat_permissions(&self, chat_id: &str) {
        if let Ok(mut allowed) = self.allowed_in_chat.lock() {
            allowed.retain(|(allowed_chat, _)| allowed_chat != chat_id);
        }
    }

    pub fn stop_everything_started(&self) {
        self.background.stop_all_now();
        self.terminals.stop_all_now();
        self.bot_simulator.stop();
    }

    pub fn cancel(&self, message_id: &str) {
        if let Ok(map) = self.cancellations.lock() {
            if let Some(notify) = map.get(message_id) {
                notify.notify_one();
            }
        }
    }
}
