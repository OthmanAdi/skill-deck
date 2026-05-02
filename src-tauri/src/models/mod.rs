// @agent-context: Core data models shared across all modules.
// Every struct here is serializable to JSON for Tauri IPC (frontend <-> backend).
// When adding a new agent adapter, you map its native format INTO these models.

pub mod agent;
pub mod config;
pub mod skill;

pub use agent::*;
pub use config::*;
pub use skill::*;
