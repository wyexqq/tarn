use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewBuilder, WebviewUrl};
use tokio::sync::oneshot;

use crate::state::AppState;

pub const BROWSER_LABEL: &str = "browser";
const MAIN_WINDOW_LABEL: &str = "main";
const SEARCH_ADDRESS: &str = "https://duckduckgo.com/";
const APP_OWN_HOSTS: [&str; 3] = ["tauri.localhost", "ipc.localhost", "asset.localhost"];
const ALLOWED_SCHEMES: [&str; 4] = ["http", "https", "file", "about"];
const PAGE_LOAD_WAIT: Duration = Duration::from_secs(15);
const PAGE_LOAD_CHECK_INTERVAL: Duration = Duration::from_millis(100);
const SCRIPT_WAIT: Duration = Duration::from_secs(10);
const SETTLE_AFTER_ACTION: Duration = Duration::from_millis(700);
const HIDDEN_PAGE_SIZE: (f64, f64) = (1000.0, 760.0);

pub const TOOL_BROWSER_OPEN: &str = "browser_open";
pub const TOOL_BROWSER_SEARCH: &str = "browser_search";
pub const TOOL_BROWSER_READ: &str = "browser_read";
pub const TOOL_BROWSER_CLICK: &str = "browser_click";
pub const TOOL_BROWSER_TYPE: &str = "browser_type";

const READ_PAGE_SCRIPT: &str = r#"(() => {
  try {
    const selector = 'a[href], button, input, textarea, select, summary, [role=button], [role=link], [role=tab], [role=menuitem], [contenteditable=true]';
    const isShown = (element) => {
      const box = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      return box.width > 0 && box.height > 0 && style.visibility !== 'hidden' && style.display !== 'none';
    };
    const shortText = (value) => String(value || '').replace(/\s+/g, ' ').trim().slice(0, 80);
    document.querySelectorAll('[data-tarn-index]').forEach((element) => element.removeAttribute('data-tarn-index'));
    const elements = [];
    for (const element of document.querySelectorAll(selector)) {
      if (elements.length >= 150) break;
      if (!isShown(element)) continue;
      const index = elements.length + 1;
      element.setAttribute('data-tarn-index', String(index));
      const isPassword = element.type === 'password';
      const ownValue = isPassword ? '' : element.value;
      const label = shortText(element.getAttribute('aria-label') || element.innerText || element.placeholder || element.title || element.name || ownValue);
      const kind = element.tagName.toLowerCase() + (element.type ? ':' + element.type : '');
      const address = element.href ? String(element.href).slice(0, 200) : '';
      elements.push({ index, kind, label, address });
    }
    const text = document.body ? document.body.innerText : '';
    return { url: location.href, title: document.title, text: text.slice(0, 12000), elements };
  } catch (error) {
    return { problem: String(error) };
  }
})()"#;

const CLICK_SCRIPT: &str = r#"(() => {
  try {
    const element = document.querySelector('[data-tarn-index="INDEX"]');
    if (!element) return { problem: 'missing' };
    element.scrollIntoView({ block: 'center' });
    element.click();
    return { problem: null };
  } catch (error) {
    return { problem: String(error) };
  }
})()"#;

const TYPE_SCRIPT: &str = r#"(() => {
  try {
    const element = document.querySelector('[data-tarn-index="INDEX"]');
    if (!element) return { problem: 'missing' };
    if (element.type === 'password') return { problem: 'password' };
    const text = TEXT;
    element.scrollIntoView({ block: 'center' });
    element.focus();
    if (element.isContentEditable) {
      element.textContent = text;
    } else {
      const ownType = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement : element instanceof HTMLSelectElement ? HTMLSelectElement : HTMLInputElement;
      Object.getOwnPropertyDescriptor(ownType.prototype, 'value').set.call(element, text);
    }
    element.dispatchEvent(new Event('input', { bubbles: true }));
    element.dispatchEvent(new Event('change', { bubbles: true }));
    if (SUBMIT) {
      if (element.form && element.form.requestSubmit) {
        element.form.requestSubmit();
      } else {
        const enter = { key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true };
        element.dispatchEvent(new KeyboardEvent('keydown', enter));
        element.dispatchEvent(new KeyboardEvent('keyup', enter));
      }
    }
    return { problem: null };
  } catch (error) {
    return { problem: String(error) };
  }
})()"#;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct PanelBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BrowserView {
    pub url: String,
    pub title: String,
    pub is_loading: bool,
    pub activity: String,
}

