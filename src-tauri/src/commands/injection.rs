// @agent-context: Tauri commands for drag-and-drop terminal injection.
// Exposes window-at-cursor detection and content injection to the frontend.

use crate::detection::window_at_point::{self, WindowAtPoint};
use crate::detection::terminal_inject::{self, InjectionResult};

/// Get information about the window currently under the mouse cursor.
/// Called during drag to determine if the user is hovering over a terminal.
#[tauri::command]
pub fn get_window_at_cursor() -> WindowAtPoint {
    window_at_point::get_window_at_cursor()
}

/// Inject text content into a terminal window identified by PID.
/// Called when the user drops a skill card onto a terminal.
#[tauri::command]
pub fn inject_to_terminal(content: String, target_pid: u32) -> InjectionResult {
    terminal_inject::inject_to_terminal(&content, target_pid)
}
