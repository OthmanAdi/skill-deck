// @agent-context: Terminal context detection commands.
// Detects which terminal is focused and its current working directory.
// This is the most platform-specific code in the app.
//
// PLATFORM STATUS:
// - Windows: GetForegroundWindow + process tree walk + PEB read (cmd.exe works, PowerShell limited)
// - macOS: TODO stub in v0.1
// - Linux X11/Wayland: TODO stub in v0.1
//
// TODO(v2): OSC 9;9 shell integration for accurate PowerShell CWD

use serde::{Deserialize, Serialize};

/// Information about the currently focused terminal
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalContext {
    /// Whether a terminal is currently focused
    pub is_terminal_focused: bool,
    /// Name of the terminal app (e.g., "Windows Terminal", "iTerm2")
    pub terminal_name: Option<String>,
    /// Current working directory of the shell running in the terminal
    pub cwd: Option<String>,
    /// PID of the shell process
    pub shell_pid: Option<u32>,
}

/// Detect the currently focused terminal and its CWD.
/// Returns None fields if no terminal is focused or CWD can't be determined.
#[tauri::command]
pub fn detect_terminal_context() -> TerminalContext {
    // Platform-specific implementation
    #[cfg(target_os = "windows")]
    {
        detect_windows()
    }
    #[cfg(target_os = "macos")]
    {
        detect_macos()
    }
    #[cfg(target_os = "linux")]
    {
        detect_linux()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        TerminalContext {
            is_terminal_focused: false,
            terminal_name: None,
            cwd: None,
            shell_pid: None,
        }
    }
}

/// Detect terminal context for a known terminal host PID.
/// Used during drag/drop so injection decisions are based on the drop target, not stale UI state.
pub(crate) fn detect_terminal_context_for_pid(pid: u32) -> TerminalContext {
    #[cfg(target_os = "windows")]
    {
        detect_windows_for_pid(pid)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = pid;
        empty_context()
    }
}

fn empty_context() -> TerminalContext {
    TerminalContext {
        is_terminal_focused: false,
        terminal_name: None,
        cwd: None,
        shell_pid: None,
    }
}

// ── Windows implementation ───────────────────────────────────────────────────

// @agent-context: Windows CWD detection fallback chain.
// sysinfo's .cwd() is unreliable on Windows — it often returns None for shell
// child processes of Windows Terminal because it needs SeDebugPrivilege.
//
// Fallback chain for reading CWD of a shell process:
//   1. sysinfo .cwd() — targeted refresh on the specific process (cheap, works sometimes)
//   2. NtQueryInformationProcess + ReadProcessMemory to read CurrentDirectory
//      from the PEB's RTL_USER_PROCESS_PARAMETERS (reliable for cmd/pwsh/bash)
//   3. /proc/{pid}/cwd symlink — works for MSYS2/Git Bash under mintty
//   4. std::env::current_dir() — last resort, returns OUR process's CWD

#[cfg(target_os = "windows")]
fn detect_windows() -> TerminalContext {
    // Get foreground window PID using Windows API
    let fg_pid = match get_foreground_pid_windows() {
        Some(pid) => pid,
        None => return empty_context(),
    };

    detect_windows_for_pid(fg_pid)
}

#[cfg(target_os = "windows")]
fn detect_windows_for_pid(pid: u32) -> TerminalContext {
    use sysinfo::System;

    // @agent-context: We do a targeted refresh here. Refreshing ALL processes is expensive
    // (~50ms) and unnecessary. We only need the foreground process and its children.
    // However, sysinfo requires a full process list to walk the tree via parent PIDs,
    // so we refresh all but with update_kind=false to skip expensive per-process data.
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let process = sys.process(sysinfo::Pid::from_u32(pid));
    match process {
        Some(proc) => {
            let exe_name = proc.name().to_string_lossy().to_lowercase();
            let is_terminal = is_known_terminal_windows(&exe_name);

            if !is_terminal {
                return empty_context();
            }

            // @agent-context: Walk the process tree to find the shell child.
            // Windows Terminal's tree: WindowsTerminal.exe → OpenConsoleProxy.exe → shell
            // VS Code's tree: Code.exe → ... → shell
            // Plain cmd: cmd.exe is both terminal and shell
            let shell_pid = find_shell_child(&sys, sysinfo::Pid::from_u32(pid));

            // If the foreground process itself is a shell (e.g., standalone cmd.exe/pwsh.exe),
            // use it directly
            let effective_shell_pid = shell_pid.or_else(|| {
                if is_shell_process(&exe_name) {
                    Some(sysinfo::Pid::from_u32(pid))
                } else {
                    None
                }
            });

            let cwd = effective_shell_pid
                .map(|spid| spid.as_u32())
                .and_then(|pid_u32| read_cwd_windows(&sys, pid_u32));

            TerminalContext {
                is_terminal_focused: true,
                terminal_name: Some(exe_name),
                cwd,
                shell_pid: effective_shell_pid.map(|p| p.as_u32()),
            }
        }
        None => empty_context(),
    }
}