#[derive(Default)]
pub struct Browser {
    view: Mutex<BrowserView>,
    placement: Mutex<Option<(PanelBounds, bool)>>,
    finished_loads: AtomicU64,
    creating: tokio::sync::Mutex<()>,
}

pub fn address_from_typed(typed: &str) -> Result<Url, String> {
    let text = typed.trim();
    if text.is_empty() {
        return Err("Адрес пустой".to_string());
    }
    if text.contains("://") || text.starts_with("about:") {
        return Url::parse(text).map_err(|_| format!("Не понимаю адрес {text}"));
    }
    if Path::new(text).is_absolute() {
        return Url::from_file_path(text).map_err(|_| format!("Не понимаю путь {text}"));
    }
    let host = text.split(['/', '?', '#']).next().unwrap_or_default();
    let is_this_computer = host.starts_with("localhost") || host.starts_with("127.") || host.starts_with("[::1]") || host.ends_with(".localhost");
    if is_this_computer {
        return Url::parse(&format!("http://{text}")).map_err(|_| format!("Не понимаю адрес {text}"));
    }
    let looks_like_site = !text.contains(char::is_whitespace) && host.contains('.') && !host.starts_with('.') && !host.ends_with('.');
    if looks_like_site {
        return Url::parse(&format!("https://{text}")).map_err(|_| format!("Не понимаю адрес {text}"));
    }
    search_address(text)
}

pub fn search_address(query: &str) -> Result<Url, String> {
    Url::parse_with_params(SEARCH_ADDRESS, &[("q", query.trim())]).map_err(|error| error.to_string())
}

pub fn may_open(url: &Url, app_origin: Option<&str>) -> bool {
    if !ALLOWED_SCHEMES.contains(&url.scheme()) {
        return false;
    }
    if url.scheme() == "about" && url.as_str() != "about:blank" {
        return false;
    }
    let host = url.host_str().unwrap_or_default().to_lowercase();
    if APP_OWN_HOSTS.contains(&host.as_str()) {
        return false;
    }
    let origin = url.origin().ascii_serialization();
    app_origin.is_none_or(|app| app != origin)
}

fn app_origin(app: &AppHandle) -> Option<String> {
    let main = app.get_webview(MAIN_WINDOW_LABEL)?;
    let url = main.url().ok()?;
    Some(url.origin().ascii_serialization())
}

fn state_of(app: &AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}

fn describe_page(page: &Value) -> String {
    let mut lines = vec![
        format!("Страница: {}", page["title"].as_str().unwrap_or_default()),
        format!("Адрес: {}", page["url"].as_str().unwrap_or_default()),
        String::new(),
        page["text"].as_str().unwrap_or_default().trim().to_string(),
    ];
    let elements = page["elements"].as_array().cloned().unwrap_or_default();
    if !elements.is_empty() {
        lines.push(String::new());
        lines.push("Элементы страницы. Номер нужен для browser_click и browser_type:".to_string());
    }
    for element in elements {
        let label = element["label"].as_str().unwrap_or_default();
        let kind = element["kind"].as_str().unwrap_or_default();
        let address = element["address"].as_str().unwrap_or_default();
        let target = if address.is_empty() { String::new() } else { format!(" → {address}") };
        lines.push(format!("[{}] {kind} «{label}»{target}", element["index"]));
    }
    lines.join("\n")
}

impl Browser {
    pub fn view(&self) -> BrowserView {
        self.view.lock().map(|view| view.clone()).unwrap_or_default()
    }

    fn change_view(&self, app: &AppHandle, change: impl FnOnce(&mut BrowserView)) {
        let updated = match self.view.lock() {
            Ok(mut view) => {
                change(&mut view);
                view.clone()
            }
            Err(_) => return,
        };
        let _ = app.emit("browser-state", updated);
    }

