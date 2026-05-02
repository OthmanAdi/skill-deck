// @agent-context: Terminal content injection via clipboard + synthetic keystroke.
//
// STRATEGY:
// 1. Write the skill content/reference to the clipboard
// 2. Bring the target terminal window to foreground
// 3. Send Ctrl+V (paste) keystroke to the terminal
//
// PLATFORM STATUS:
// - Windows: SetClipboardData + SetForegroundWindow + SendInput(Ctrl+V)
// - macOS: stub (TODO: NSPasteboard + osascript)
// - Linux: stub (TODO: xdotool type or xclip + xdotool key)

use serde::{Deserialize, Serialize};

/// Result of an injection attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectionResult {
    pub success: bool,
    pub error: Option<String>,
    pub reference: Option<String>,
    pub reference_kind: Option<String>,
}

/// Inject text content into a terminal window identified by PID.
///
/// The injection uses clipboard paste: write to clipboard, focus window, send Ctrl+V.
/// This is the most reliable cross-terminal method — all terminals support Ctrl+V paste.
pub fn inject_to_terminal(content: &str, target_pid: u32) -> InjectionResult {
    #[cfg(target_os = "windows")]
    {
        if !is_terminal_pid_windows(target_pid) {
            return InjectionResult {
                success: false,
                error: Some(format!(
                    "Refusing to inject into non-terminal PID {}",
                    target_pid
                )),
                reference: None,
                reference_kind: None,
            };
        }
    }

    #[cfg(target_os = "windows")]
    {
        inject_windows(content, target_pid)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (content, target_pid);
        InjectionResult {
            success: false,
            error: Some("macOS injection not yet implemented".to_string()),
            reference: None,
            reference_kind: None,
        }
    }
    #[cfg(target_os = "linux")]
    {
        let _ = (content, target_pid);
        InjectionResult {
            success: false,
            error: Some("Linux injection not yet implemented".to_string()),
            reference: None,
            reference_kind: None,
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (content, target_pid);
        InjectionResult {
            success: false,
            error: Some("Platform not supported".to_string()),
            reference: None,
            reference_kind: None,
        }
    }
}

#[cfg(target_os = "windows")]
fn is_terminal_pid_windows(target_pid: u32) -> bool {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(target_pid)]),
        true,
    );

    let process_name = sys
        .process(sysinfo::Pid::from_u32(target_pid))
        .map(|p| p.name().to_string_lossy().to_lowercase());

    match process_name {
        Some(name) => is_terminal_process_name(&name),
        None => false,
    }
}

#[cfg(target_os = "windows")]
fn is_terminal_process_name(exe_name: &str) -> bool {
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
    ];
    TERMINALS.iter().any(|t| exe_name.contains(t))
}

// ── Windows implementation ──────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn inject_windows(content: &str, target_pid: u32) -> InjectionResult {
    use std::mem;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

    // Step 1: Find the window belonging to target_pid
    let hwnd = find_window_for_pid_impl(target_pid);
    let hwnd = match hwnd {
        Some(h) => h,
        None => {
            return InjectionResult {
                success: false,
                error: Some(format!("No visible window found for PID {}", target_pid)),
                reference: None,
                reference_kind: None,
            }
        }
    };

    // Step 2: Write content to clipboard as CF_UNICODETEXT
    let wide: Vec<u16> = content.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * 2;

    unsafe {
        // Open clipboard
        let mut opened = false;
        for _ in 0..8 {
            if OpenClipboard(None).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(35));
        }
        if !opened {
            return InjectionResult {
                success: false,
                error: Some("Failed to open clipboard".to_string()),
                reference: None,
                reference_kind: None,
            };
        }

        let _ = EmptyClipboard();

        // Allocate global memory for the clipboard data
        let hmem = match GlobalAlloc(GMEM_MOVEABLE, byte_len) {
            Ok(h) => h,
            Err(e) => {
                let _ = CloseClipboard();
                return InjectionResult {
                    success: false,
                    error: Some(format!("GlobalAlloc failed: {}", e)),
                    reference: None,
                    reference_kind: None,
                };
            }
        };

        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            let _ = CloseClipboard();
            return InjectionResult {
                success: false,
                error: Some("GlobalLock failed".to_string()),
                reference: None,
                reference_kind: None,
            };
        }

        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr as *mut u8, byte_len);
        let _ = GlobalUnlock(hmem);

        // CF_UNICODETEXT = 13
        if let Err(e) = SetClipboardData(13, Some(HANDLE(hmem.0))) {
            let _ = CloseClipboard();
            return InjectionResult {
                success: false,
                error: Some(format!("SetClipboardData failed: {}", e)),
                reference: None,
                reference_kind: None,
            };
        }
        let _ = CloseClipboard();
    }

    // Step 3: Bring target window to foreground
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }

    // Small delay to let the window come to focus
    std::thread::sleep(std::time::Duration::from_millis(100));

    unsafe {
        if GetForegroundWindow() != hwnd {
            return InjectionResult {
                success: false,
                error: Some("Target terminal could not be focused safely".to_string()),
                reference: None,
                reference_kind: None,
            };
        }
    }

    // Step 4: Send Ctrl+V keystroke
    unsafe {
        let make_key_input =
            |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY, up: bool| -> INPUT {
                let mut input: INPUT = mem::zeroed();
                input.r#type = INPUT_KEYBOARD;
                input.Anonymous.ki = KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    time: 0,
                    dwExtraInfo: 0,
                };
                input
            };

        let inputs = [
            make_key_input(VK_CONTROL, false), // Ctrl down
            make_key_input(VK_V, false),       // V down
            make_key_input(VK_V, true),        // V up
            make_key_input(VK_CONTROL, true),  // Ctrl up
        ];

        let sent = SendInput(&inputs, mem::size_of::<INPUT>() as i32);
        if sent != 4 {
            return InjectionResult {
                success: false,
                error: Some(format!("SendInput only sent {} of 4 inputs", sent)),
                reference: None,
                reference_kind: None,
            };
        }
    }

    InjectionResult {
        success: true,
        error: None,
        reference: None,
        reference_kind: None,
    }
}

/// Find a visible window belonging to a given PID
#[cfg(target_os = "windows")]
fn find_window_for_pid_impl(target_pid: u32) -> Option<windows::Win32::Foundation::HWND> {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };

    struct EnumState {
        target_pid: u32,
        found_hwnd: Option<HWND>,
    }

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let state = &mut *(lparam.0 as *mut EnumState);
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        if pid == state.target_pid && IsWindowVisible(hwnd).as_bool() {
            state.found_hwnd = Some(hwnd);
            return windows::core::BOOL(0); // Stop enumeration
        }
        windows::core::BOOL(1) // Continue
    }

    let mut state = EnumState {
        target_pid,
        found_hwnd: None,
    };

    unsafe {
        let _ = EnumWindows(Some(enum_callback), LPARAM(&mut state as *mut _ as isize));
    }

    state.found_hwnd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_injection_result_serialization() {
        let result = InjectionResult {
            success: true,
            error: None,
            reference: None,
            reference_kind: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"success\":true"));
    }

    #[test]
    fn test_injection_result_error() {
        let result = InjectionResult {
            success: false,
            error: Some("test error".to_string()),
            reference: None,
            reference_kind: None,
        };
        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("test error"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_is_terminal_process_name() {
        assert!(is_terminal_process_name("windowsterminal.exe"));
        assert!(is_terminal_process_name("pwsh.exe"));
        assert!(!is_terminal_process_name("notepad.exe"));
        assert!(!is_terminal_process_name("chrome.exe"));
        assert!(!is_terminal_process_name("code.exe"));
    }
}
