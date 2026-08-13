// @agent-context: Tauri IPC for the harness subsystem.
// Replaces the deleted commands/ai.rs surface with `harness_*` naming.
// Streams events to the frontend on `app.emit("harness-event", ...)`.

use crate::commands::preferences::{save_config, ConfigState};
use crate::commands::skills::{scan_for_lookup, SkillCatalogState};
use crate::harness::{
    agent::{run_turn, TurnRequest, TurnSummary},
    builtin::{self, BuiltinRuntime, BuiltinStatus},
    cancel::CancelRegistry,
    config::{HarnessSettings, ProviderConfig},
    error::{HarnessError, HarnessErrorKind},
    hooks::EventSink,
    memory::conversation::ConversationMemory,
    model_directory::{self, ModelProfile},
    providers::{self, DetectedOllama, HealthReport, ModelInfo},
    session::{self, HarnessSession},
    step_policy,
    telemetry::TelemetryWriter,
    tools::HarnessContext,
};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub providers: Vec<ProviderConfig>,
    pub active_provider: Option<String>,
    pub active_model: Option<String>,
}

/// Mask API keys before sending to the frontend. The frontend only needs to
/// know whether a key is set, not the actual value. This prevents credential
/// exposure through DevTools / JS runtime inspection.
/// Char-based slicing: the old byte slice `&k[k.len() - 4..]` panicked when
/// the boundary landed inside a multibyte character (user-pasted keys are
/// arbitrary text).
fn redact_providers(providers: &[ProviderConfig]) -> Vec<ProviderConfig> {
    providers
        .iter()
        .map(|p| ProviderConfig {
            api_key: p.api_key.as_ref().map(|k| redact_key(k)),
            ..p.clone()
        })
        .collect()
}

fn redact_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() > 4 {
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("••••{tail}")
    } else {
        "••••".into()
    }
}

fn snapshot_from(guard: &std::sync::MutexGuard<'_, crate::models::AppConfig>) -> ProviderSnapshot {
    ProviderSnapshot {
        providers: redact_providers(&guard.harness_providers),
        active_provider: guard.harness_active_provider.clone(),
        active_model: guard.harness_active_model.clone(),
    }
}

#[tauri::command]
pub fn harness_list_providers(state: State<'_, ConfigState>) -> ProviderSnapshot {
    let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    snapshot_from(&guard)
}

#[tauri::command]
pub fn harness_save_provider(
    state: State<'_, ConfigState>,
    config: ProviderConfig,
) -> Result<ProviderSnapshot, String> {
    let mut config = config;
    let snap = {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        if let Some(idx) = guard
            .harness_providers
            .iter()
            .position(|p| p.id == config.id)
        {
            // The frontend only ever sees MASKED keys (see redact_providers) —
            // if the incoming key is a mask, the user edited other fields and
            // the stored real key must survive the round-trip.
            if config
                .api_key
                .as_deref()
                .map(|k| k.starts_with("••••"))
                .unwrap_or(false)
            {
                config.api_key = guard.harness_providers[idx].api_key.clone();
            }
            guard.harness_providers[idx] = config;
        } else {
            guard.harness_providers.push(config);
        }
        snapshot_from(&guard)
    };
    let cfg = state
        .0
        .lock()
        .map(|g| g.clone())
        .map_err(|e| e.to_string())?;
    save_config(&cfg)?;
    Ok(snap)
}

#[tauri::command]
pub fn harness_delete_provider(
    state: State<'_, ConfigState>,
    provider_id: String,
) -> Result<ProviderSnapshot, String> {
    let snap = {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        guard.harness_providers.retain(|p| p.id != provider_id);
        if guard.harness_active_provider.as_deref() == Some(provider_id.as_str()) {
            guard.harness_active_provider = None;
            guard.harness_active_model = None;
        }
        snapshot_from(&guard)
    };
    let cfg = state
        .0
        .lock()
        .map(|g| g.clone())
        .map_err(|e| e.to_string())?;
    save_config(&cfg)?;
    Ok(snap)
}

#[tauri::command]
pub fn harness_set_active(
    state: State<'_, ConfigState>,
    provider_id: Option<String>,
    model: Option<String>,
) -> Result<ProviderSnapshot, String> {
    let snap = {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        guard.harness_active_provider = provider_id;
        guard.harness_active_model = model;
        snapshot_from(&guard)
    };
    let cfg = state
        .0
        .lock()
        .map(|g| g.clone())
        .map_err(|e| e.to_string())?;
    save_config(&cfg)?;
    Ok(snap)
}

