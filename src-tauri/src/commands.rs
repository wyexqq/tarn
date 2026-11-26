use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use base64::Engine;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::chat::{self, find_provider, GenerationRequest};
use crate::claude_code_export;
use crate::compaction::ContextUsage;
use crate::db::{Database, NewMessage};
use crate::background::TaskSummary;
use crate::bot_sim::SimView;
use crate::browser::{BrowserView, PanelBounds};
use crate::export;
use crate::file_panel::{self, ChangedFile, FileView, FolderEntry};
use crate::importer;
use crate::keys;
use crate::memory::{self, MemoryFile, OwnMemory};
use crate::panel_tools;
use crate::providers::{self, Protocol, ProviderConfig};
use crate::skills::{self, McpServerConfig, Plugin, Skill};
use crate::state::{AppState, ToolAnswer};
use crate::tools::AccessMode;
use crate::user_terminal::{TerminalEvent, TerminalNotifier, TerminalView};
use crate::types::*;
use crate::voice::{self, VoiceStatus};

type AppStateHandle<'a> = State<'a, Arc<AppState>>;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoClaudeCodeReplace {
    file_path: String,
    backup_path: String,
    written_hash: String,
}

const MAX_TEXT_ATTACHMENT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_IMAGE_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn hotkey_enabled(app: &AppHandle) -> bool {
    let state = app.state::<Arc<AppState>>();
    state
        .with_db(|db| Ok(db.get_setting("hotkeyEnabled")))
        .ok()
        .flatten()
        .map(|value| value != "false")
        .unwrap_or(true)
}

