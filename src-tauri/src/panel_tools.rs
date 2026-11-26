use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::background::{BackgroundEvent, BackgroundNotifier, NewTask};
use crate::bot_sim::{self, TOOL_BOT_SIMULATOR};
use crate::browser::{self, TOOL_BROWSER_CLICK, TOOL_BROWSER_OPEN, TOOL_BROWSER_READ, TOOL_BROWSER_SEARCH, TOOL_BROWSER_TYPE};
use crate::providers::ToolSpec;
use crate::state::AppState;
use crate::tools::{self, spec, AccessMode, EnabledTools, ToolContext, ToolRisk};
use crate::types::{Artifact, PlanStep};
use crate::widgets::{self, TOOL_SHOW_WIDGET};

pub const TOOL_START_BACKGROUND: &str = "start_background";
pub const TOOL_READ_BACKGROUND: &str = "read_background";
pub const TOOL_STOP_BACKGROUND: &str = "stop_background";
pub const TOOL_UPDATE_PLAN: &str = "update_plan";
pub const TOOL_CREATE_ARTIFACT: &str = "create_artifact";
pub const TOOL_SEND_FILE: &str = "send_file";

const BROWSER_SERVER: &str = "browser";
const ARTIFACTS_FOLDER: &str = "artifacts";
const MAX_ARTIFACT_BYTES: usize = 5 * 1024 * 1024;
const ARTIFACT_KINDS: [(&str, &str); 7] = [
    ("html", "html"),
    ("markdown", "md"),
    ("text", "txt"),
    ("csv", "csv"),
    ("svg", "svg"),
    ("json", "json"),
    ("code", "txt"),
];
const PLAN_STATUSES: [&str; 3] = ["pending", "in_progress", "done"];

pub struct PanelContext<'a> {
    pub app: &'a AppHandle,
    pub state: &'a AppState,
    pub chat_id: &'a str,
    pub message_id: &'a str,
}

pub struct PanelOutcome {
    pub result: Result<String, String>,
    pub artifact: Option<Artifact>,
}

impl PanelOutcome {
    fn text(result: Result<String, String>) -> PanelOutcome {
        PanelOutcome { result, artifact: None }
    }
}