#[tauri::command]
pub async fn harness_detect_ollama() -> Option<DetectedOllama> {
    providers::detect_local_ollama().await
}

#[tauri::command]
pub async fn harness_health(
    state: State<'_, ConfigState>,
    provider_id: String,
) -> Result<HealthReport, String> {
    let config = {
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        guard
            .harness_providers
            .iter()
            .find(|p| p.id == provider_id)
            .cloned()
            .ok_or_else(|| format!("provider not found: {provider_id}"))?
    };
    let client = providers::resolve(config).map_err(|e| e.to_string())?;
    Ok(client.health().await)
}

#[tauri::command]
pub async fn harness_list_models(
    state: State<'_, ConfigState>,
    provider_id: String,
) -> Result<Vec<ModelInfo>, String> {
    let config = {
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        guard
            .harness_providers
            .iter()
            .find(|p| p.id == provider_id)
            .cloned()
            .ok_or_else(|| format!("provider not found: {provider_id}"))?
    };
    let client = providers::resolve(config).map_err(|e| e.to_string())?;
    client.list_models().await.map_err(|e| e.to_string())
}

/// Send one user turn into the harness.
///
/// Error contract (task: error taxonomy on the IPC boundary): failures reject
/// with a serialized `HarnessError { kind, message }` — `kind` is one of the
/// camelCase `HarnessErrorKind` values (`providerUnreachable`, `providerAuth`,
/// `providerTimeout`, `modelNotFound`, `rateLimited`, `toolFailed`,
/// `cancelled`, `busy`, `internal`), `message` keeps the raw error text.
/// Concurrency contract: one active turn per session — a second send while a
/// turn runs rejects with kind `busy` (documented in `harness/cancel.rs`).
fn destructive_refusal_payload(
    session_id: String,
    user_text: &str,
    provider_id: &str,
    model: &str,
    prior: Option<HarnessSession>,
    now: u64,
) -> Option<(TurnSummary, HarnessSession)> {
    let final_text = step_policy::destructive_intent_refusal(user_text)?;
    let (created_at, title, mut memory) = match prior {
        Some(prior) => (prior.created_at, prior.title, prior.memory),
        None => (
            now,
            user_text.chars().take(60).collect::<String>(),
            ConversationMemory::new(&session_id),
        ),
    };
    memory.push_user(user_text);
    memory.push_assistant(final_text);

    let summary = TurnSummary {
        session_id: session_id.clone(),
        final_text: final_text.to_string(),
        in_tokens: 0,
        out_tokens: 0,
        duration_ms: 0,
        item_count_hint: None,
        tool_calls_made: 0,
    };
    let persisted = HarnessSession {
        id: session_id,
        title,
        provider_id: provider_id.to_string(),
        model: model.to_string(),
        created_at,
        updated_at: now,
        final_summary: memory.running_summary.clone(),
        memory,
    };
    Some((summary, persisted))
}