pub fn close_to_tray_enabled(app: &AppHandle) -> bool {
    let state = app.state::<Arc<AppState>>();
    state
        .with_db(|db| Ok(db.get_setting("closeToTray")))
        .ok()
        .flatten()
        .is_some_and(|value| value == "true")
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
    let window = app.get_window("main").ok_or("окно не найдено")?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_chats(state: AppStateHandle) -> Result<Vec<ChatSummary>, String> {
    state.with_db(|db| db.list_chats())
}

#[tauri::command]
pub fn get_thread(state: AppStateHandle, chat_id: String, before_id: Option<String>, limit: i64, ensure_id: Option<String>) -> Result<ThreadPage, String> {
    state.with_db(|db| db.thread_page(&chat_id, before_id.as_deref(), limit, ensure_id.as_deref()))
}

#[tauri::command]
pub fn rename_chat(state: AppStateHandle, chat_id: String, title: String) -> Result<(), String> {
    state.with_db(|db| db.rename_chat(&chat_id, title.trim()))
}

#[tauri::command]
pub fn set_chat_pinned(state: AppStateHandle, chat_id: String, pinned: bool) -> Result<(), String> {
    state.with_db(|db| db.set_pinned(&chat_id, pinned))
}

#[tauri::command]
pub fn delete_chat(state: AppStateHandle, chat_id: String) -> Result<(), String> {
    let artifacts = state.with_db(|db| db.artifacts_of(&chat_id))?;
    let own_copies = panel_tools::artifacts_folder(&state.data_dir);
    for artifact in artifacts.iter().filter(|artifact| Path::new(&artifact.path).starts_with(&own_copies)) {
        let _ = std::fs::remove_file(&artifact.path);
    }
    state.with_db(|db| db.delete_chat(&chat_id))
}

#[tauri::command]
pub fn switch_branch(state: AppStateHandle, chat_id: String, message_id: String) -> Result<(), String> {
    state.with_db(|db| {
        let leaf = db.deepest_descendant(&message_id)?;
        db.set_active_leaf(&chat_id, &leaf, None)
    })
}

fn insert_assistant_placeholders(
    db: &crate::db::Database,
    chat_id: &str,
    parent_id: &str,
    model_refs: &[String],
) -> Result<Vec<Message>, String> {
    let group = if model_refs.len() > 1 { Some(new_id()) } else { None };
    let empty_attachments = json!([]);
    let mut created = Vec::new();
    for model_ref in model_refs {
        let (provider, model) = providers::split_model_ref(model_ref)
            .ok_or_else(|| format!("неверная модель: {model_ref}"))?;
        let id = new_id();
        db.insert_message(&NewMessage {
            id: &id,
            chat_id,
            parent_id: Some(parent_id),
            role: "assistant",
            content: "",
            attachments: &empty_attachments,
            provider: Some(provider),
            model: Some(model),
            status: "streaming",
            compare_group: group.as_deref(),
        })?;
        created.push(db.get_message(&id)?);
    }
    if let Some(first) = created.first() {
        db.set_active_leaf(chat_id, &first.id, model_refs.first().map(String::as_str))?;
    }
    Ok(created)
}

fn start_generations(app: &AppHandle, messages: &[Message], model_refs: &[String], parent_id: &str) {
    let allow_computer = model_refs.len() == 1;
    for (message, model_ref) in messages.iter().zip(model_refs) {
        chat::spawn_generation(
            app.clone(),
            GenerationRequest {
                message_id: message.id.clone(),
                chat_id: message.chat_id.clone(),
                model_ref: model_ref.clone(),
                parent_id: parent_id.to_string(),
                allow_computer,
            },
        );
    }
}

fn save_chat_access(db: &Database, chat_id: &str, access_mode: &str, folder: Option<&str>) -> Result<(), String> {
    let mode = AccessMode::parse(access_mode);
    if mode != AccessMode::Chat {
        let chosen_folder = match folder {
            Some(path) => Some(path.to_string()),
            None => db.get_chat(chat_id)?.cwd,
        };
        let folder_exists = chosen_folder.is_some_and(|path| Path::new(&path).is_dir());
        if !folder_exists {
            return Err("Выбери существующую папку для этого режима".to_string());
        }
    }
    db.set_chat_access(chat_id, access_mode, folder)
}

#[tauri::command]
pub fn set_chat_access(state: AppStateHandle, chat_id: String, access_mode: String, folder: Option<String>) -> Result<ChatSummary, String> {
    state.forget_chat_permissions(&chat_id);
    state.with_db(|db| {
        save_chat_access(db, &chat_id, &access_mode, folder.as_deref())?;
        db.get_chat(&chat_id)
    })
}

#[tauri::command]
pub fn answer_tool_request(state: AppStateHandle, message_id: String, call_id: String, answer: String) -> Result<(), String> {
    let parsed = match answer.as_str() {
        "once" => ToolAnswer::Once,
        "chat" => ToolAnswer::InThisChat,
        _ => ToolAnswer::Deny,
    };
    if state.answer_waiting_tool(&message_id, &call_id, parsed) {
        Ok(())
    } else {
        Err("Этот запрос уже неактуален".to_string())
    }
}

#[tauri::command]
pub fn list_skills(state: AppStateHandle) -> Result<Vec<Skill>, String> {
    state.with_db(|db| Ok(skills::list_skills(db)))
}

#[tauri::command]
pub fn set_skill_enabled(state: AppStateHandle, id: String, enabled: bool) -> Result<(), String> {
    state.with_db(|db| skills::set_skill_enabled(db, &id, enabled))
}

#[tauri::command]
pub fn add_skill_folder(state: AppStateHandle, folder: String) -> Result<usize, String> {
    state.with_db(|db| skills::add_skill_folder(db, &folder))
}

#[tauri::command]
pub fn list_plugins(state: AppStateHandle) -> Result<Vec<Plugin>, String> {
    state.with_db(|db| Ok(skills::list_plugins(db)))
}

#[tauri::command]
pub fn set_plugin_enabled(state: AppStateHandle, id: String, enabled: bool) -> Result<(), String> {
    state.with_db(|db| skills::set_plugin_enabled(db, &id, enabled))
}

fn server_target(server: &McpServerConfig) -> String {
    match (&server.url, &server.command) {
        (Some(url), _) => url.clone(),
        (None, Some(command)) => std::iter::once(command.clone()).chain(server.args.iter().cloned()).collect::<Vec<_>>().join(" "),
        (None, None) => String::new(),
    }
}

#[tauri::command]
pub fn list_mcp_servers(state: AppStateHandle) -> Result<Vec<McpServerStatus>, String> {
    state.with_db(|db| {
        Ok(skills::all_servers(db)
            .iter()
            .map(|server| McpServerStatus {
                name: server.name.clone(),
                source: server.source.clone(),
                target: server_target(server),
                enabled: skills::server_is_enabled(db, &server.name),
            })
            .collect())
    })
}

#[tauri::command]
pub async fn check_mcp_server(state: State<'_, Arc<AppState>>, name: String) -> Result<usize, String> {
    let servers = state.with_db(|db| Ok(skills::all_servers(db)))?;
    let server = servers.iter().find(|server| server.name == name).ok_or("Сервер не найден")?;
    state.mcp.forget(&name).await;
    let tools = state.mcp.tools_of(server, state.http.current()).await?;
    Ok(tools.len())
}

#[tauri::command]
pub fn add_mcp_server(state: AppStateHandle, name: String, target: String) -> Result<(), String> {
    state.with_db(|db| skills::add_manual_server(db, &name, &target))
}

#[tauri::command]
pub async fn remove_mcp_server(state: State<'_, Arc<AppState>>, name: String) -> Result<(), String> {
    state.mcp.forget(&name).await;
    state.with_db(|db| skills::remove_manual_server(db, &name))
}

#[tauri::command]
pub async fn set_mcp_server_enabled(state: State<'_, Arc<AppState>>, name: String, enabled: bool) -> Result<(), String> {
    if !enabled {
        state.mcp.forget(&name).await;
    }
    state.with_db(|db| skills::set_server_enabled(db, &name, enabled))
}

#[tauri::command]
pub fn voice_status(state: AppStateHandle) -> VoiceStatus {
    voice::status(&voice::voice_folder(&state.data_dir))
}

#[tauri::command]
pub async fn install_voice(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<VoiceStatus, String> {
    if state.voice_installing.swap(true, Ordering::SeqCst) {
        return Err("Загрузка уже идёт".to_string());
    }
    let voice_dir = voice::voice_folder(&state.data_dir);
    let http = state.http.current();
    let result = voice::install(&http, &voice_dir, |progress| {
        let _ = app.emit("voice-progress", progress);
    })
    .await;
    state.voice_installing.store(false, Ordering::SeqCst);
    result?;
    Ok(voice::status(&voice_dir))
}

#[tauri::command]
pub async fn prepare_speech(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.speech.prepare(&voice::voice_folder(&state.data_dir)).await
}

#[tauri::command]
pub async fn transcribe_audio(state: State<'_, Arc<AppState>>, request: voice::RecognitionRequest) -> Result<String, String> {
    state.speech.transcribe(&voice::voice_folder(&state.data_dir), &request).await
}

#[tauri::command]
pub async fn release_speech(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.speech.release().await;
    Ok(())
}

#[tauri::command]
pub fn fork_chat(state: AppStateHandle, chat_id: String, message_id: Option<String>) -> Result<ChatSummary, String> {
    let new_chat_id = new_id();
    state.with_db(|db| db.fork_chat(&chat_id, message_id.as_deref(), &new_chat_id))
}

#[tauri::command]
pub fn rewind_preview(state: AppStateHandle, message_id: String) -> Result<RewindPreview, String> {
    state.with_db(|db| db.rewind_preview(&message_id))
}

#[tauri::command]
pub fn rewind_chat(state: AppStateHandle, message_id: String, restore_files: bool) -> Result<RewindOutcome, String> {
    state.with_db(|db| db.rewind_chat(&message_id, restore_files))
}

#[tauri::command]
pub fn undo_file_restore(state: AppStateHandle, chat_id: String) -> Result<Vec<String>, String> {
    state.with_db(|db| db.undo_file_restore(&chat_id))
}

#[tauri::command]
pub fn send_message(app: AppHandle, state: AppStateHandle, request: SendRequest) -> Result<SendOutcome, String> {
    if request.model_refs.is_empty() {
        return Err("Сначала выбери модель".to_string());
    }
    let chat_id = request.chat_id.clone().unwrap_or_else(new_id);
    let is_new_chat = request.chat_id.is_none();
    let user_id = new_id();
    let SendRequest { text, attachments, model_refs, parent_id, access_mode, folder, .. } = request;
    let request_model_refs = model_refs;
    let (chat, user_message, assistants) = state.with_db(|db| {
        if is_new_chat {
            let title = chat::chat_title_from(&text, &attachments);
            db.create_chat(&chat_id, &title, request_model_refs.first().map(String::as_str))?;
            if let Some(mode) = &access_mode {
                save_chat_access(db, &chat_id, mode, folder.as_deref())?;
            }
        }
        let stored_attachments = db.move_images_to_store(&chat_id, attachments)?;
        let attachments_json = serde_json::to_value(&stored_attachments).map_err(|error| error.to_string())?;
        let parent = match &parent_id {
            Some(parent) => Some(parent.clone()),
            None => db.active_leaf(&chat_id)?,
        };
        db.insert_message(&NewMessage {
            id: &user_id,
            chat_id: &chat_id,
            parent_id: parent.as_deref(),
            role: "user",
            content: &text,
            attachments: &attachments_json,
            provider: None,
            model: None,
            status: "done",
            compare_group: None,
        })?;
        let assistants = insert_assistant_placeholders(db, &chat_id, &user_id, &request_model_refs)?;
        Ok((db.get_chat(&chat_id)?, db.get_message(&user_id)?, assistants))
    })?;
    start_generations(&app, &assistants, &request_model_refs, &user_id);
    Ok(SendOutcome {
        chat,
        user_message: Some(user_message),
        assistant_messages: assistants,
    })
}

#[tauri::command]
pub fn regenerate_message(app: AppHandle, state: AppStateHandle, message_id: String, model_ref: String) -> Result<SendOutcome, String> {
    let model_refs = vec![model_ref];
    let (chat, assistants, parent_id) = state.with_db(|db| {
        let original = db.get_message(&message_id)?;
        let parent_id = original.parent_id.clone().ok_or("у сообщения нет вопроса")?;
        let assistants = insert_assistant_placeholders(db, &original.chat_id, &parent_id, &model_refs)?;
        Ok((db.get_chat(&original.chat_id)?, assistants, parent_id))
    })?;
    start_generations(&app, &assistants, &model_refs, &parent_id);
    Ok(SendOutcome {
        chat,
        user_message: None,
        assistant_messages: assistants,
    })
}

#[tauri::command]
pub fn edit_message(app: AppHandle, state: AppStateHandle, message_id: String, text: String, model_ref: String) -> Result<SendOutcome, String> {
    let model_refs = vec![model_ref];
    let user_id = new_id();
    let (chat, user_message, assistants) = state.with_db(|db| {
        let original = db.get_message(&message_id)?;
        db.insert_message(&NewMessage {
            id: &user_id,
            chat_id: &original.chat_id,
            parent_id: original.parent_id.as_deref(),
            role: "user",
            content: &text,
            attachments: &original.attachments,
            provider: None,
            model: None,
            status: "done",
            compare_group: None,
        })?;
        let assistants = insert_assistant_placeholders(db, &original.chat_id, &user_id, &model_refs)?;
        Ok((db.get_chat(&original.chat_id)?, db.get_message(&user_id)?, assistants))
    })?;
    start_generations(&app, &assistants, &model_refs, &user_id);
    Ok(SendOutcome {
        chat,
        user_message: Some(user_message),
        assistant_messages: assistants,
    })
}

#[tauri::command]
pub fn cancel_generation(state: AppStateHandle, message_id: String) {
    state.cancel(&message_id);
}

#[tauri::command]
pub fn search(state: AppStateHandle, query: String) -> Result<Vec<SearchHit>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    state.with_db(|db| db.search(&query, 30))
}

fn provider_is_connected(config: &ProviderConfig, has_key: bool, model_count: i64) -> bool {
    if config.requires_key {
        has_key
    } else {
        model_count > 0
    }
}

#[tauri::command]
pub fn list_providers(state: AppStateHandle) -> Result<Vec<ProviderInfo>, String> {
    state.with_db(|db| {
        let configs = chat::all_providers(db)?;
        let mut infos = Vec::new();
        for config in configs {
            let has_key = keys::exists(&config.id);
            let model_count = db.model_count(&config.id)?;
            infos.push(ProviderInfo {
                connected: provider_is_connected(&config, has_key, model_count),
                id: config.id,
                name: config.name,
                protocol: config.protocol.as_str().to_string(),
                base_url: config.base_url,
                requires_key: config.requires_key,
                is_local: config.is_local,
                is_custom: config.is_custom,
                has_key,
                model_count,
            });
        }
        Ok(infos)
    })
}

async fn refresh_provider_models(state: &AppState, config: &ProviderConfig, key: Option<&str>) -> Result<usize, providers::ProviderError> {
    let http = state.http.current();
    let fetched = providers::fetch_models(&http, config, key).await?;
    let count = fetched.models.len();
    let saved = state.with_db(|db| {
        db.replace_models(&config.id, &fetched.models)?;
        for (model, input, output) in &fetched.prices {
            db.set_price(model, *input, *output)?;
        }
        Ok(())
    });
    if let Err(message) = saved {
        return Err(providers::ProviderError { status: None, message });
    }
    Ok(count)
}

#[tauri::command]
pub async fn add_key(state: State<'_, Arc<AppState>>, key: String, provider_id: Option<String>) -> Result<AddKeyResult, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Вставь ключ".to_string());
    }
    let candidates: Vec<String> = match provider_id {
        Some(id) => vec![id],
        None => providers::candidate_providers_for_key(&key).into_iter().map(String::from).collect(),
    };
    if candidates.is_empty() {
        return Err("Не узнаю этот ключ. Выбери провайдера вручную".to_string());
    }
    let mut last_error = String::new();
    for candidate in candidates {
        let config = state.with_db(|db| find_provider(db, &candidate))?;
        match refresh_provider_models(&state, &config, Some(&key)).await {
            Ok(count) => {
                keys::save(&config.id, &key)?;
                return Ok(AddKeyResult {
                    provider_id: config.id,
                    provider_name: config.name,
                    model_count: count,
                });
            }
            Err(error) => last_error = error.message,
        }
    }
    Err(last_error)
}