#[cfg(target_os = "windows")]
fn get_foreground_pid_windows() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid > 0 {
            Some(pid)
        } else {
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn is_known_terminal_windows(exe_name: &str) -> bool {
    // @agent-context: Comprehensive list of Windows terminal emulators.
    // "wt.exe" is the Windows Terminal process name in some installations.
    // Includes both the terminal host processes and standalone shell processes
    // (cmd.exe, pwsh.exe) which can be their own terminal windows.
    const TERMINALS: &[&str] = &[
        "windowsterminal.exe",
        "wt.exe",
        "cmd.exe",
        "powershell.exe",
        "pwsh.exe",
        "mintty.exe",
        "alacritty.exe",
        "wezterm-gui.exe",
        "kitty.exe",
        "hyper.exe",
        "tabby.exe",
        "conemu64.exe",
        "conemu.exe",
        "terminus.exe",
        "fluent-terminal.exe",
        "rio.exe",
    ];
    TERMINALS.iter().any(|t| exe_name.contains(t))
}

/// Check if a process name is a shell (as opposed to a terminal emulator)
#[cfg(target_os = "windows")]
fn is_shell_process(exe_name: &str) -> bool {
    const SHELLS: &[&str] = &[
        "bash",
        "zsh",
        "fish",
        "sh",
        "powershell",
        "pwsh",
        "cmd",
        "nu",
        "elvish",
        "xonsh",
        "dash",
        "nushell",
    ];
    SHELLS.iter().any(|s| exe_name.contains(s))
}

// @agent-context: CWD fallback chain for Windows.
// This is the core reliability improvement. Each method targets different scenarios:
//   1. sysinfo — works when our process has sufficient privileges
//   2. PEB read — works for most processes, reads CurrentDirectory from process memory
//   3. /proc/PID/cwd — MSYS2 / Git Bash specific
//   4. current_dir — our own CWD as last resort
#[cfg(target_os = "windows")]
fn read_cwd_windows(sys: &sysinfo::System, pid: u32) -> Option<String> {
    // Strategy 1: Try sysinfo .cwd() — sometimes works, especially with elevated privileges
    if let Some(cwd) = try_sysinfo_cwd(sys, pid) {
        return Some(cwd);
    }

    // Strategy 2: Read CWD from Process Environment Block (PEB) via NtQueryInformationProcess
    // This is the most reliable method — it reads CurrentDirectory directly from process memory
    if let Some(cwd) = try_peb_cwd(pid) {
        return Some(cwd);
    }

    // Strategy 3: Try /proc/PID/cwd symlink (works for MSYS2/Git Bash processes)
    if let Some(cwd) = try_proc_cwd(pid) {
        return Some(cwd);
    }

    // Do not return Skill Deck's own process CWD. A missing terminal CWD is safer than a wrong project.
    None
}

/// Try reading CWD via sysinfo's .cwd() method
#[cfg(target_os = "windows")]
fn try_sysinfo_cwd(sys: &sysinfo::System, pid: u32) -> Option<String> {
    sys.process(sysinfo::Pid::from_u32(pid))
        .and_then(|p| p.cwd().map(|c| c.to_string_lossy().to_string()))
        .filter(|s| !s.is_empty())
}

/// Read the CurrentDirectory from a process's PEB (Process Environment Block).
///
/// This works by:
/// 1. Opening the target process with PROCESS_QUERY_INFORMATION | PROCESS_VM_READ
/// 2. Calling NtQueryInformationProcess to get the PEB address
/// 3. Reading PEB.ProcessParameters pointer via ReadProcessMemory
/// 4. Reading RTL_USER_PROCESS_PARAMETERS.CurrentDirectory.Buffer
///
/// This is the same technique that Process Explorer and Process Hacker use.
#[cfg(target_os = "windows")]
fn try_peb_cwd(pid: u32) -> Option<String> {
    use std::mem;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    // @agent-context: NtQueryInformationProcess is not in the `windows` crate's safe bindings.
    // We call it via raw FFI from ntdll.dll. This is standard practice — Process Explorer,
    // Process Hacker, and .NET's Process class all do the same thing.
    //
    // The PEB and RTL_USER_PROCESS_PARAMETERS offsets below are for 64-bit Windows only.
    // On 32-bit Windows the offsets differ (PEB.ProcessParameters at 0x10, CurrentDirectory
    // at 0x24). Bail out on non-64-bit pointer widths.
    if mem::size_of::<usize>() != 8 {
        return None;
    }

    // ProcessBasicInformation = 0
    const PROCESS_BASIC_INFORMATION: u32 = 0;

    #[repr(C)]
    struct ProcessBasicInformation {
        reserved1: usize,
        peb_base_address: usize,
        reserved2: [usize; 2],
        unique_process_id: usize,
        reserved3: usize,
    }

    // @agent-context: UNICODE_STRING and RTL_USER_PROCESS_PARAMETERS layout.
    // We only need the CurrentDirectory field, which is at a known offset in the struct.
    // On 64-bit Windows:
    //   RTL_USER_PROCESS_PARAMETERS + 0x38 = CurrentDirectory.DosPath (UNICODE_STRING)
    //   UNICODE_STRING = { Length: u16, MaximumLength: u16, padding: u32, Buffer: *u16 }
    #[repr(C)]
    struct UnicodeString {
        length: u16, // in bytes, not chars
        maximum_length: u16,
        _padding: u32, // alignment padding on 64-bit
        buffer: usize, // pointer to wide char buffer
    }

    // Load NtQueryInformationProcess from ntdll.dll
    type NtQueryInformationProcessFn = unsafe extern "system" fn(
        process_handle: HANDLE,
        process_information_class: u32,
        process_information: *mut std::ffi::c_void,
        process_information_length: u32,
        return_length: *mut u32,
    ) -> i32; // NTSTATUS

    let nt_query = unsafe {
        let module = windows::core::s!("ntdll.dll");
        let lib = windows::Win32::System::LibraryLoader::GetModuleHandleA(module).ok()?;
        let proc_name = windows::core::s!("NtQueryInformationProcess");
        let addr = windows::Win32::System::LibraryLoader::GetProcAddress(lib, proc_name)?;
        mem::transmute::<unsafe extern "system" fn() -> isize, NtQueryInformationProcessFn>(addr)
    };

    // Open the target process
    let process_handle =
        unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()? };

    // Ensure we close the handle when done
    struct HandleGuard(HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    let _guard = HandleGuard(process_handle);

    // Step 1: Get PEB address via NtQueryInformationProcess
    let mut pbi: ProcessBasicInformation = unsafe { mem::zeroed() };
    let mut return_length: u32 = 0;
    let status = unsafe {
        nt_query(
            process_handle,
            PROCESS_BASIC_INFORMATION,
            &mut pbi as *mut _ as *mut std::ffi::c_void,
            mem::size_of::<ProcessBasicInformation>() as u32,
            &mut return_length,
        )
    };

    // NTSTATUS: 0 = STATUS_SUCCESS
    if status != 0 || pbi.peb_base_address == 0 {
        return None;
    }

    // Step 2: Read PEB to get ProcessParameters pointer
    // PEB.ProcessParameters is at offset 0x20 on 64-bit Windows
    let process_params_ptr_addr = pbi.peb_base_address + 0x20;
    let mut process_params_addr: usize = 0;
    let ok = unsafe {
        ReadProcessMemory(
            process_handle,
            process_params_ptr_addr as *const std::ffi::c_void,
            &mut process_params_addr as *mut _ as *mut std::ffi::c_void,
            mem::size_of::<usize>(),
            None,
        )
    };
    if ok.is_err() || process_params_addr == 0 {
        return None;
    }

    // Step 3: Read CurrentDirectory.DosPath (UNICODE_STRING) from RTL_USER_PROCESS_PARAMETERS
    // Offset 0x38 on 64-bit Windows
    let current_dir_offset = process_params_addr + 0x38;
    let mut unicode_str: UnicodeString = unsafe { mem::zeroed() };
    let ok = unsafe {
        ReadProcessMemory(
            process_handle,
            current_dir_offset as *const std::ffi::c_void,
            &mut unicode_str as *mut _ as *mut std::ffi::c_void,
            mem::size_of::<UnicodeString>(),
            None,
        )
    };
    if ok.is_err() || unicode_str.buffer == 0 || unicode_str.length == 0 {
        return None;
    }

    // Step 4: Read the actual wide string buffer
    let char_count = (unicode_str.length / 2) as usize;
    if char_count == 0 || char_count > 32768 {
        return None;
    }
    let mut wide_buf: Vec<u16> = vec![0u16; char_count];
    let ok = unsafe {
        ReadProcessMemory(
            process_handle,
            unicode_str.buffer as *const std::ffi::c_void,
            wide_buf.as_mut_ptr() as *mut std::ffi::c_void,
            char_count * 2,
            None,
        )
    };
    if ok.is_err() {
        return None;
    }

    // Convert wide string to Rust String
    let path = String::from_utf16_lossy(&wide_buf);
    // Trim trailing backslash if present (e.g., "C:\Users\foo\" -> "C:\Users\foo")
    // But keep root paths like "C:\" intact
    let path = path.trim_end_matches('\\');
    let path = if path.len() == 2 && path.ends_with(':') {
        // Root of a drive like "C:" — add backslash back
        format!("{}\\", path)
    } else {
        path.to_string()
    };

    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

/// Try reading CWD via /proc/PID/cwd symlink (MSYS2/Git Bash)
#[cfg(target_os = "windows")]
fn try_proc_cwd(pid: u32) -> Option<String> {
    // MSYS2/Git Bash provides a /proc filesystem
    let proc_path = format!("/proc/{}/cwd", pid);
    std::fs::read_link(&proc_path)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
}

// ── macOS implementation ─────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn detect_macos() -> TerminalContext {
    // TODO(v1): Implement using NSWorkspace.frontmostApplication + proc_pidinfo
    // For now, return a placeholder that the frontend can handle
    TerminalContext {
        is_terminal_focused: false,
        terminal_name: None,
        cwd: None,
        shell_pid: None,
    }
}

// ── Linux implementation ─────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn detect_linux() -> TerminalContext {
    // TODO(v1): Implement using X11 _NET_ACTIVE_WINDOW + /proc/<pid>/cwd
    // For now, return a placeholder
    TerminalContext {
        is_terminal_focused: false,
        terminal_name: None,
        cwd: None,
        shell_pid: None,
    }
}