pub fn specs(mode: AccessMode, enabled: &EnabledTools) -> Vec<ToolSpec> {
    let mut specs = vec![
        spec(
            TOOL_UPDATE_PLAN,
            "Показать пользователю план работы и отмечать сделанные шаги. Каждый раз присылай весь список.",
            json!({ "type": "object", "properties": {
                "steps": { "type": "array", "items": { "type": "object", "properties": {
                    "text": { "type": "string" },
                    "status": { "type": "string", "enum": PLAN_STATUSES }
                }, "required": ["text", "status"] } }
            }, "required": ["steps"] }),
        ),
        spec(
            TOOL_CREATE_ARTIFACT,
            "Создать артефакт: страницу, документ, таблицу, картинку или код. Пользователь откроет его в панели и сможет сохранить файлом.",
            json!({ "type": "object", "properties": {
                "title": { "type": "string" },
                "kind": { "type": "string", "enum": ARTIFACT_KINDS.map(|(kind, _)| kind) },
                "content": { "type": "string" }
            }, "required": ["title", "kind", "content"] }),
        ),
        widgets::tool_spec(),
    ];
    if mode == AccessMode::Chat {
        return specs;
    }
    if enabled.read {
        specs.push(spec(
            TOOL_SEND_FILE,
            "Отправить пользователю файл с компьютера: он появится карточкой в чате.",
            json!({ "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }),
        ));
    }
    specs.push(spec(
        TOOL_BROWSER_SEARCH,
        "Найти в интернете через встроенный браузер. Пользователь видит страницу. Возвращает текст страницы и пронумерованные элементы.",
        json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }),
    ));
    specs.push(spec(
        TOOL_BROWSER_READ,
        "Заново прочитать текущую страницу встроенного браузера.",
        json!({ "type": "object", "properties": {} }),
    ));
    if mode == AccessMode::Plan {
        return specs;
    }
    specs.push(bot_sim::tool_spec());
    specs.push(spec(
        TOOL_BROWSER_OPEN,
        "Открыть адрес во встроенном браузере, в том числе localhost. Возвращает текст страницы и пронумерованные элементы.",
        json!({ "type": "object", "properties": { "url": { "type": "string" } }, "required": ["url"] }),
    ));
    specs.push(spec(
        TOOL_BROWSER_CLICK,
        "Нажать на элемент страницы по номеру из последнего чтения.",
        json!({ "type": "object", "properties": { "index": { "type": "integer" } }, "required": ["index"] }),
    ));
    specs.push(spec(
        TOOL_BROWSER_TYPE,
        "Ввести текст в поле страницы по номеру. submit отправляет форму.",
        json!({ "type": "object", "properties": {
            "index": { "type": "integer" },
            "text": { "type": "string" },
            "submit": { "type": "boolean" }
        }, "required": ["index", "text"] }),
    ));
    if enabled.command {
        specs.push(spec(
            TOOL_START_BACKGROUND,
            "Запустить долгую команду PowerShell в фоне, например сервер разработки. Возвращает номер задачи и первые секунды вывода.",
            json!({ "type": "object", "properties": {
                "command": { "type": "string" },
                "name": { "type": "string", "description": "Короткое название для пользователя" }
            }, "required": ["command", "name"] }),
        ));
        specs.push(spec(
            TOOL_READ_BACKGROUND,
            "Прочитать состояние и последние строки вывода фоновой задачи.",
            json!({ "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] }),
        ));
        specs.push(spec(
            TOOL_STOP_BACKGROUND,
            "Остановить фоновую задачу.",
            json!({ "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] }),
        ));
    }
    specs
}

pub fn runs_without_permission(name: &str) -> bool {
    matches!(name, TOOL_UPDATE_PLAN | TOOL_CREATE_ARTIFACT | TOOL_SHOW_WIDGET)
}

pub fn risk_of(name: &str, input: &Value) -> Option<ToolRisk> {
    match name {
        TOOL_START_BACKGROUND => Some(ToolRisk::Command(input["command"].as_str().unwrap_or_default().to_string())),
        TOOL_READ_BACKGROUND | TOOL_STOP_BACKGROUND | TOOL_SEND_FILE => Some(ToolRisk::Read),
        TOOL_BOT_SIMULATOR => Some(bot_sim::risk_of(input)),
        name if browser::is_browser_tool(name) => Some(ToolRisk::External { server: BROWSER_SERVER.to_string(), read_only: browser::changes_nothing(name) }),
        _ => None,
    }
}

pub fn is_panel_tool(name: &str) -> bool {
    risk_of(name, &Value::Null).is_some() || runs_without_permission(name)
}

pub fn background_notifier(app: &AppHandle) -> BackgroundNotifier {
    let app = app.clone();
    Arc::new(move |event| match event {
        BackgroundEvent::Changed { chat_id } => {
            let _ = app.emit("background-changed", json!({ "chatId": chat_id }));
        }
        BackgroundEvent::Output { task_id, text } => {
            let _ = app.emit("background-output", json!({ "taskId": task_id, "text": text }));
        }
    })
}

pub fn parse_plan(input: &Value) -> Result<Vec<PlanStep>, String> {
    let steps = input["steps"].as_array().ok_or("Не указан список steps")?;
    steps
        .iter()
        .map(|step| {
            let text = step["text"].as_str().unwrap_or_default().trim().to_string();
            let status = step["status"].as_str().unwrap_or("pending").to_string();
            if text.is_empty() {
                return Err("У шага нет текста".to_string());
            }
            if !PLAN_STATUSES.contains(&status.as_str()) {
                return Err(format!("Неизвестное состояние шага {status}"));
            }
            Ok(PlanStep { text, status })
        })
        .collect()
}

fn extension_of_kind(kind: &str) -> Option<&'static str> {
    ARTIFACT_KINDS.iter().find(|(known, _)| *known == kind).map(|(_, extension)| *extension)
}

pub fn artifacts_folder(data_dir: &Path) -> PathBuf {
    data_dir.join(ARTIFACTS_FOLDER)
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_artifact_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn update_plan(context: &PanelContext, input: &Value) -> Result<String, String> {
    let steps = parse_plan(input)?;
    context.state.with_db(|db| db.set_plan(context.chat_id, &steps))?;
    let _ = context.app.emit("plan-changed", json!({ "chatId": context.chat_id, "steps": steps }));
    Ok("План показан пользователю".to_string())
}

fn create_artifact(context: &PanelContext, input: &Value) -> PanelOutcome {
    let title = input["title"].as_str().unwrap_or_default().trim();
    let kind = input["kind"].as_str().unwrap_or_default();
    let content = input["content"].as_str().unwrap_or_default();
    let Some(extension) = extension_of_kind(kind) else {
        return PanelOutcome::text(Err(format!("Неизвестный вид артефакта {kind}")));
    };
    if title.is_empty() || content.is_empty() {
        return PanelOutcome::text(Err("Нужны название и содержимое".to_string()));
    }
    if content.len() > MAX_ARTIFACT_BYTES {
        return PanelOutcome::text(Err("Артефакт больше 5 МБ".to_string()));
    }
    let id = new_artifact_id();
    let folder = artifacts_folder(&context.state.data_dir);
    let path = folder.join(format!("{id}.{extension}"));
    let written = std::fs::create_dir_all(&folder).and_then(|_| std::fs::write(&path, content));
    if let Err(error) = written {
        return PanelOutcome::text(Err(format!("Не удалось сохранить артефакт: {error}")));
    }
    let artifact = Artifact {
        id,
        chat_id: context.chat_id.to_string(),
        message_id: context.message_id.to_string(),
        title: title.to_string(),
        kind: kind.to_string(),
        path: path.display().to_string(),
        size: content.len() as i64,
        created_at: now_millis(),
    };
    record_artifact(context, artifact, format!("Артефакт «{title}» создан, пользователь видит его в панели"))
}

fn record_artifact(context: &PanelContext, artifact: Artifact, message: String) -> PanelOutcome {
    if let Err(problem) = context.state.with_db(|db| db.add_artifact(&artifact)) {
        return PanelOutcome::text(Err(problem));
    }
    let _ = context.app.emit("artifacts-changed", json!({ "chatId": context.chat_id }));
    PanelOutcome { result: Ok(message), artifact: Some(artifact) }
}

fn send_file(context: &PanelContext, input: &Value, tool_context: &ToolContext) -> PanelOutcome {
    let raw = input["path"].as_str().unwrap_or_default();
    let path = match tools::readable_path(raw, tool_context) {
        Ok(path) => path,
        Err(problem) => return PanelOutcome::text(Err(problem)),
    };
    let Ok(metadata) = std::fs::metadata(&path) else {
        return PanelOutcome::text(Err(format!("Файла {raw} нет")));
    };
    if !metadata.is_file() {
        return PanelOutcome::text(Err(format!("{raw} это не файл")));
    }
    let shown_path = path.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let title = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_else(|| raw.to_string());
    let artifact = Artifact {
        id: new_artifact_id(),
        chat_id: context.chat_id.to_string(),
        message_id: context.message_id.to_string(),
        title: title.clone(),
        kind: "file".to_string(),
        path: shown_path,
        size: metadata.len() as i64,
        created_at: now_millis(),
    };
    record_artifact(context, artifact, format!("Файл {title} отправлен пользователю"))
}

async fn start_background(context: &PanelContext<'_>, input: &Value, folder: Option<PathBuf>) -> Result<String, String> {
    let command = input["command"].as_str().unwrap_or_default().trim().to_string();
    if command.is_empty() {
        return Err("Не указана команда".to_string());
    }
    let name = input["name"].as_str().map(str::trim).filter(|name| !name.is_empty()).unwrap_or(&command).chars().take(60).collect();
    let task = NewTask {
        chat_id: context.chat_id.to_string(),
        name,
        command,
        folder,
        keeps_computer_awake: Some(context.state.keep_awake.begin_work()),
    };
    let tasks = &context.state.background;
    let task_id = tasks.start(task, background_notifier(context.app))?;
    tasks.give_time_to_start(&task_id).await;
    tasks.report_for_model(&task_id).ok_or_else(|| "Задача пропала".to_string())
}

fn own_task_id<'a>(context: &PanelContext, input: &'a Value) -> Result<&'a str, String> {
    let task_id = input["id"].as_str().unwrap_or_default();
    let belongs_here = context.state.background.summary(task_id).is_some_and(|summary| summary.chat_id == context.chat_id);
    if belongs_here {
        Ok(task_id)
    } else {
        Err(format!("Фоновой задачи {task_id} в этом чате нет"))
    }
}

async fn stop_background(context: &PanelContext<'_>, input: &Value) -> Result<String, String> {
    let task_id = own_task_id(context, input)?;
    context.state.background.stop(task_id).await?;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    context.state.background.report_for_model(task_id).ok_or_else(|| "Задача пропала".to_string())
}

pub fn run_without_permission(context: &PanelContext, name: &str, input: &Value) -> PanelOutcome {
    match name {
        TOOL_UPDATE_PLAN => PanelOutcome::text(update_plan(context, input)),
        TOOL_CREATE_ARTIFACT => create_artifact(context, input),
        TOOL_SHOW_WIDGET => PanelOutcome::text(widgets::check(input)),
        _ => PanelOutcome::text(Err(format!("Инструмента {name} нет"))),
    }
}

pub async fn run(context: &PanelContext<'_>, name: &str, input: &Value, tool_context: &ToolContext) -> PanelOutcome {
    match name {
        TOOL_SEND_FILE => send_file(context, input, tool_context),
        TOOL_START_BACKGROUND => PanelOutcome::text(start_background(context, input, tool_context.folder.clone()).await),
        TOOL_READ_BACKGROUND => PanelOutcome::text(own_task_id(context, input).and_then(|task_id| context.state.background.report_for_model(task_id).ok_or_else(|| "Задача пропала".to_string()))),
        TOOL_STOP_BACKGROUND => PanelOutcome::text(stop_background(context, input).await),
        TOOL_BOT_SIMULATOR => PanelOutcome::text(context.state.bot_simulator.run_tool(context.app, input).await),
        name if browser::is_browser_tool(name) => PanelOutcome::text(context.state.browser.run_tool(context.app, context.chat_id, name, input).await),
        _ => PanelOutcome::text(Err(format!("Инструмента {name} нет"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_tools() -> EnabledTools {
        EnabledTools { read: true, edit: true, command: true }
    }

    fn names(specs: &[ToolSpec]) -> Vec<String> {
        specs.iter().map(|spec| spec.name.clone()).collect()
    }

    #[test]
    fn plain_chat_gets_only_the_plan_artifacts_and_widgets() {
        assert_eq!(names(&specs(AccessMode::Chat, &all_tools())), [TOOL_UPDATE_PLAN, TOOL_CREATE_ARTIFACT, TOOL_SHOW_WIDGET]);
    }

    #[test]
    fn bot_simulator_is_offered_only_where_actions_are_allowed() {
        assert!(!names(&specs(AccessMode::Plan, &all_tools())).contains(&TOOL_BOT_SIMULATOR.to_string()));
        assert!(names(&specs(AccessMode::Auto, &all_tools())).contains(&TOOL_BOT_SIMULATOR.to_string()));
        assert!(is_panel_tool(TOOL_BOT_SIMULATOR));
        assert!(runs_without_permission(TOOL_SHOW_WIDGET));
    }

    #[test]
    fn plan_mode_cannot_click_or_start_anything() {
        let offered = names(&specs(AccessMode::Plan, &all_tools()));
        assert!(offered.contains(&TOOL_BROWSER_SEARCH.to_string()));
        assert!(!offered.contains(&TOOL_BROWSER_CLICK.to_string()));
        assert!(!offered.contains(&TOOL_START_BACKGROUND.to_string()));
        for name in &offered {
            if let Some(risk) = risk_of(name, &Value::Null) {
                assert!(tools::is_allowed_at_all(AccessMode::Plan, &risk), "{name}");
            }
        }
    }

    #[test]
    fn background_needs_the_command_switch() {
        let without_commands = EnabledTools { read: true, edit: true, command: false };
        assert!(!names(&specs(AccessMode::Auto, &without_commands)).contains(&TOOL_START_BACKGROUND.to_string()));
        assert!(names(&specs(AccessMode::Auto, &all_tools())).contains(&TOOL_START_BACKGROUND.to_string()));
    }

    #[test]
    fn browser_actions_that_change_pages_ask_like_other_outside_tools() {
        let click = risk_of(TOOL_BROWSER_CLICK, &Value::Null).unwrap();
        let search = risk_of(TOOL_BROWSER_SEARCH, &Value::Null).unwrap();
        assert!(tools::needs_approval(AccessMode::Auto, &click));
        assert!(!tools::needs_approval(AccessMode::Ask, &search));
        assert_eq!(click.permission_scope(), "external:browser");
    }

    #[test]
    fn plan_steps_are_checked() {
        let good = json!({ "steps": [{ "text": "Сделать", "status": "done" }, { "text": "Проверить", "status": "pending" }] });
        assert_eq!(parse_plan(&good).unwrap().len(), 2);
        assert!(parse_plan(&json!({ "steps": [{ "text": "", "status": "done" }] })).is_err());
        assert!(parse_plan(&json!({ "steps": [{ "text": "x", "status": "почти" }] })).is_err());
    }
}
