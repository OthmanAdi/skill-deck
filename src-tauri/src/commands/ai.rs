// @agent-context: Tauri IPC commands for the AI subsystem.
//
// Flow: frontend calls `ai_chat_send` with the active provider + model.
// The command resolves the provider via the registry, runs `run_agent_turn`,
// and streams events via `app_handle.emit("ai-agent-event", payload)`.

use crate::ai::{
    self, build_provider, AgentTurnRequest, AgentTurnResult, ProviderConfig,
    ProviderRegistrySnapshot,
};
use crate::commands::preferences::{save_config, ConfigState};
use crate::commands::skills::scan_skills;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHealthResult {
    pub ok: bool,
    pub detail: Option<String>,
}

#[tauri::command]
pub async fn ai_detect_ollama() -> Result<Option<ai::ollama::DetectedOllama>, String> {
    Ok(ai::ollama::detect_local().await)
}

#[tauri::command]
pub fn ai_list_providers(state: State<'_, ConfigState>) -> ProviderRegistrySnapshot {
    let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    ProviderRegistrySnapshot {
        providers: guard.ai_providers.clone(),
        active_provider: guard.ai_active_provider.clone(),
        active_model: guard.ai_active_model.clone(),
    }
}

#[tauri::command]
pub fn ai_save_provider(
    state: State<'_, ConfigState>,
    config: ProviderConfig,
) -> Result<ProviderRegistrySnapshot, String> {
    let snapshot = {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        if let Some(idx) = guard.ai_providers.iter().position(|p| p.id == config.id) {
            guard.ai_providers[idx] = config;
        } else {
            guard.ai_providers.push(config);
        }
        ProviderRegistrySnapshot {
            providers: guard.ai_providers.clone(),
            active_provider: guard.ai_active_provider.clone(),
            active_model: guard.ai_active_model.clone(),
        }
    };
    let cfg_clone = state
        .0
        .lock()
        .map(|g| g.clone())
        .map_err(|e| e.to_string())?;
    save_config(&cfg_clone)?;
    Ok(snapshot)
}

#[tauri::command]
pub fn ai_delete_provider(
    state: State<'_, ConfigState>,
    provider_id: String,
) -> Result<ProviderRegistrySnapshot, String> {
    let snapshot = {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        guard.ai_providers.retain(|p| p.id != provider_id);
        if guard.ai_active_provider.as_deref() == Some(provider_id.as_str()) {
            guard.ai_active_provider = None;
            guard.ai_active_model = None;
        }
        ProviderRegistrySnapshot {
            providers: guard.ai_providers.clone(),
            active_provider: guard.ai_active_provider.clone(),
            active_model: guard.ai_active_model.clone(),
        }
    };
    let cfg_clone = state
        .0
        .lock()
        .map(|g| g.clone())
        .map_err(|e| e.to_string())?;
    save_config(&cfg_clone)?;
    Ok(snapshot)
}

#[tauri::command]
pub fn ai_set_active_selection(
    state: State<'_, ConfigState>,
    provider_id: Option<String>,
    model: Option<String>,
) -> Result<ProviderRegistrySnapshot, String> {
    let snapshot = {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        guard.ai_active_provider = provider_id;
        guard.ai_active_model = model;
        ProviderRegistrySnapshot {
            providers: guard.ai_providers.clone(),
            active_provider: guard.ai_active_provider.clone(),
            active_model: guard.ai_active_model.clone(),
        }
    };
    let cfg_clone = state
        .0
        .lock()
        .map(|g| g.clone())
        .map_err(|e| e.to_string())?;
    save_config(&cfg_clone)?;
    Ok(snapshot)
}

#[tauri::command]
pub async fn ai_health(
    state: State<'_, ConfigState>,
    provider_id: String,
) -> Result<AiHealthResult, String> {
    let config = {
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        guard
            .ai_providers
            .iter()
            .find(|p| p.id == provider_id)
            .cloned()
            .ok_or_else(|| format!("provider not found: {}", provider_id))?
    };
    let provider = build_provider(config).map_err(|e| e.user_message())?;
    let h = provider.health().await;
    Ok(AiHealthResult {
        ok: h.ok,
        detail: h.detail,
    })
}

#[tauri::command]
pub async fn ai_list_models(
    state: State<'_, ConfigState>,
    provider_id: String,
) -> Result<Vec<ai::ModelInfo>, String> {
    let config = {
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        guard
            .ai_providers
            .iter()
            .find(|p| p.id == provider_id)
            .cloned()
            .ok_or_else(|| format!("provider not found: {}", provider_id))?
    };
    let provider = build_provider(config).map_err(|e| e.user_message())?;
    provider.list_models().await.map_err(|e| e.user_message())
}

#[tauri::command]
pub async fn ai_chat_send(
    app: AppHandle,
    state: State<'_, ConfigState>,
    session_id: Option<String>,
    user_text: String,
    provider_id: String,
    model: String,
) -> Result<AgentTurnResult, String> {
    let provider_config = {
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        guard
            .ai_providers
            .iter()
            .find(|p| p.id == provider_id)
            .cloned()
            .ok_or_else(|| format!("provider not found: {}", provider_id))?
    };

    let provider = build_provider(provider_config).map_err(|e| e.user_message())?;

    // Snapshot current skills for the tool context. We re-use the existing
    // scan_skills command so the agent sees what the UI sees.
    let skills_state = app.state::<ConfigState>();
    let skills = scan_skills(skills_state).skills;

    let emit_app = app.clone();
    let emit: Arc<dyn Fn(ai::agent::AgentEvent) + Send + Sync> = Arc::new(move |event| {
        let _ = emit_app.emit("ai-agent-event", &event);
    });

    ai::run_agent_turn(
        provider,
        skills,
        AgentTurnRequest {
            session_id,
            user_text,
            provider_id,
            model,
        },
        emit,
    )
    .await
}

#[tauri::command]
pub fn ai_list_sessions() -> Vec<ai::AiSession> {
    ai::session::list()
}

#[tauri::command]
pub fn ai_get_session(session_id: String) -> Result<ai::AiSession, String> {
    ai::session::load(&session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ai_delete_session(session_id: String) -> Result<(), String> {
    ai::session::delete(&session_id).map_err(|e| e.to_string())
}
