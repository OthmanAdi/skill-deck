// @agent-context: Detect which window is under the mouse cursor.
// Used during drag-and-drop to determine if the user is hovering over a terminal.
//
// CRITICAL: Skill Deck is alwaysOnTop=true, so WindowFromPoint() always returns
// our own window. We must skip our own PID and walk the Z-order to find the real
// window underneath the cursor.
//
// PLATFORM STATUS:
// - Windows: EnumWindows Z-order walk, skipping our PID, rect-contains check
// - macOS: stub
// - Linux: stub

use serde::{Deserialize, Serialize};

/// Information about the window under the mouse cursor
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowAtPoint {
    pub found: bool,
    pub is_terminal: bool,
    pub process_name: Option<String>,
    pub window_title: Option<String>,
    pub pid: Option<u32>,
}

impl Default for WindowAtPoint {
    fn default() -> Self {
        Self {
            found: false,
            is_terminal: false,
            process_name: None,
            window_title: None,
            pid: None,
        }
    }
}

/// Get information about the window under the current mouse cursor position,
/// EXCLUDING Skill Deck's own window (which is always on top).
pub fn get_window_at_cursor() -> WindowAtPoint {
    #[cfg(target_os = "windows")]
    {
        get_window_at_cursor_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        WindowAtPoint::default()
    }
}

// ── Windows implementation ──────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn get_window_at_cursor_windows() -> WindowAtPoint {
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetCursorPos, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible, IsIconic,
    };

    // Step 1: Get cursor position
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) }.is_err() {
        return WindowAtPoint::default();
    }

    // Step 2: Our own PID — we must skip windows belonging to this process
    let our_pid = std::process::id();

    // Step 3: Walk ALL top-level windows in Z-order (EnumWindows goes top-to-bottom).
    // Find the first visible, non-minimized, non-us window whose rect contains the cursor.
    struct EnumState {
        cursor: POINT,
        our_pid: u32,
        found_hwnd: Option<HWND>,
    }

    unsafe extern "system" fn enum_callback(
        hwnd: HWND,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::core::BOOL {
        let state = &mut *(lparam.0 as *mut EnumState);

        // Skip invisible and minimized windows
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return windows::core::BOOL(1); // continue
        }

        // Skip our own process
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == state.our_pid {
            return windows::core::BOOL(1); // continue
        }

        // Check if cursor is inside this window's rect
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return windows::core::BOOL(1); // continue
        }

        let inside = state.cursor.x >= rect.left
            && state.cursor.x < rect.right
            && state.cursor.y >= rect.top
            && state.cursor.y < rect.bottom;

        if inside {
            state.found_hwnd = Some(hwnd);
            return windows::core::BOOL(0); // stop enumeration — found it
        }

        windows::core::BOOL(1) // continue
    }

    let mut state = EnumState {
        cursor: point,
        our_pid,
        found_hwnd: None,
    };

    unsafe {
        let _ = EnumWindows(
            Some(enum_callback),
            LPARAM(&mut state as *mut _ as isize),
        );
    }

    let hwnd = match state.found_hwnd {
        Some(h) => h,
        None => return WindowAtPoint::default(),
    };

    // Step 4: Get PID + window title of the found window
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)); }

    if pid == 0 {
        return WindowAtPoint::default();
    }

    let title = {
        let mut buf = [0u16; 256];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
        if len > 0 {
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        } else {
            None
        }
    };

    // Step 5: Look up the process name
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
        true,
    );

    let process_name = sys
        .process(sysinfo::Pid::from_u32(pid))
        .map(|p| p.name().to_string_lossy().to_lowercase());

    let is_terminal = process_name
        .as_ref()
        .map(|n| is_terminal_process(n))
        .unwrap_or(false);

    WindowAtPoint {
        found: true,
        is_terminal,
        process_name: process_name.map(|s| s.to_string()),
        window_title: title,
        pid: Some(pid),
    }
}

#[cfg(target_os = "windows")]
fn is_terminal_process(exe_name: &str) -> bool {
    const TERMINALS: &[&str] = &[
        "windowsterminal",
        "wt",
        "cmd",
        "powershell",
        "pwsh",
        "mintty",
        "alacritty",
        "wezterm",
        "kitty",
        "hyper",
        "tabby",
        "conemu",
        "terminus",
        "fluent-terminal",
        "rio",
        "code", // VS Code integrated terminal
    ];
    TERMINALS.iter().any(|t| exe_name.contains(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_window_at_point() {
        let w = WindowAtPoint::default();
        assert!(!w.found);
        assert!(!w.is_terminal);
        assert!(w.process_name.is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_is_terminal_process() {
        assert!(is_terminal_process("windowsterminal.exe"));
        assert!(is_terminal_process("cmd.exe"));
        assert!(is_terminal_process("pwsh.exe"));
        assert!(is_terminal_process("alacritty.exe"));
        assert!(is_terminal_process("code.exe"));
        assert!(!is_terminal_process("chrome.exe"));
        assert!(!is_terminal_process("notepad.exe"));
        assert!(!is_terminal_process("explorer.exe"));
        assert!(!is_terminal_process("skill-deck.exe"));
    }
}