    fn placement(&self) -> Option<(PanelBounds, bool)> {
        self.placement.lock().ok().and_then(|placement| *placement)
    }

    fn page_started(&self, app: &AppHandle, url: &Url) {
        self.change_view(app, |view| {
            view.url = url.to_string();
            view.is_loading = true;
        });
    }

    fn page_finished(&self, app: &AppHandle, url: &Url) {
        self.finished_loads.fetch_add(1, Ordering::SeqCst);
        self.change_view(app, |view| {
            view.url = url.to_string();
            view.is_loading = false;
        });
    }

    async fn webview(&self, app: &AppHandle) -> Result<Webview, String> {
        let _only_one_creation = self.creating.lock().await;
        if let Some(existing) = app.get_webview(BROWSER_LABEL) {
            return Ok(existing);
        }
        let window = app.get_window(MAIN_WINDOW_LABEL).ok_or("Окно приложения не найдено")?;
        let origin = app_origin(app);
        let handle_for_popups = app.clone();
        let handle_for_downloads = app.clone();
        let builder = WebviewBuilder::new(BROWSER_LABEL, WebviewUrl::External(Url::parse("about:blank").map_err(|error| error.to_string())?))
            .disable_drag_drop_handler()
            .on_navigation(move |url| may_open(url, origin.as_deref()))
            .on_new_window(move |url, _| {
                let app = handle_for_popups.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(webview) = app.get_webview(BROWSER_LABEL) {
                        let _ = webview.navigate(url);
                    }
                });
                NewWindowResponse::Deny
            })
            .on_download(move |_, event| {
                if let DownloadEvent::Requested { url, .. } = event {
                    let _ = handle_for_downloads.emit("browser-download-blocked", url.to_string());
                }
                false
            })
            .on_page_load(|webview, payload| {
                let app = webview.app_handle().clone();
                let state = state_of(&app);
                match payload.event() {
                    PageLoadEvent::Started => state.browser.page_started(&app, payload.url()),
                    PageLoadEvent::Finished => state.browser.page_finished(&app, payload.url()),
                }
            })
            .on_document_title_changed(|webview, title| {
                let app = webview.app_handle().clone();
                state_of(&app).browser.change_view(&app, |view| view.title = title);
            });
        let (bounds, visible) = self.placement().unwrap_or((PanelBounds { x: 0.0, y: 0.0, width: HIDDEN_PAGE_SIZE.0, height: HIDDEN_PAGE_SIZE.1 }, false));
        let webview = window
            .add_child(builder, LogicalPosition::new(bounds.x, bounds.y), LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)))
            .map_err(|error| format!("Не удалось открыть браузер: {error}"))?;
        if !visible {
            let _ = webview.hide();
        }
        Ok(webview)
    }

    pub async fn place(&self, app: &AppHandle, bounds: Option<PanelBounds>, visible: bool) -> Result<(), String> {
        if let Ok(mut placement) = self.placement.lock() {
            let known_bounds = bounds.or(placement.map(|(old, _)| old));
            *placement = known_bounds.map(|bounds| (bounds, visible));
        }
        let Some(webview) = app.get_webview(BROWSER_LABEL) else {
            return Ok(());
        };
        if let Some(bounds) = bounds {
            let _ = webview.set_position(LogicalPosition::new(bounds.x, bounds.y));
            let _ = webview.set_size(LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)));
        }
        let shown = if visible { webview.show() } else { webview.hide() };
        shown.map_err(|error| error.to_string())
    }

    async fn wait_for_load_after(&self, loads_before: u64) {
        let mut waited = Duration::ZERO;
        while waited < PAGE_LOAD_WAIT && self.finished_loads.load(Ordering::SeqCst) <= loads_before {
            tokio::time::sleep(PAGE_LOAD_CHECK_INTERVAL).await;
            waited += PAGE_LOAD_CHECK_INTERVAL;
        }
    }

    async fn settle_after_action(&self, loads_before: u64) {
        tokio::time::sleep(SETTLE_AFTER_ACTION).await;
        let navigation_began = self.view().is_loading || self.finished_loads.load(Ordering::SeqCst) > loads_before;
        if navigation_began {
            self.wait_for_load_after(loads_before).await;
        }
    }

    pub async fn go(&self, app: &AppHandle, target: &str) -> Result<(), String> {
        let webview = self.webview(app).await?;
        match target {
            "back" => evaluate(&webview, "history.back(); null").await.map(|_| ()),
            "forward" => evaluate(&webview, "history.forward(); null").await.map(|_| ()),
            "reload" => webview.reload().map_err(|error| error.to_string()),
            typed => {
                let url = address_from_typed(typed)?;
                if !may_open(&url, app_origin(app).as_deref()) {
                    return Err("Этот адрес во встроенном браузере не открывается".to_string());
                }
                webview.navigate(url).map_err(|error| error.to_string())
            }
        }
    }

    fn show_activity(&self, app: &AppHandle, chat_id: &str, activity: &str) {
        if !activity.is_empty() {
            let _ = app.emit("panel-open", json!({ "chatId": chat_id, "tab": "browser" }));
        }
        self.change_view(app, |view| view.activity = activity.to_string());
    }

    async fn read_page(&self, webview: &Webview) -> Result<String, String> {
        let page = evaluate(webview, READ_PAGE_SCRIPT).await?;
        if let Some(problem) = page["problem"].as_str() {
            return Err(format!("Не удалось прочитать страницу: {problem}"));
        }
        Ok(describe_page(&page))
    }

    async fn open_url(&self, app: &AppHandle, url: Url) -> Result<String, String> {
        if !may_open(&url, app_origin(app).as_deref()) {
            return Err("Этот адрес во встроенном браузере не открывается".to_string());
        }
        let webview = self.webview(app).await?;
        let loads_before = self.finished_loads.load(Ordering::SeqCst);
        webview.navigate(url).map_err(|error| error.to_string())?;
        self.wait_for_load_after(loads_before).await;
        self.read_page(&webview).await
    }

    async fn click(&self, app: &AppHandle, index: u64) -> Result<String, String> {
        let webview = self.webview(app).await?;
        let loads_before = self.finished_loads.load(Ordering::SeqCst);
        let answer = evaluate(&webview, &CLICK_SCRIPT.replace("INDEX", &index.to_string())).await?;
        match answer["problem"].as_str() {
            Some("missing") => return Err(format!("Элемента {index} нет. Сначала вызови browser_read")),
            Some(problem) => return Err(format!("Не удалось нажать: {problem}")),
            None => {}
        }
        self.settle_after_action(loads_before).await;
        self.read_page(&webview).await
    }

    async fn type_text(&self, app: &AppHandle, index: u64, text: &str, submit: bool) -> Result<String, String> {
        let webview = self.webview(app).await?;
        let loads_before = self.finished_loads.load(Ordering::SeqCst);
        let text_literal = serde_json::to_string(text).map_err(|error| error.to_string())?;
        let script = TYPE_SCRIPT.replace("INDEX", &index.to_string()).replace("SUBMIT", if submit { "true" } else { "false" }).replace("TEXT", &text_literal);
        let answer = evaluate(&webview, &script).await?;
        match answer["problem"].as_str() {
            Some("missing") => return Err(format!("Элемента {index} нет. Сначала вызови browser_read")),
            Some("password") => return Err("Пароли вводит пользователь сам".to_string()),
            Some(problem) => return Err(format!("Не удалось ввести текст: {problem}")),
            None => {}
        }
        self.settle_after_action(loads_before).await;
        self.read_page(&webview).await
    }

    pub async fn run_tool(&self, app: &AppHandle, chat_id: &str, name: &str, input: &Value) -> Result<String, String> {
        let activity = match name {
            TOOL_BROWSER_OPEN => "Модель открывает страницу",
            TOOL_BROWSER_SEARCH => "Модель ищет",
            TOOL_BROWSER_CLICK => "Модель нажимает на странице",
            TOOL_BROWSER_TYPE => "Модель печатает на странице",
            _ => "Модель смотрит страницу",
        };
        self.show_activity(app, chat_id, activity);
        let index = input["index"].as_u64().unwrap_or_default();
        let outcome = match name {
            TOOL_BROWSER_OPEN => match address_from_typed(input["url"].as_str().unwrap_or_default()) {
                Ok(url) => self.open_url(app, url).await,
                Err(problem) => Err(problem),
            },
            TOOL_BROWSER_SEARCH => match search_address(input["query"].as_str().unwrap_or_default()) {
                Ok(url) => self.open_url(app, url).await,
                Err(problem) => Err(problem),
            },
            TOOL_BROWSER_CLICK => self.click(app, index).await,
            TOOL_BROWSER_TYPE => self.type_text(app, index, input["text"].as_str().unwrap_or_default(), input["submit"].as_bool().unwrap_or(false)).await,
            _ => match self.webview(app).await {
                Ok(webview) => self.read_page(&webview).await,
                Err(problem) => Err(problem),
            },
        };
        self.show_activity(app, chat_id, "");
        outcome
    }
}