// ── Shared helpers ───────────────────────────────────────────────────────────

/// Walk the process tree to find the deepest shell child of a terminal process.
///
/// @agent-context: Windows Terminal's process tree is deeper than other terminals:
///   WindowsTerminal.exe → conhost.exe/OpenConsoleProxy.exe → shell (pwsh/bash/cmd)
/// VS Code's tree:
///   Code.exe → ... → node.exe → shell
/// Plain terminal:
///   cmd.exe (is both terminal and shell)
///   mintty.exe → bash.exe
///
/// The algorithm does a breadth-first search for shell processes, then recurses
/// into each shell to find the deepest one (handles tmux, screen, nested shells).
#[cfg(target_os = "windows")]
fn find_shell_child(sys: &sysinfo::System, parent_pid: sysinfo::Pid) -> Option<sysinfo::Pid> {
    const SHELLS: &[&str] = &[
        "bash",
        "zsh",
        "fish",
        "sh",
        "powershell",
        "pwsh",
        "cmd",
        "nu",
        "nushell",
        "elvish",
        "xonsh",
        "dash",
    ];

    // @agent-context: Intermediate processes that are NOT shells but appear between
    // the terminal and the actual shell. We should look through these, not stop at them.
    const INTERMEDIARIES: &[&str] = &[
        "conhost",
        "openconsoleproxy",
        "conpty",
        "winpty-agent",
        "node",
        "electron",
        "wslhost",
        "wsl",
    ];

    // Find all direct children of the parent
    let children: Vec<_> = sys
        .processes()
        .iter()
        .filter(|(_, p)| p.parent() == Some(parent_pid))
        .collect();

    // First pass: look for shell processes among direct children
    let mut found_shell: Option<sysinfo::Pid> = None;
    for (pid, proc) in &children {
        let name = proc.name().to_string_lossy().to_lowercase();
        // Strip .exe suffix for matching
        let name_stem = name.trim_end_matches(".exe");
        if SHELLS.iter().any(|s| name_stem == *s || name.contains(s)) {
            // Found a shell — recurse to find the deepest shell in the chain
            // (handles nested shells, tmux, etc.)
            let deeper = find_shell_child(sys, **pid);
            found_shell = Some(deeper.unwrap_or(**pid));
            break;
        }
    }

    if found_shell.is_some() {
        return found_shell;
    }

    // Second pass: recurse through intermediary processes (conhost, OpenConsoleProxy, etc.)
    // These sit between the terminal and the shell in the process tree
    for (pid, proc) in &children {
        let name = proc.name().to_string_lossy().to_lowercase();
        let name_stem = name.trim_end_matches(".exe");
        let is_intermediary = INTERMEDIARIES
            .iter()
            .any(|i| name_stem == *i || name.contains(i));

        if is_intermediary {
            if let Some(shell) = find_shell_child(sys, **pid) {
                return Some(shell);
            }
        }
    }

    // Third pass: if no shell and no known intermediary found, try all children
    // This handles unknown intermediary processes in custom terminal setups
    for (pid, _) in &children {
        if let Some(shell) = find_shell_child(sys, **pid) {
            return Some(shell);
        }
    }

    None
}
