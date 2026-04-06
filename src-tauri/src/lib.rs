// @agent-context: Main Tauri application entry point.
//
// ARCHITECTURE OVERVIEW:
// ┌─────────────────────────────────────────────────────┐
// │  Svelte 5 Frontend (src/)                           │
// │  - Overlay UI with skill cards                      │
// │  - Calls Rust backend via invoke()                  │
// └──────────────┬──────────────────────────────────────┘
//                │ IPC (JSON-RPC)
// ┌──────────────▼──────────────────────────────────────┐
// │  Tauri Commands (src-tauri/src/commands/)            │
// │  - scan_skills: discover all skills across agents   │
// │  - detect_terminal_context: CWD of focused terminal │
// │  - toggle_star / get_config: user preferences       │
// ├─────────────────────────────────────────────────────┤
// │  Agents (src-tauri/src/agents/)                     │
// │  - registry.rs: 15+ agents with paths and formats   │
// │  - scanner.rs: filesystem glob + parse pipeline     │
// ├─────────────────────────────────────────────────────┤
// │  Parsers (src-tauri/src/parsers/)                   │
// │  - frontmatter.rs: universal YAML+MD parser (90%)  │
// │  - skill_md.rs: SKILL.md format (Claude/Codex)     │
// ├─────────────────────────────────────────────────────┤
// │  Models (src-tauri/src/models/)                     │
// │  - Skill, AgentInfo, AppConfig — JSON-serializable  │
// └─────────────────────────────────────────────────────┘

mod agents;
mod commands;
mod detection;
mod models;
mod parsers;

use commands::preferences::{load_config, ConfigState};
use std::sync::Mutex;
use tauri::Manager;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = load_config();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_fs::init())
        .manage(ConfigState(Mutex::new(config)))
        .invoke_handler(tauri::generate_handler![
            // Skill scanning
            commands::scan_skills,
            commands::list_agents,
            commands::read_skill_content,
            // Terminal context
            commands::detect_terminal_context,
            // Preferences
            commands::toggle_star,
            commands::set_skill_icon,
            commands::get_config,
            commands::set_hotkey,
            commands::set_theme,
            commands::get_starred_skills,
            // Update checking
            commands::check_skill_update,
            commands::set_skill_repo,
            commands::set_skill_install_command,
            // Drag & drop injection
            commands::get_window_at_cursor,
            commands::inject_to_terminal,
        ])
        .setup(|app| {
            // The overlay window starts hidden — shown on global hotkey or tray click
            let window = app.get_webview_window("overlay")
                .expect("overlay window not found in tauri.conf.json");

            // Position overlay at bottom-right of primary monitor
            if let Ok(Some(monitor)) = window.primary_monitor() {
                let monitor_size = monitor.size();
                let scale = monitor.scale_factor();
                let w = 420.0;
                let h = 640.0;
                let x = (monitor_size.width as f64 / scale) - w - 16.0;
                let y = (monitor_size.height as f64 / scale) - h - 48.0;
                window.set_position(tauri::LogicalPosition::new(x, y)).ok();
            }

            // @agent-context: Tray icon with left-click toggle and right-click context menu.
            let show_item = MenuItemBuilder::with_id("show", "Show / Hide")
                .build(app)?;
            let rescan_item = MenuItemBuilder::with_id("rescan", "Rescan Skills")
                .build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit Skill Deck")
                .build(app)?;

            let tray_menu = MenuBuilder::new(app)
                .item(&show_item)
                .item(&rescan_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let window_for_tray = window.clone();
            let window_for_menu = window.clone();
            TrayIconBuilder::new()
                .tooltip("Skill Deck — Ctrl+Shift+K")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
                .on_tray_icon_event(move |_tray, event| {
                    // Only toggle on left-click — right-click opens the context menu
                    if let tauri::tray::TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event {
                        if window_for_tray.is_visible().unwrap_or(false) {
                            let _ = window_for_tray.hide();
                        } else {
                            let _ = window_for_tray.show();
                            let _ = window_for_tray.set_focus();
                        }
                    }
                })
                .on_menu_event(move |app_handle, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if window_for_menu.is_visible().unwrap_or(false) {
                                let _ = window_for_menu.hide();
                            } else {
                                let _ = window_for_menu.show();
                                let _ = window_for_menu.set_focus();
                            }
                        }
                        "rescan" => {
                            let _ = window_for_menu.show();
                            let _ = window_for_menu.set_focus();
                            let _ = window_for_menu.eval("window.location.reload()");
                        }
                        "quit" => {
                            app_handle.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Skill Deck");
}