// Three of the seven parameters are Tauri-injected managed state, not caller
// arguments — the IPC surface the frontend sees is still four fields.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn harness_chat_send(
    app: AppHandle,
    state: State<'_, ConfigState>,
    catalog: State<'_, SkillCatalogState>,
    builtin_runtime: State<'_, BuiltinRuntime>,
    session_id: Option<String>,
    user_text: String,
    provider_id: String,
    model: String,
) -> Result<TurnSummary, HarnessError> {
    let session_id_resolved = match session_id.as_deref() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => format!("session-{}", uuid::Uuid::new_v4()),
    };
    // Admission happens before provider resolution, local sidecar startup, and
    // configured skill scanning. A destructive request therefore has no route to tools
    // even if a small local model would otherwise emit a tool call.
    if step_policy::destructive_intent_refusal(&user_text).is_some() {
        let prior_session = session::load(&session_id_resolved).ok();
        if let Some((summary, refusal_session)) = destructive_refusal_payload(
            session_id_resolved.clone(),
            &user_text,
            &provider_id,
            &model,
            prior_session,
            session::now_secs(),
        ) {
            session::save(&refusal_session).map_err(HarnessError::from)?;
            return Ok(summary);
        }
    }

    let (config, settings) = {
        let guard = state
            .0
            .lock()
            .map_err(|e| HarnessError::new(HarnessErrorKind::Internal, e.to_string()))?;
        let cfg = guard
            .harness_providers
            .iter()
            .find(|p| p.id == provider_id)
            .cloned()
            .ok_or_else(|| {
                HarnessError::new(
                    HarnessErrorKind::Internal,
                    format!("provider not found: {provider_id}"),
                )
            })?;
        (cfg, guard.harness_settings.clone())
    };

    // Auto-start guard: a builtin turn must have a healthy sidecar BEFORE
    // resolve() builds the rig client from the endpoint. `ensure_running`
    // reuses a live sidecar, so this is a cheap check on the steady path.
    let mut config = config;
    if config.kind == "builtin" {
        let exe = resolve_llama_server_exe(&app).map_err(HarnessError::from)?;
        let endpoint = builtin::ensure_running(&builtin_runtime.0, exe)
            .await
            .map_err(HarnessError::from)?;
        config.endpoint = Some(endpoint);
    }

    let client = Arc::new(providers::resolve(config).map_err(HarnessError::from)?);
    let scan = scan_for_lookup(&state, &catalog)
        .map_err(|error| HarnessError::new(HarnessErrorKind::Internal, error))?;
    let ctx =
        HarnessContext::new(scan.scan.skills, settings.clone()).map_err(HarnessError::from)?;

    let telemetry =
        Arc::new(TelemetryWriter::open(&session_id_resolved).map_err(HarnessError::from)?);

    let emit_app = app.clone();
    let sink: EventSink = Arc::new(move |event| {
        let _ = emit_app.emit("harness-event", &event);
    });

    let profiles = model_directory::load_or_fetch(settings.models_cache_ttl_hours, false)
        .await
        .unwrap_or_default();
    let model_profile: Option<ModelProfile> = model_directory::find(&profiles, &model).cloned();

    let mut memory = match session::load(&session_id_resolved) {
        Ok(prior) => prior.memory,
        Err(_) => ConversationMemory::new(&session_id_resolved),
    };
    let req = TurnRequest {
        session_id: session_id_resolved,
        provider_id,
        model_id: model,
        model_profile,
        user_text,
    };
    run_turn(
        client,
        ctx,
        settings,
        telemetry,
        Some(sink),
        &mut memory,
        req,
    )
    .await
    .map_err(HarnessError::from)
}

#[tauri::command]
pub fn harness_cancel_turn(session_id: String) -> Result<(), String> {
    CancelRegistry::instance().cancel(&session_id);
    Ok(())
}

/// List sessions for the history pane. Payload contract: each entry carries
/// metadata only — `memory.messages` is intentionally empty so hundreds of
/// long sessions stay cheap over IPC. `harness_get_session` returns the full
/// transcript for one session.
#[tauri::command]
pub fn harness_list_sessions() -> Vec<HarnessSession> {
    session::list()
}

