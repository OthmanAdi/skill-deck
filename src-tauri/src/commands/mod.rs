// @agent-context: Tauri IPC commands — the API surface between Rust and Svelte.
// Each #[tauri::command] function is callable from the frontend via `invoke()`.
// Commands are registered in lib.rs via `generate_handler![]`.
//
// NAMING CONVENTION: snake_case in Rust, camelCase in TypeScript (auto-converted by Tauri).
// Example: `scan_skills` in Rust → `invoke('scan_skills')` in TypeScript.

pub mod preferences;
pub mod skills;
pub mod updates;

pub use preferences::*;
pub use skills::*;
pub use updates::*;