#[tauri::command]
pub fn delete_key(state: AppStateHandle, provider_id: String) -> Result<(), String> {
    keys::remove(&provider_id)?;
    state.with_db(|db| db.replace_models(&provider_id, &[]))
}

#[tauri::command]
pub async fn add_custom_provider(state: State<'_, Arc<AppState>>, request: CustomProviderRequest) -> Result<AddKeyResult, String> {
    let id = format!("custom-{}", new_id());
    let base_url = request.base_url.trim().trim_end_matches('/').to_string();
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err("Адрес должен начинаться с http:// или https://".to_string());
    }
    let key = request.key.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(String::from);
    let protocol = Protocol::parse(&request.protocol);
    let config = ProviderConfig {
        id: id.clone(),
        name: request.name.trim().to_string(),
        protocol: protocol.clone(),
        is_local: base_url.contains("localhost") || base_url.contains("127.0.0.1"),
        base_url: base_url.clone(),
        requires_key: false,
        is_custom: true,
    };
    state.with_db(|db| db.add_custom_provider(&id, &config.name, &base_url, protocol.as_str()))?;
    match refresh_provider_models(&state, &config, key.as_deref()).await {
        Ok(count) => {
            if let Some(value) = &key {
                keys::save(&id, value)?;
            }
            Ok(AddKeyResult {
                provider_id: id,
                provider_name: config.name,
                model_count: count,
            })
        }
        Err(error) => {
            state.with_db(|db| db.delete_custom_provider(&id))?;
            Err(error.message)
        }
    }
}