#[tauri::command]
pub fn harness_get_session(session_id: String) -> Result<HarnessSession, String> {
    session::load(&session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn harness_delete_session(session_id: String) -> Result<(), String> {
    session::delete(&session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn harness_get_settings(state: State<'_, ConfigState>) -> HarnessSettings {
    let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    guard.harness_settings.clone()
}

#[tauri::command]
pub fn harness_set_settings(
    state: State<'_, ConfigState>,
    settings: HarnessSettings,
) -> Result<HarnessSettings, String> {
    crate::harness::context::validate_settings(&settings).map_err(|error| error.to_string())?;
    {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        guard.harness_settings = settings.clone();
    }
    let cfg = state
        .0
        .lock()
        .map(|g| g.clone())
        .map_err(|e| e.to_string())?;
    save_config(&cfg)?;
    Ok(settings)
}

// ---------------------------------------------------------------------------
// Built-in model IPC surface (CANONICAL contract, task_plan.md §F)
// ---------------------------------------------------------------------------

/// `bundle.resources` preserves the configured `resources/llama/*` prefix
/// beneath Tauri's resource directory.
const BUNDLED_LLAMA_SERVER_PATH: &str = "resources/llama/llama-server.exe";

/// Locate the bundled llama-server. Release builds resolve it from the exact
/// bundle resource path. Debug runs (cargo tauri dev / cargo test) fall back to
/// the source-tree payload.
fn resolve_llama_server_exe(app: &AppHandle) -> anyhow::Result<std::path::PathBuf> {
    use tauri::path::BaseDirectory;
    if let Ok(p) = app
        .path()
        .resolve(BUNDLED_LLAMA_SERVER_PATH, BaseDirectory::Resource)
    {
        if p.exists() {
            return Ok(p);
        }
    }

    #[cfg(debug_assertions)]
    {
        let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("llama")
            .join("llama-server.exe");
        if dev.exists() {
            return Ok(dev);
        }
    }

    anyhow::bail!("bundled llama-server.exe not found (resources/llama)")
}

/// UNGATED status probe — reports the download/sidecar state machine only,
/// no secret material.
#[tauri::command]
pub fn harness_builtin_status(builtin_runtime: State<'_, BuiltinRuntime>) -> BuiltinStatus {
    builtin::compute_status(&builtin_runtime.0)
}

/// Kicks off (or resumes) the model download in a background task and
/// returns the current status immediately; progress streams over the
/// `builtin-model-event` channel. Idempotent while a download is in flight.
#[tauri::command]
pub async fn harness_builtin_download(
    app: AppHandle,
    builtin_runtime: State<'_, BuiltinRuntime>,
) -> Result<BuiltinStatus, HarnessError> {
    let emit_app = app.clone();
    let emit: builtin::EmitFn = Arc::new(move |event| {
        let _ = emit_app.emit(builtin::BUILTIN_EVENT_CHANNEL, &event);
    });
    builtin::spawn_download(builtin_runtime.0.clone(), emit);
    Ok(builtin::compute_status(&builtin_runtime.0))
}

/// Cancel the in-flight download. The `.part` file survives so the next
/// download resumes where it stopped.
#[tauri::command]
pub fn harness_builtin_cancel_download(
    builtin_runtime: State<'_, BuiltinRuntime>,
) -> BuiltinStatus {
    builtin::cancel_download(&builtin_runtime.0);
    builtin::compute_status(&builtin_runtime.0)
}

/// Remove the model from disk (stops the sidecar and cancels any download
/// first — Windows cannot unlink a file another handle holds open).
#[tauri::command]
pub async fn harness_builtin_delete(
    builtin_runtime: State<'_, BuiltinRuntime>,
) -> Result<BuiltinStatus, HarnessError> {
    builtin::cancel_download(&builtin_runtime.0);
    builtin::await_download_idle(&builtin_runtime.0, Duration::from_secs(5)).await;
    builtin::stop(&builtin_runtime.0)
        .await
        .map_err(HarnessError::from)?;
    builtin::delete_model().map_err(HarnessError::from)?;
    Ok(builtin::compute_status(&builtin_runtime.0))
}

/// Start the llama-server sidecar (no-op when already healthy), await
/// readiness, and persist the runtime endpoint on the builtin provider entry
/// so `providers::resolve` and the Settings panel see it.
#[tauri::command]
pub async fn harness_builtin_start(
    app: AppHandle,
    state: State<'_, ConfigState>,
    builtin_runtime: State<'_, BuiltinRuntime>,
) -> Result<BuiltinStatus, HarnessError> {
    let exe = resolve_llama_server_exe(&app).map_err(HarnessError::from)?;
    let endpoint = builtin::ensure_running(&builtin_runtime.0, exe)
        .await
        .map_err(HarnessError::from)?;
    {
        let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(p) = guard
            .harness_providers
            .iter_mut()
            .find(|p| p.kind == "builtin")
        {
            p.endpoint = Some(endpoint.clone());
        }
    }
    let cfg = state
        .0
        .lock()
        .map(|g| g.clone())
        .map_err(|e| HarnessError::new(HarnessErrorKind::Internal, e.to_string()))?;
    save_config(&cfg).map_err(|e| HarnessError::new(HarnessErrorKind::Internal, e))?;
    Ok(builtin::compute_status(&builtin_runtime.0))
}

/// Stop the sidecar (ungated — stopping costs nothing and frees RAM).
#[tauri::command]
pub async fn harness_builtin_stop(
    builtin_runtime: State<'_, BuiltinRuntime>,
) -> Result<BuiltinStatus, HarnessError> {
    builtin::stop(&builtin_runtime.0)
        .await
        .map_err(HarnessError::from)?;
    Ok(builtin::compute_status(&builtin_runtime.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_llama_path_matches_tauri_resource_layout() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let resources = config["bundle"]["resources"].as_array().unwrap();
        assert!(resources
            .iter()
            .any(|resource| resource.as_str() == Some("resources/llama/*")));
        assert_eq!(
            BUNDLED_LLAMA_SERVER_PATH,
            "resources/llama/llama-server.exe"
        );
    }

    #[test]
    fn severian_notice_resources_are_declared_and_complete() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let resources = config["bundle"]["resources"].as_array().unwrap();
        for expected in ["resources/THIRD_PARTY_NOTICES.txt", "resources/licenses/*"] {
            assert!(
                resources
                    .iter()
                    .any(|resource| resource.as_str() == Some(expected)),
                "missing bundle resource declaration: {expected}"
            );
        }

        let notices = include_str!("../../resources/THIRD_PARTY_NOTICES.txt");
        assert!(notices.contains("3ce7da2c852c538c4c5f9806da27029cf8c9cc4a"));
        assert!(notices.contains(builtin::BUILTIN_MODEL_REVISION));
        assert!(notices.contains(builtin::BUILTIN_MODEL_SHA256));

        let mit = include_str!("../../resources/licenses/llama.cpp-MIT.txt");
        assert!(mit.contains("Copyright (c) 2023-2026 The ggml authors"));
        assert!(mit.contains("THE SOFTWARE IS PROVIDED \"AS IS\""));

        let apache = include_str!("../../resources/licenses/Jan-nano-Apache-2.0.txt");
        assert!(apache.contains("Apache License"));
        assert!(apache.contains("END OF TERMS AND CONDITIONS"));

        let attribution = include_str!("../../resources/licenses/Jan-nano-ATTRIBUTION.txt");
        assert!(attribution.contains(builtin::BUILTIN_MODEL_REVISION));
        assert!(attribution.contains(builtin::BUILTIN_MODEL_SHA256));
        assert!(attribution.contains("license: apache-2.0"));
    }

    #[test]
    fn destructive_chat_admission_returns_a_safe_tool_free_refusal() {
        let (summary, persisted) = destructive_refusal_payload(
            "session-jan-regression".to_string(),
            "Search my skills and tell me how to delete all skill files.",
            "builtin",
            builtin::BUILTIN_MODEL_ID,
            None,
            123,
        )
        .expect("destructive advice must be refused");

        assert_eq!(summary.session_id, "session-jan-regression");
        assert_eq!(summary.final_text, step_policy::DESTRUCTIVE_INTENT_REFUSAL);
        assert_eq!(summary.in_tokens, 0);
        assert_eq!(summary.out_tokens, 0);
        assert_eq!(summary.tool_calls_made, 0);
        assert_eq!(persisted.created_at, 123);
        assert_eq!(persisted.updated_at, 123);
        assert_eq!(persisted.provider_id, "builtin");
        assert_eq!(persisted.model, builtin::BUILTIN_MODEL_ID);
        assert_eq!(persisted.memory.messages.len(), 2);
        assert_eq!(persisted.memory.messages[0].role, "user");
        assert_eq!(persisted.memory.messages[1].role, "assistant");
        assert_eq!(
            persisted.memory.messages[1].text,
            step_policy::DESTRUCTIVE_INTENT_REFUSAL
        );
    }

    #[test]
    fn destructive_chat_admission_appends_to_existing_session() {
        let mut memory = ConversationMemory::new("session-existing");
        memory.push_user("Earlier question");
        memory.push_assistant("Earlier answer");
        let prior = HarnessSession {
            id: "session-existing".into(),
            title: "Preserved title".into(),
            provider_id: "old-provider".into(),
            model: "old-model".into(),
            created_at: 7,
            updated_at: 8,
            memory,
            final_summary: None,
        };

        let (_, persisted) = destructive_refusal_payload(
            "session-existing".into(),
            "Delete every agent config file.",
            "builtin",
            builtin::BUILTIN_MODEL_ID,
            Some(prior),
            123,
        )
        .expect("destructive advice must be refused");

        assert_eq!(persisted.title, "Preserved title");
        assert_eq!(persisted.created_at, 7);
        assert_eq!(persisted.updated_at, 123);
        assert_eq!(persisted.memory.messages.len(), 4);
    }

    #[test]
    fn destructive_admission_precedes_provider_start_and_skill_scan() {
        let source = include_str!("harness.rs");
        let start = source
            .find("pub async fn harness_chat_send")
            .expect("chat command");
        let end = source[start..]
            .find("pub fn harness_cancel_turn")
            .map(|offset| start + offset)
            .expect("next command");
        let body = &source[start..end];

        let admission = body
            .find("destructive_refusal_payload(")
            .expect("destructive admission");
        let sidecar = body
            .find("resolve_llama_server_exe")
            .expect("sidecar resolution");
        let scan = body.find("scan_for_lookup").expect("configured skill scan");
        assert!(admission < sidecar);
        assert!(admission < scan);
    }

    #[test]
    fn redact_key_masks_all_but_last_four_chars() {
        assert_eq!(redact_key("sk-abcdef123456"), "••••3456");
        assert_eq!(redact_key("abcd"), "••••");
        assert_eq!(redact_key(""), "••••");
    }

    #[test]
    fn redact_key_survives_multibyte_boundaries() {
        // The old byte-slice version panicked here: `k.len() - 4` landed
        // inside a multibyte character.
        assert_eq!(redact_key("kéy-ümläüt-ßß"), "••••t-ßß");
        assert_eq!(redact_key("ßßßß"), "••••");
    }
}