async fn evaluate(webview: &Webview, script: &str) -> Result<Value, String> {
    let (sender, receiver) = oneshot::channel::<String>();
    let sender = Mutex::new(Some(sender));
    webview
        .eval_with_callback(script, move |result| {
            if let Some(sender) = sender.lock().ok().and_then(|mut waiting| waiting.take()) {
                let _ = sender.send(result);
            }
        })
        .map_err(|error| error.to_string())?;
    let raw = tokio::time::timeout(SCRIPT_WAIT, receiver)
        .await
        .map_err(|_| "Страница не ответила вовремя".to_string())?
        .map_err(|_| "Страница не ответила".to_string())?;
    serde_json::from_str(&raw).map_err(|_| "Страница вернула непонятный ответ".to_string())
}

pub fn is_browser_tool(name: &str) -> bool {
    matches!(name, TOOL_BROWSER_OPEN | TOOL_BROWSER_SEARCH | TOOL_BROWSER_READ | TOOL_BROWSER_CLICK | TOOL_BROWSER_TYPE)
}

pub fn changes_nothing(name: &str) -> bool {
    matches!(name, TOOL_BROWSER_SEARCH | TOOL_BROWSER_READ)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_becomes_an_address_or_a_search() {
        assert_eq!(address_from_typed("localhost:5173").unwrap().as_str(), "http://localhost:5173/");
        assert_eq!(address_from_typed("127.0.0.1:8080/api").unwrap().as_str(), "http://127.0.0.1:8080/api");
        assert_eq!(address_from_typed("example.com/путь").unwrap().host_str(), Some("example.com"));
        assert_eq!(address_from_typed("example.com").unwrap().scheme(), "https");
        let search = address_from_typed("как работает парсер").unwrap();
        assert_eq!(search.host_str(), Some("duckduckgo.com"));
        assert!(search.query_pairs().any(|(key, value)| key == "q" && value == "как работает парсер"));
        assert_eq!(address_from_typed("C:\\site\\index.html").unwrap().scheme(), "file");
    }

    #[test]
    fn the_app_itself_cannot_be_opened_inside_the_browser() {
        let app = Some("http://tauri.localhost");
        assert!(!may_open(&Url::parse("http://tauri.localhost/index.html").unwrap(), app));
        assert!(!may_open(&Url::parse("http://ipc.localhost/x").unwrap(), app));
        assert!(!may_open(&Url::parse("http://localhost:1427/").unwrap(), Some("http://localhost:1427")));
        assert!(!may_open(&Url::parse("javascript:alert(1)").unwrap(), app));
        assert!(may_open(&Url::parse("http://localhost:5173/").unwrap(), app));
        assert!(may_open(&Url::parse("https://example.com/").unwrap(), app));
        assert!(may_open(&Url::parse("about:blank").unwrap(), app));
    }

    #[test]
    fn page_description_lists_elements_with_numbers() {
        let page = json!({
            "title": "Поиск", "url": "https://example.com/",
            "text": "Привет",
            "elements": [{ "index": 1, "kind": "input:text", "label": "Запрос", "address": "" }, { "index": 2, "kind": "a", "label": "Войти", "address": "https://example.com/login" }]
        });
        let described = describe_page(&page);
        assert!(described.contains("[1] input:text «Запрос»"));
        assert!(described.contains("[2] a «Войти» → https://example.com/login"));
    }
}