#[tauri::command]
pub fn delete_custom_provider(state: AppStateHandle, provider_id: String) -> Result<(), String> {
    keys::remove(&provider_id)?;
    state.with_db(|db| db.delete_custom_provider(&provider_id))
}

#[tauri::command]
pub async fn refresh_models(state: State<'_, Arc<AppState>>) -> Result<HashMap<String, String>, String> {
    let configs = state.with_db(chat::all_providers)?;
    let mut failures = HashMap::new();
    for config in configs {
        let key = keys::load(&config.id);
        if config.requires_key && key.is_none() {
            continue;
        }
        if let Err(error) = refresh_provider_models(&state, &config, key.as_deref()).await {
            if config.is_local {
                state.with_db(|db| db.replace_models(&config.id, &[]))?;
            } else {
                failures.insert(config.name.clone(), error.message);
            }
        }
    }
    Ok(failures)
}

#[tauri::command]
pub fn list_models(state: AppStateHandle) -> Result<Vec<ModelInfo>, String> {
    state.with_db(|db| db.list_models())
}

fn image_mime_for(extension: &str) -> Option<&'static str> {
    match extension {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

#[tauri::command]
pub fn read_attachment(path: String) -> Result<Attachment, String> {
    let file_path = Path::new(&path);
    let name = file_path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let metadata = std::fs::metadata(file_path).map_err(|_| format!("Не удалось открыть {name}"))?;
    if metadata.is_dir() {
        return Err(format!("{name} это папка. Перетащи отдельные файлы"));
    }
    let extension = file_path
        .extension()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if let Some(mime) = image_mime_for(&extension) {
        if metadata.len() > MAX_IMAGE_ATTACHMENT_BYTES {
            return Err(format!("{name} больше 10 МБ"));
        }
        let bytes = std::fs::read(file_path).map_err(|_| format!("Не удалось прочитать {name}"))?;
        return Ok(Attachment {
            name,
            mime: mime.to_string(),
            kind: "image".to_string(),
            size: metadata.len(),
            text: None,
            data_base64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            image_id: None,
        });
    }
    if metadata.len() > MAX_TEXT_ATTACHMENT_BYTES {
        return Err(format!("{name} больше 2 МБ"));
    }
    let bytes = std::fs::read(file_path).map_err(|_| format!("Не удалось прочитать {name}"))?;
    let text = String::from_utf8(bytes).map_err(|_| format!("{name} не текстовый файл. Пока подходят текст и картинки"))?;
    Ok(Attachment {
        name,
        mime: "text/plain".to_string(),
        kind: "text".to_string(),
        size: metadata.len(),
        text: Some(text),
        data_base64: None,
        image_id: None,
    })
}

#[tauri::command]
pub fn read_image(state: AppStateHandle, image_id: String) -> Result<String, String> {
    let image = state.with_db(|db| db.image_base64(&image_id))?;
    let (mime, encoded) = image.ok_or("Изображение не найдено")?;
    Ok(format!("data:{mime};base64,{encoded}"))
}

#[tauri::command]
pub fn get_settings(state: AppStateHandle) -> Result<HashMap<String, String>, String> {
    state.with_db(|db| Ok(db.all_settings()?.into_iter().collect()))
}

#[tauri::command]
pub fn set_setting(app: AppHandle, state: AppStateHandle, key: String, value: String) -> Result<(), String> {
    state.with_db(|db| db.set_setting(&key, &value))?;
    if key == "hotkeyEnabled" {
        if value == "false" {
            crate::unregister_summon_shortcut(&app);
        } else {
            crate::register_summon_shortcut(&app);
        }
    }
    if key == "keepAwake" {
        state.keep_awake.set_enabled(value != "false");
    }
    Ok(())
}

#[tauri::command]
pub fn usage_summary(state: AppStateHandle) -> Result<UsageSummary, String> {
    state.with_db(|db| db.usage_summary())
}

#[tauri::command]
pub fn request_log(state: AppStateHandle) -> Result<Vec<RequestLogEntry>, String> {
    state.with_db(|db| db.request_log(200))
}

#[tauri::command]
pub fn clear_request_log(state: AppStateHandle) -> Result<(), String> {
    state.with_db(|db| db.clear_request_log())
}

#[tauri::command]
pub fn list_prices(state: AppStateHandle) -> Result<Vec<PriceEntry>, String> {
    state.with_db(|db| db.list_prices())
}

#[tauri::command]
pub fn set_price(state: AppStateHandle, model: String, input: Option<f64>, output: Option<f64>) -> Result<(), String> {
    state.with_db(|db| match (input, output) {
        (Some(input_price), Some(output_price)) => db.set_price(&model, input_price, output_price),
        _ => db.delete_price(&model),
    })
}

#[tauri::command]
pub fn scan_import_sources(state: AppStateHandle) -> Result<Vec<ImportSource>, String> {
    importer::scan_sources(&state)
}

#[tauri::command]
pub fn start_import(app: AppHandle, state: AppStateHandle, sources: Vec<ImportRequest>) -> Result<(), String> {
    for source in &sources {
        state.with_db(|db| db.set_setting(&importer::path_setting_key(&source.source), &source.path))?;
    }
    importer::start_import(app, sources, false)
}

#[tauri::command]
pub async fn sync_chat(state: State<'_, Arc<AppState>>, chat_id: String) -> Result<i64, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || importer::sync_one_chat(&state, &chat_id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn sync_all_chats(app: AppHandle, state: AppStateHandle) -> Result<(), String> {
    importer::start_import(app, importer::existing_sources(&state), false)
}

#[tauri::command]
pub async fn context_usage(state: State<'_, Arc<AppState>>, chat_id: String, model_ref: String) -> Result<ContextUsage, String> {
    chat::context_usage(state.inner(), &chat_id, &model_ref).await
}

#[tauri::command]
pub async fn compact_chat(state: State<'_, Arc<AppState>>, chat_id: String, model_ref: String) -> Result<Option<String>, String> {
    chat::compact_chat(state.inner(), &chat_id, &model_ref).await
}

fn project_of_chat(db: &Database, chat_id: Option<&str>) -> Result<Option<String>, String> {
    match chat_id {
        Some(id) => Ok(db.get_chat(id)?.cwd),
        None => Ok(None),
    }
}

#[tauri::command]
pub fn memory_files(state: AppStateHandle, chat_id: Option<String>) -> Result<Vec<MemoryFile>, String> {
    let (project, sources) = state.with_db(|db| Ok((project_of_chat(db, chat_id.as_deref())?, chat::memory_sources_of(db))))?;
    let places = chat::memory_places_of(&state.data_dir, project.as_deref());
    Ok(memory::collect_memory(&places, sources))
}

#[tauri::command]
pub fn own_memory(state: AppStateHandle, chat_id: Option<String>) -> Result<OwnMemory, String> {
    let project = state.with_db(|db| project_of_chat(db, chat_id.as_deref()))?;
    let read_text = |path: &Path| std::fs::read_to_string(path).unwrap_or_default();
    Ok(OwnMemory {
        shared: read_text(&memory::shared_memory_path(&state.data_dir)),
        project_text: project.as_deref().map(|folder| read_text(&memory::project_memory_path(&state.data_dir, Path::new(folder)))),
        project_folder: project,
    })
}

#[tauri::command]
pub fn save_own_memory(state: AppStateHandle, place: String, chat_id: Option<String>, text: String) -> Result<(), String> {
    let path = if place == "project" {
        let project = state.with_db(|db| project_of_chat(db, chat_id.as_deref()))?.ok_or("У этого чата нет рабочей папки")?;
        memory::project_memory_path(&state.data_dir, Path::new(&project))
    } else {
        memory::shared_memory_path(&state.data_dir)
    };
    memory::write_memory_text(&path, &text)
}

fn add_to_client_chat_list(plan: &claude_code_export::SessionPlan) -> bool {
    let local_app_data = dirs::data_local_dir().map(|folder| folder.join("Claude-Data"));
    let roaming_app_data = dirs::config_dir().map(|folder| folder.join("Claude"));
    let app_data_folders: Vec<_> = local_app_data.into_iter().chain(roaming_app_data).collect();
    let Some(chat_list) = claude_code_export::find_client_chat_list(&app_data_folders) else {
        return false;
    };
    claude_code_export::add_to_client_chat_list(&chat_list, plan, chrono::Utc::now().timestamp_millis()).is_ok()
}

fn deliver_to_claude_code(state: &AppState, chat_id: &str, replace_original: bool) -> Result<ClaudeCodeDelivery, String> {
    let home = dirs::home_dir().ok_or("Не найдена домашняя папка")?;
    let (export, projects_folder) = state.with_db(|db| Ok((db.claude_code_export(chat_id)?, importer::claude_code_projects_folder(db))))?;
    let cwd = export.chat.cwd.clone().unwrap_or_else(|| home.to_string_lossy().to_string());
    let source_lines = match &export.source_file {
        Some(file) => claude_code_export::read_lines(Path::new(file))?,
        None => Vec::new(),
    };
    let (file, session_id, backup_path) = if replace_original {
        let original = export.source_file.as_ref().ok_or("У этого чата нет исходного чата в Claude Code")?;
        let original = Path::new(original).to_path_buf();
        let session_id = original.file_stem().map(|stem| stem.to_string_lossy().to_string()).ok_or("Странное имя файла чата")?;
        claude_code_export::refuse_if_in_use(&original, &session_id, &home.join(".claude"))?;
        let backups_folder = state.data_dir.join("claude-code-backups");
        std::fs::create_dir_all(&backups_folder).map_err(|error| format!("Не удалось создать папку для копий: {error}"))?;
        let backup = claude_code_export::backup_path(&backups_folder, &session_id, chrono::Utc::now().timestamp_millis());
        std::fs::copy(&original, &backup).map_err(|error| format!("Не удалось сделать копию чата: {error}"))?;
        (original, session_id, Some(backup))
    } else {
        let session_id = new_id();
        let folder = match &export.source_file {
            Some(file) => Path::new(file).parent().map(Path::to_path_buf).unwrap_or_else(|| projects_folder.clone()),
            None => projects_folder.join(claude_code_export::project_folder_name(&cwd)),
        };
        (folder.join(format!("{session_id}.jsonl")), session_id, None)
    };
    let plan = claude_code_export::SessionPlan {
        session_id: &session_id,
        cwd: &cwd,
        title: &export.chat.title,
        copied_up_to_uuid: export.copied_up_to_uuid.as_deref(),
        new_turns: &export.turns,
    };
    let lines = claude_code_export::build_session_lines(&source_lines, &plan);
    claude_code_export::write_lines_atomically(&file, &lines)?;
    let listed_in_client = !replace_original && add_to_client_chat_list(&plan);
    let written_is_this_chats_own_file = replace_original && chat_id.starts_with("cc:");
    if written_is_this_chats_own_file {
        let local_ids: Vec<String> = export.turns.iter().map(|turn| turn.uuid.clone()).collect();
        state.with_db(|db| db.mark_messages_as_imported(chat_id, &local_ids))?;
    }
    Ok(ClaudeCodeDelivery {
        session_id,
        cwd,
        written_hash: claude_code_export::hash_of_file(&file)?,
        file_path: file.to_string_lossy().to_string(),
        backup_path: backup_path.map(|path| path.to_string_lossy().to_string()),
        listed_in_client,
    })
}

#[tauri::command]
pub async fn send_to_claude_code(state: State<'_, Arc<AppState>>, chat_id: String, replace_original: bool) -> Result<ClaudeCodeDelivery, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || deliver_to_claude_code(&state, &chat_id, replace_original))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn undo_claude_code_replace(delivery: UndoClaudeCodeReplace) -> Result<(), String> {
    let file = Path::new(&delivery.file_path);
    if claude_code_export::hash_of_file(file)? != delivery.written_hash {
        return Err("Чат уже изменился в Claude Code. Копия лежит здесь: ".to_string() + &delivery.backup_path);
    }
    std::fs::copy(&delivery.backup_path, file).map_err(|error| format!("Не удалось вернуть чат: {error}"))?;
    Ok(())
}

#[tauri::command]
pub fn export_chat(state: AppStateHandle, chat_id: String, path: String) -> Result<(), String> {
    let (chat, messages) = state.with_db(|db| {
        let chat = db.get_chat(&chat_id)?;
        let page = db.thread_page(&chat_id, None, 1_000_000, None)?;
        Ok((chat, page.messages))
    })?;
    let document = if path.to_lowercase().ends_with(".json") {
        export::to_json(&chat, &messages)?
    } else {
        export::to_markdown(&chat, &messages)
    };
    std::fs::write(&path, document).map_err(|error| format!("Не удалось сохранить файл: {error}"))
}

fn chat_folder(state: &AppState, chat_id: &str) -> Result<Option<std::path::PathBuf>, String> {
    let chat = state.with_db(|db| db.get_chat(chat_id))?;
    Ok(chat.cwd.map(std::path::PathBuf::from))
}

#[tauri::command]
pub fn background_tasks(state: AppStateHandle, chat_id: String) -> Vec<TaskSummary> {
    state.background.list(&chat_id)
}

#[tauri::command]
pub fn background_output(state: AppStateHandle, task_id: String) -> String {
    state.background.output(&task_id).unwrap_or_default()
}

#[tauri::command]
pub async fn stop_background_task(state: State<'_, Arc<AppState>>, task_id: String) -> Result<(), String> {
    state.background.stop(&task_id).await
}

fn terminal_notifier(app: &AppHandle) -> TerminalNotifier {
    let app = app.clone();
    Arc::new(move |event| match event {
        TerminalEvent::Output { chat_id, text } => {
            let _ = app.emit("terminal-output", json!({ "chatId": chat_id, "text": text }));
        }
        TerminalEvent::Folder { chat_id, folder } => {
            let _ = app.emit("terminal-folder", json!({ "chatId": chat_id, "folder": folder }));
        }
    })
}

#[tauri::command]
pub fn terminal_view(state: AppStateHandle, chat_id: String) -> TerminalView {
    state.terminals.view(&chat_id)
}

#[tauri::command]
pub fn terminal_run(app: AppHandle, state: AppStateHandle, chat_id: String, line: String) -> Result<(), String> {
    let folder = chat_folder(&state, &chat_id)?;
    state.terminals.run(&chat_id, &line, folder, terminal_notifier(&app))
}

#[tauri::command]
pub async fn terminal_stop(state: State<'_, Arc<AppState>>, chat_id: String) -> Result<(), String> {
    state.terminals.stop(&chat_id).await;
    Ok(())
}

#[tauri::command]
pub fn terminal_clear(state: AppStateHandle, chat_id: String) {
    state.terminals.clear(&chat_id);
}

#[tauri::command]
pub async fn browser_place(app: AppHandle, state: State<'_, Arc<AppState>>, bounds: Option<PanelBounds>, visible: bool) -> Result<(), String> {
    state.browser.place(&app, bounds, visible).await
}

#[tauri::command]
pub async fn browser_go(app: AppHandle, state: State<'_, Arc<AppState>>, target: String) -> Result<(), String> {
    state.browser.go(&app, &target).await
}

#[tauri::command]
pub fn browser_view(state: AppStateHandle) -> BrowserView {
    state.browser.view()
}

#[tauri::command]
pub fn list_folder(state: AppStateHandle, chat_id: String, relative: String) -> Result<Vec<FolderEntry>, String> {
    let root = chat_folder(&state, &chat_id)?.ok_or("У чата нет папки")?;
    file_panel::list_folder(&root, &relative)
}

#[tauri::command]
pub fn changed_files(state: AppStateHandle, chat_id: String) -> Result<Vec<ChangedFile>, String> {
    let root = chat_folder(&state, &chat_id)?;
    let changes = state.with_db(|db| db.chat_file_changes(&chat_id))?;
    Ok(file_panel::summarize_changes(root.as_deref(), changes))
}

#[tauri::command]
pub fn file_view(state: AppStateHandle, chat_id: String, path: String) -> Result<FileView, String> {
    let changes = file_panel::first_changes_by_path(state.with_db(|db| db.chat_file_changes(&chat_id))?);
    let is_changed_path = changes.iter().any(|change| change.path == path);
    let target = if is_changed_path {
        std::path::PathBuf::from(&path)
    } else {
        let root = chat_folder(&state, &chat_id)?.ok_or("У чата нет папки")?;
        file_panel::inside_folder(&root, &path)?
    };
    let comparable_target = file_panel::comparable_path(&target);
    let change = changes.into_iter().find(|change| file_panel::comparable_path(Path::new(&change.path)) == comparable_target);
    Ok(file_panel::view_file(&file_panel::plain_path(&target), change))
}

#[tauri::command]
pub fn chat_plan(state: AppStateHandle, chat_id: String) -> Result<Vec<PlanStep>, String> {
    state.with_db(|db| db.plan_of(&chat_id))
}

#[tauri::command]
pub fn chat_artifacts(state: AppStateHandle, chat_id: String) -> Result<Vec<Artifact>, String> {
    state.with_db(|db| db.artifacts_of(&chat_id))
}

const MAX_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;

#[tauri::command]
pub fn artifact_text(state: AppStateHandle, artifact_id: String) -> Result<String, String> {
    let artifact = state.with_db(|db| db.get_artifact(&artifact_id))?;
    let size = std::fs::metadata(&artifact.path).map_err(|_| "Файла больше нет".to_string())?.len();
    if size > MAX_PREVIEW_BYTES {
        return Err("Файл слишком большой для просмотра".to_string());
    }
    let bytes = std::fs::read(&artifact.path).map_err(|error| format!("Не удалось прочитать: {error}"))?;
    String::from_utf8(bytes).map_err(|_| "Это не текстовый файл".to_string())
}

#[tauri::command]
pub fn save_artifact_copy(state: AppStateHandle, artifact_id: String, target: String) -> Result<(), String> {
    let artifact = state.with_db(|db| db.get_artifact(&artifact_id))?;
    std::fs::copy(&artifact.path, &target).map(|_| ()).map_err(|error| format!("Не удалось сохранить: {error}"))
}

#[tauri::command]
pub async fn open_artifact_in_browser(app: AppHandle, state: State<'_, Arc<AppState>>, artifact_id: String) -> Result<(), String> {
    let artifact = state.with_db(|db| db.get_artifact(&artifact_id))?;
    state.browser.go(&app, &artifact.path).await
}

#[tauri::command]
pub fn widget_page(state: AppStateHandle, html: String) -> Result<String, String> {
    state.widget_pages.add(html)
}

#[tauri::command]
pub async fn bot_sim_start(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<SimView, String> {
    state.bot_simulator.start(&app).await?;
    Ok(state.bot_simulator.view())
}

#[tauri::command]
pub fn bot_sim_view(state: AppStateHandle) -> SimView {
    state.bot_simulator.view()
}

#[tauri::command]
pub fn bot_sim_send(state: AppStateHandle, text: String) -> Result<(), String> {
    state.bot_simulator.send_text(&text)
}

#[tauri::command]
pub fn bot_sim_send_photo(state: AppStateHandle, path: String, caption: String) -> Result<(), String> {
    state.bot_simulator.send_photo(Path::new(&path), &caption)
}

#[tauri::command]
pub fn bot_sim_press(state: AppStateHandle, message_id: i64, data: String) -> Result<(), String> {
    state.bot_simulator.press(message_id, &data)
}

#[tauri::command]
pub fn bot_sim_reset(state: AppStateHandle) {
    state.bot_simulator.reset();
}

#[tauri::command]
pub fn bot_sim_stop(state: AppStateHandle) {
    state.bot_simulator.stop();
}

#[tauri::command]
pub fn save_widget_picture(target: String, png_base64: String) -> Result<(), String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(png_base64.as_bytes()).map_err(|_| "Картинка повреждена".to_string())?;
    std::fs::write(&target, bytes).map_err(|error| format!("Не удалось сохранить: {error}"))
}
