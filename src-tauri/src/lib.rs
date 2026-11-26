mod background;
mod bot_sim;
mod browser;
mod chat;
mod claude_code_export;
mod commands;
mod compaction;
mod db;
mod export;
mod file_history;
mod file_panel;
mod images;
mod importer;
mod keep_awake;
mod keys;
mod mcp;
mod memory;
mod network;
mod panel_tools;
mod providers;
mod skills;
mod state;
mod tools;
mod types;
mod user_terminal;
mod speech_worker;
mod voice;
mod widget_pages;
mod widgets;

pub use speech_worker::{run as run_speech_worker, WORKER_FLAG as SPEECH_WORKER_FLAG};

use std::path::PathBuf;
use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::state::AppState;

const SUMMON_SHORTCUT: &str = "Ctrl+Shift+Space";
const PORTABLE_FLAG_FILE: &str = "tarn.portable";

fn resolve_data_dir(app: &tauri::App) -> PathBuf {
    if let Ok(custom) = std::env::var("TARN_DATA_DIR") {
        return PathBuf::from(custom);
    }
    let portable_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from));
    if let Some(dir) = portable_dir {
        if dir.join(PORTABLE_FLAG_FILE).exists() {
            return dir.join("data");
        }
    }
    app.path()
        .app_data_dir()
        .expect("папка данных приложения должна определяться")
}

fn summon_main_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_window("main") else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit("focus-composer", ());
}

pub fn register_summon_shortcut(app: &tauri::AppHandle) {
    let result = app.global_shortcut().on_shortcut(SUMMON_SHORTCUT, |app, _, event| {
        if event.state() == ShortcutState::Pressed {
            summon_main_window(app);
        }
    });
    if let Err(error) = result {
        eprintln!("не удалось занять горячую клавишу: {error}");
    }
}

pub fn unregister_summon_shortcut(app: &tauri::AppHandle) {
    if let Err(error) = app.global_shortcut().unregister(SUMMON_SHORTCUT) {
        eprintln!("не удалось освободить горячую клавишу: {error}");
    }
}

fn build_tray_icon(app: &tauri::App) -> tauri::Result<()> {
    let open_item = MenuItem::with_id(app, "open", "Открыть Tarn", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Выйти", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &separator, &quit_item])?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Tarn")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => summon_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                summon_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn add_everyday_window_plugins(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            summon_main_window(app);
        }))
        .plugin(tauri_plugin_window_state::Builder::default().with_state_flags(
            tauri_plugin_window_state::StateFlags::SIZE
                | tauri_plugin_window_state::StateFlags::POSITION
                | tauri_plugin_window_state::StateFlags::MAXIMIZED,
        ).build())
}

#[cfg(windows)]
fn turn_off_saved_form_data(window: &tauri::WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings4;
    use windows_core::Interface;

    let result = window.with_webview(|webview| unsafe {
        let Ok(core) = webview.controller().CoreWebView2() else {
            return;
        };
        let Ok(settings) = core.Settings().and_then(|settings| settings.cast::<ICoreWebView2Settings4>()) else {
            return;
        };
        let _ = settings.SetIsGeneralAutofillEnabled(false);
        let _ = settings.SetIsPasswordAutosaveEnabled(false);
    });
    if let Err(error) = result {
        eprintln!("не удалось отключить сохранённые сведения: {error}");
    }
}

#[cfg(not(windows))]
fn turn_off_saved_form_data(_window: &tauri::WebviewWindow) {}

pub fn run() {
    let runs_on_separate_data = std::env::var_os("TARN_DATA_DIR").is_some();
    let builder = if runs_on_separate_data {
        tauri::Builder::default()
    } else {
        add_everyday_window_plugins(tauri::Builder::default())
    };
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .register_uri_scheme_protocol(widget_pages::SCHEME, |context, request| {
            let state = context.app_handle().state::<Arc<AppState>>();
            widget_pages::respond(&state, context.webview_label(), request.uri().path())
        })
        .setup(|app| {
            let data_dir = resolve_data_dir(app);
            std::fs::create_dir_all(&data_dir)?;
            let state = AppState::open(&data_dir)?;
            app.manage(Arc::new(state));
            if let Some(window) = app.get_webview_window("main") {
                turn_off_saved_form_data(&window);
            }
            let handle = app.handle().clone();
            if commands::hotkey_enabled(&handle) {
                register_summon_shortcut(&handle);
            }
            build_tray_icon(app)?;
            importer::finish_pending_refresh(handle.clone());
            importer::start_background_watch(handle);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if commands::close_to_tray_enabled(window.app_handle()) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::show_main_window,
            commands::list_chats,
            commands::get_thread,
            commands::rename_chat,
            commands::set_chat_pinned,
            commands::delete_chat,
            commands::switch_branch,
            commands::send_message,
            commands::regenerate_message,
            commands::edit_message,
            commands::cancel_generation,
            commands::search,
            commands::list_providers,
            commands::add_key,
            commands::delete_key,
            commands::add_custom_provider,
            commands::delete_custom_provider,
            commands::refresh_models,
            commands::list_models,
            commands::read_attachment,
            commands::read_image,
            commands::get_settings,
            commands::set_setting,
            commands::usage_summary,
            commands::request_log,
            commands::clear_request_log,
            commands::list_prices,
            commands::set_price,
            commands::scan_import_sources,
            commands::start_import,
            commands::export_chat,
            commands::set_chat_access,
            commands::answer_tool_request,
            commands::list_skills,
            commands::set_skill_enabled,
            commands::add_skill_folder,
            commands::list_plugins,
            commands::set_plugin_enabled,
            commands::list_mcp_servers,
            commands::check_mcp_server,
            commands::add_mcp_server,
            commands::remove_mcp_server,
            commands::set_mcp_server_enabled,
            commands::voice_status,
            commands::install_voice,
            commands::prepare_speech,
            commands::transcribe_audio,
            commands::release_speech,
            commands::fork_chat,
            commands::sync_chat,
            commands::sync_all_chats,
            commands::context_usage,
            commands::compact_chat,
            commands::memory_files,
            commands::own_memory,
            commands::save_own_memory,
            commands::send_to_claude_code,
            commands::undo_claude_code_replace,
            commands::rewind_preview,
            commands::rewind_chat,
            commands::undo_file_restore,
            commands::background_tasks,
            commands::background_output,
            commands::stop_background_task,
            commands::terminal_view,
            commands::terminal_run,
            commands::terminal_stop,
            commands::terminal_clear,
            commands::browser_place,
            commands::browser_go,
            commands::browser_view,
            commands::list_folder,
            commands::changed_files,
            commands::file_view,
            commands::chat_plan,
            commands::chat_artifacts,
            commands::artifact_text,
            commands::save_artifact_copy,
            commands::open_artifact_in_browser,
            commands::widget_page,
            commands::save_widget_picture,
            commands::bot_sim_start,
            commands::bot_sim_view,
            commands::bot_sim_send,
            commands::bot_sim_send_photo,
            commands::bot_sim_press,
            commands::bot_sim_reset,
            commands::bot_sim_stop,
        ])
        .build(tauri::generate_context!())
        .expect("приложение должно запускаться")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                app.state::<Arc<AppState>>().stop_everything_started();
            }
        });
}
