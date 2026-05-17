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
mod ai;
mod commands;
mod detection;
mod models;
mod parsers;

use commands::preferences::{load_config, register_overlay_hotkey, ConfigState, HotkeyState};
use std::sync::Mutex;
use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = load_config();
    let startup_overlay_width = config.overlay_width.clamp(380, 2400) as f64;
    let startup_overlay_height = config.overlay_height.clamp(560, 1800) as f64;
    let startup_overlay_mode = config.overlay_mode.clone();
    let startup_hotkey = config.hotkey.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_fs::init())
        .manage(ConfigState(Mutex::new(config)))
        .manage(HotkeyState(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            // Skill scanning
            commands::scan_skills,
            commands::list_agents,
            commands::read_skill_content,
            // Preferences
            commands::toggle_star,
            commands::set_skill_icon,
            commands::get_config,
            commands::set_hotkey,
            commands::set_theme,
            commands::set_overlay_mode,
            commands::set_overlay_size,
            commands::set_font_scale,
            commands::set_finder_open,
            commands::set_skill_sort_mode,
            commands::set_collapsed_agents,
            commands::set_collapsed_tree_nodes,
            commands::get_starred_skills,
            // Update checking
            commands::check_skill_update,
            commands::set_skill_repo,
            commands::set_skill_install_command,
            commands::snapshot_skill_before_update,
            commands::list_skill_versions,
            commands::restore_skill_version,
            commands::delete_skill_version,
            commands::read_skill_snapshot,
            commands::search_marketplace,
            commands::search_marketplaces_aggregated,
            // AI
            commands::ai_detect_ollama,
            commands::ai_list_providers,
            commands::ai_save_provider,
            commands::ai_delete_provider,
            commands::ai_set_active_selection,
            commands::ai_health,
            commands::ai_list_models,
            commands::ai_chat_send,
            commands::ai_cancel_turn,
            commands::ai_list_sessions,
            commands::ai_get_session,
            commands::ai_delete_session,
        ])
        .setup(move |app| {
            // The overlay window starts hidden — shown on global hotkey or tray click
            let window = match app.get_webview_window("overlay") {
                Some(window) => window,
                None => return Err("overlay window not found in tauri.conf.json".into()),
            };

            // Position overlay at bottom-right of primary monitor
            if let Ok(Some(monitor)) = window.primary_monitor() {
                let monitor_size = monitor.size();
                let scale = monitor.scale_factor();
                let w = startup_overlay_width;
                let h = startup_overlay_height;

                let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(w, h)));
                let x = (monitor_size.width as f64 / scale) - w - 16.0;
                let y = (monitor_size.height as f64 / scale) - h - 48.0;
                window.set_position(tauri::LogicalPosition::new(x, y)).ok();
            }

            let _ = window.set_always_on_top(startup_overlay_mode != "auto-hide");

            {
                if let Ok(active) =
                    register_overlay_hotkey(app.handle(), &startup_hotkey, None, true)
                {
                    let state = app.state::<ConfigState>();
                    if let Ok(mut cfg) = state.0.lock() {
                        if cfg.hotkey != active {
                            cfg.hotkey = active.clone();
                            let _ = commands::preferences::save_config(&cfg);
                        }
                    }

                    let hotkey_state = app.state::<HotkeyState>();
                    if let Ok(mut current) = hotkey_state.0.lock() {
                        *current = Some(active);
                    };
                } else {
                    eprintln!("skill-deck: no global hotkey could be registered");
                }
            }

            // @agent-context: Tray icon with left-click toggle and right-click context menu.
            let show_item = MenuItemBuilder::with_id("show", "Show / Hide").build(app)?;
            let mode_pinned_item = CheckMenuItemBuilder::with_id("mode_pinned", "Pinned mode")
                .checked(startup_overlay_mode != "auto-hide")
                .build(app)?;
            let mode_auto_hide_item =
                CheckMenuItemBuilder::with_id("mode_auto_hide", "Auto-hide mode")
                    .checked(startup_overlay_mode == "auto-hide")
                    .build(app)?;
            let rescan_item = MenuItemBuilder::with_id("rescan", "Rescan Skills").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit Skill Deck").build(app)?;

            let tray_menu = MenuBuilder::new(app)
                .item(&show_item)
                .separator()
                .item(&mode_pinned_item)
                .item(&mode_auto_hide_item)
                .separator()
                .item(&rescan_item)
                .item(&quit_item)
                .build()?;

            let window_for_tray = window.clone();
            let window_for_menu = window.clone();
            let default_icon = app
                .default_window_icon()
                .ok_or_else(|| "default window icon missing".to_string())?
                .clone();

            TrayIconBuilder::new()
                .tooltip("Skill Deck — Ctrl+Shift+K")
                .icon(default_icon)
                .menu(&tray_menu)
                .on_tray_icon_event(move |_tray, event| {
                    // Only toggle on left-click — right-click opens the context menu
                    if let tauri::tray::TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if window_for_tray.is_visible().unwrap_or(false) {
                            let _ = window_for_tray.hide();
                            let _ = window_for_tray.emit("overlay-visibility-changed", false);
                        } else {
                            let _ = window_for_tray.show();
                            let _ = window_for_tray.set_focus();
                            let _ = window_for_tray.emit("overlay-visibility-changed", true);
                        }
                    }
                })
                .on_menu_event(move |app_handle, event| match event.id().as_ref() {
                    "show" => {
                        if window_for_menu.is_visible().unwrap_or(false) {
                            let _ = window_for_menu.hide();
                            let _ = window_for_menu.emit("overlay-visibility-changed", false);
                        } else {
                            let _ = window_for_menu.show();
                            let _ = window_for_menu.set_focus();
                            let _ = window_for_menu.emit("overlay-visibility-changed", true);
                        }
                    }
                    "mode_pinned" => {
                        let state = app_handle.state::<ConfigState>();
                        if let Ok(mut cfg) = state.0.lock() {
                            cfg.overlay_mode = "pinned".to_string();
                            let _ = commands::preferences::save_config(&cfg);
                        }
                        let _ = window_for_menu.set_always_on_top(true);
                        let _ = window_for_menu.emit("overlay-mode-changed", "pinned");
                        let _ = mode_pinned_item.set_checked(true);
                        let _ = mode_auto_hide_item.set_checked(false);
                    }
                    "mode_auto_hide" => {
                        let state = app_handle.state::<ConfigState>();
                        if let Ok(mut cfg) = state.0.lock() {
                            cfg.overlay_mode = "auto-hide".to_string();
                            let _ = commands::preferences::save_config(&cfg);
                        }
                        let _ = window_for_menu.set_always_on_top(false);
                        let _ = window_for_menu.emit("overlay-mode-changed", "auto-hide");
                        let _ = mode_auto_hide_item.set_checked(true);
                        let _ = mode_pinned_item.set_checked(false);
                    }
                    "rescan" => {
                        let _ = window_for_menu.show();
                        let _ = window_for_menu.set_focus();
                        let _ = window_for_menu.emit("overlay-visibility-changed", true);
                        let _ = window_for_menu.eval("window.location.reload()");
                    }
                    "quit" => {
                        app_handle.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| eprintln!("error while running Skill Deck: {}", e));
}
