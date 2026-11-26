use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use sha2::{Digest, Sha256};
use tauri::http::{header, Response, StatusCode};

use crate::state::AppState;

pub const SCHEME: &str = "widget";
pub const BOT_FILE_PREFIX: &str = "file-";
const MAX_PAGES: usize = 64;
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const PAGE_ID_BYTES: usize = 12;
const PAGE_POLICY: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; form-action 'none'; base-uri 'none'";
const ONLY_WINDOW_ALLOWED: &str = "main";

#[derive(Default)]
pub struct WidgetPages {
    store: Mutex<PageStore>,
}

#[derive(Default)]
struct PageStore {
    by_id: HashMap<String, String>,
    order: VecDeque<String>,
}

fn page_id(html: &str) -> String {
    let digest = Sha256::digest(html.as_bytes());
    digest.iter().take(PAGE_ID_BYTES).map(|byte| format!("{byte:02x}")).collect()
}

impl WidgetPages {
    pub fn add(&self, html: String) -> Result<String, String> {
        if html.len() > MAX_PAGE_BYTES {
            return Err("Страница больше 2 МБ".to_string());
        }
        let id = page_id(&html);
        let mut store = self.store.lock().map_err(|_| "страницы недоступны".to_string())?;
        store.order.retain(|known| known != &id);
        store.order.push_back(id.clone());
        store.by_id.insert(id.clone(), html);
        while store.order.len() > MAX_PAGES {
            if let Some(oldest) = store.order.pop_front() {
                store.by_id.remove(&oldest);
            }
        }
        Ok(id)
    }

    pub fn get(&self, id: &str) -> Option<String> {
        self.store.lock().ok().and_then(|store| store.by_id.get(id).cloned())
    }
}

fn not_found() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(b"not found".to_vec())
        .unwrap_or_default()
}

fn page_response(html: String) -> Response<Vec<u8>> {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CONTENT_SECURITY_POLICY, PAGE_POLICY)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CACHE_CONTROL, "no-store")
        .body(html.into_bytes())
        .unwrap_or_default()
}

fn file_response(bytes: Vec<u8>, mime: String) -> Response<Vec<u8>> {
    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_SECURITY_POLICY, "default-src 'none'")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(bytes)
        .unwrap_or_default()
}

pub fn respond(state: &AppState, webview_label: &str, path: &str) -> Response<Vec<u8>> {
    if webview_label != ONLY_WINDOW_ALLOWED {
        return not_found();
    }
    let name = path.trim_start_matches('/');
    if let Some(file_id) = name.strip_prefix(BOT_FILE_PREFIX) {
        return match state.bot_simulator.file(file_id) {
            Some(file) => file_response(file.bytes, file.mime),
            None => not_found(),
        };
    }
    match state.widget_pages.get(name) {
        Some(html) => page_response(html),
        None => not_found(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_page_gets_the_same_address() {
        let pages = WidgetPages::default();
        let first = pages.add("<p>1</p>".to_string()).unwrap();
        let again = pages.add("<p>1</p>".to_string()).unwrap();
        let other = pages.add("<p>2</p>".to_string()).unwrap();
        assert_eq!(first, again);
        assert_ne!(first, other);
        assert_eq!(pages.get(&first).as_deref(), Some("<p>1</p>"));
    }

    #[test]
    fn old_pages_are_forgotten_after_the_limit() {
        let pages = WidgetPages::default();
        let first = pages.add("<p>first</p>".to_string()).unwrap();
        for number in 0..MAX_PAGES {
            pages.add(format!("<p>{number}</p>")).unwrap();
        }
        assert!(pages.get(&first).is_none());
        assert!(pages.add("x".repeat(MAX_PAGE_BYTES + 1)).is_err());
    }
}
