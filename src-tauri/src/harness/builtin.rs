// @agent-context: Built-in local model — download + sidecar lifecycle.
//
// Owns everything about the bundled zero-config provider:
//   - Model manifest constants (Jan-nano-4B Q4_K_M, pinned URL + sha256 + size).
//   - Download-on-first-run into %LOCALAPPDATA%/skill-deck/models/ with
//     streaming SHA256, HTTP Range resume against a `.part` file, a host
//     allowlist enforced on every redirect hop (SSRF posture from bb4791b),
//     and one automatic re-download on hash mismatch.
//   - Sidecar spawn of the bundled llama.cpp `llama-server.exe`
//     (resources/llama/, CPU-only b10092) via tokio::process on a freshly
//     allocated free port, `--jinja -c 8192 --no-webui`, /health readiness
//     poll, kill on app exit (lib.rs RunEvent hook) + kill_on_drop.
//   - The `builtin-model-event` progress channel payload (its own channel —
//     the chat-scoped HarnessEvent stream is NOT extended for downloads).
//
// No Tauri commands live here: `commands/harness.rs` wraps this module with
// the `harness_builtin_*` IPC surface.
// Stale-orphan posture: every start allocates a fresh free port from the OS,
// so an orphaned llama-server from a hard crash can never collide with us.

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Context window we launch llama-server with (`-c`). SINGLE source of truth
/// for the built-in model's context window — the synthetic model profile in
/// agent.rs and `list_models` both read this constant.
pub const BUILTIN_CTX: u32 = 8192;

/// Stable id for the built-in model (also the GGUF file stem on disk and the
/// `model` string sent to llama-server, which serves whatever is loaded).
pub const BUILTIN_MODEL_ID: &str = "jan-nano-4b-q4km";

/// Human label shown in the model picker + status card. The GGUF is managed by
/// Skill Deck, but downloaded on demand rather than embedded in the installer.
pub const BUILTIN_MODEL_NAME: &str = "Jan-nano 4B";

/// Provider id of the auto-managed builtin entry in `harness_providers`.
pub const BUILTIN_PROVIDER_ID: &str = "builtin";

/// Immutable Hugging Face revision containing the audited GGUF.
pub const BUILTIN_MODEL_REVISION: &str = "44c1024058604e95b4845fecde698f5f1a0c99e7";

/// Pinned download source (HF resolver, ungated, Apache-2.0). The resolver URL,
/// byte size, and digest identify one immutable artifact. R2 self-hosting is a
/// post-launch optimization.
pub const BUILTIN_MODEL_URL: &str =
    "https://huggingface.co/Menlo/Jan-nano-gguf/resolve/44c1024058604e95b4845fecde698f5f1a0c99e7/jan-nano-4b-Q4_K_M.gguf";

/// Exact byte size of the pinned GGUF (used for Range resume + progress).
pub const BUILTIN_MODEL_SIZE: u64 = 2_497_280_448;

/// Pinned SHA256 of the GGUF. A file only becomes `ready` after matching this.
pub const BUILTIN_MODEL_SHA256: &str =
    "c345a1c0eda29e56c73c4c4c00035f28470e649cf4f551dea80a7baffe79a798";

/// Tauri event channel for download/lifecycle progress. Separate from
/// "harness-event" on purpose: downloads are app-scoped, not chat-turn-scoped.
pub const BUILTIN_EVENT_CHANNEL: &str = "builtin-model-event";

/// Extra headroom demanded beyond the remaining download bytes so we never
/// fill the disk to the brim (temp file + rename need slack).
const FREE_DISK_MARGIN_BYTES: u64 = 500 * 1024 * 1024;

/// Fail-closed sidecar admission threshold.
///
/// The Q4_K_M weights are 2,497,280,448 bytes (2.33 GiB). llama.cpp must also
/// allocate the 8,192-token KV/context cache, compute buffers, and process
/// overhead. Reserving 2 GiB beyond the weights gives 4.33 GiB; rounding up to
/// 5 GiB avoids starting a process that would immediately push Windows into
/// sustained paging. This is *available* RAM, not installed RAM.
pub const MIN_AVAILABLE_RAM_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// How long the /health poll waits for llama-server to load the model.
const HEALTH_POLL_CEILING: Duration = Duration::from_secs(60);

/// Emit progress every this many bytes (keeps the event channel light —
/// a 2.5 GB stream at 64 KiB chunks would otherwise emit ~38k events).
const EMIT_EVERY_BYTES: u64 = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Wire types (camelCase per house IPC rules)
// ---------------------------------------------------------------------------

/// Status state machine per the CANONICAL IPC contract (task_plan.md §F).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BuiltinStateKind {
    NotDownloaded,
    Downloading,
    Verifying,
    Ready,
    Running,
}

/// `harness_builtin_status()` payload. UNGATED — carries no secrets.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinStatus {
    pub state: BuiltinStateKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_done: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_total: Option<u64>,
    pub model_name: String,
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

/// Payload for the `builtin-model-event` channel.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinModelEvent {
    /// "downloading" | "verifying" | "ready" | "error"
    pub phase: String,
    pub pct: f64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Sink the commands layer hands us (wraps `app.emit(BUILTIN_EVENT_CHANNEL, …)`).
pub type EmitFn = Arc<dyn Fn(BuiltinModelEvent) + Send + Sync>;

// ---------------------------------------------------------------------------
// Managed state
// ---------------------------------------------------------------------------

/// Tauri-managed wrapper (`.manage(BuiltinRuntime::new())` in lib.rs).
pub struct BuiltinRuntime(pub Arc<BuiltinShared>);

impl BuiltinRuntime {
    pub fn new() -> Self {
        Self(Arc::new(BuiltinShared::new()))
    }
}

impl Default for BuiltinRuntime {
    fn default() -> Self {
        Self::new()
    }
}

pub struct BuiltinShared {
    /// Download progress — std Mutex, never held across an await.
    download: std::sync::Mutex<DownloadState>,
    /// The sidecar process. tokio Mutex so start/stop serialize across awaits.
    sidecar: tokio::sync::Mutex<Option<SidecarProc>>,
    /// Cheap sync-readable mirror of the sidecar state for `compute_status`.
    snapshot: std::sync::Mutex<SidecarSnapshot>,
}

#[derive(Default)]
struct DownloadState {
    in_flight: bool,
    verifying: bool,
    bytes_done: u64,
    bytes_total: u64,
    cancel: Option<Arc<AtomicBool>>,
    last_error: Option<String>,
}

struct SidecarProc {
    child: tokio::process::Child,
    port: u16,
}

#[derive(Debug, Default, Clone)]
struct SidecarSnapshot {
    running: bool,
    port: Option<u16>,
}

impl BuiltinShared {
    fn new() -> Self {
        Self {
            download: std::sync::Mutex::new(DownloadState::default()),
            sidecar: tokio::sync::Mutex::new(None),
            snapshot: std::sync::Mutex::new(SidecarSnapshot::default()),
        }
    }

    fn set_snapshot(&self, running: bool, port: Option<u16>) {
        let mut snap = self.snapshot.lock().unwrap_or_else(|p| p.into_inner());
        snap.running = running;
        snap.port = port;
    }

    /// Best-effort synchronous kill for the app-exit path (RunEvent hook in
    /// lib.rs). `try_lock` is fine there: at exit nothing races start/stop,
    /// and `kill_on_drop(true)` backstops the contended case anyway.
    pub fn kill_sidecar_now(&self) {
        if let Ok(mut guard) = self.sidecar.try_lock() {
            if let Some(mut proc) = guard.take() {
                let _ = proc.child.start_kill();
            }
        }
        self.set_snapshot(false, None);
    }
}

// ---------------------------------------------------------------------------
// Paths + config injection
// ---------------------------------------------------------------------------

/// `%LOCALAPPDATA%/skill-deck/models/` — deliberately data_local_dir, NOT
/// config_dir (Roaming): a 2.5 GB blob must never ride a roaming profile.
pub fn models_dir() -> Result<PathBuf> {
    let mut dir = dirs::data_local_dir().context("no local data dir on this platform")?;
    dir.push("skill-deck");
    dir.push("models");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create models dir {}", dir.display()))?;
    Ok(dir)
}

pub fn model_path() -> Result<PathBuf> {
    Ok(models_dir()?.join(format!("{BUILTIN_MODEL_ID}.gguf")))
}

fn part_path() -> Result<PathBuf> {
    Ok(models_dir()?.join(format!("{BUILTIN_MODEL_ID}.gguf.part")))
}

/// True when the finished, hash-verified GGUF is on disk. Size check only —
/// the `.part` file is renamed to the final name ONLY after its digest matched,
/// so presence-at-size implies a verified file.
fn model_on_disk() -> bool {
    model_path()
        .ok()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len() == BUILTIN_MODEL_SIZE)
        .unwrap_or(false)
}

/// Inject the auto-managed builtin provider entry as the FIRST provider so it
/// renders first in Settings. Idempotent. Returns true when the config changed.
pub fn ensure_builtin_provider(cfg: &mut crate::models::AppConfig) -> bool {
    use crate::harness::config::ProviderConfig;
    if cfg.harness_providers.iter().any(|p| p.kind == "builtin") {
        return false;
    }
    cfg.harness_providers.insert(
        0,
        ProviderConfig {
            id: BUILTIN_PROVIDER_ID.to_string(),
            kind: "builtin".to_string(),
            label: "Jan-nano 4B (managed local)".to_string(),
            endpoint: None, // written at sidecar start
            api_key: None,
            deployment: None,
            api_version: None,
            default_model: Some(BUILTIN_MODEL_ID.to_string()),
            notes: Some(
                "Managed Jan-nano 4B via llama.cpp; verified online download, local thereafter"
                    .into(),
            ),
        },
    );
    true
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Pure state derivation — unit-tested without touching disk or process state.
/// Precedence: an in-flight download (verifying > downloading) beats the
/// sidecar, which beats the on-disk file.
fn derive_state(
    download_in_flight: bool,
    verifying: bool,
    sidecar_running: bool,
    on_disk: bool,
) -> BuiltinStateKind {
    if download_in_flight {
        if verifying {
            BuiltinStateKind::Verifying
        } else {
            BuiltinStateKind::Downloading
        }
    } else if sidecar_running {
        BuiltinStateKind::Running
    } else if on_disk {
        BuiltinStateKind::Ready
    } else {
        BuiltinStateKind::NotDownloaded
    }
}

pub fn compute_status(shared: &BuiltinShared) -> BuiltinStatus {
    let (in_flight, verifying, bytes_done, bytes_total) = {
        let d = shared.download.lock().unwrap_or_else(|p| p.into_inner());
        (d.in_flight, d.verifying, d.bytes_done, d.bytes_total)
    };
    let snap = {
        let s = shared.snapshot.lock().unwrap_or_else(|p| p.into_inner());
        s.clone()
    };
    let on_disk = model_on_disk();
    let state = derive_state(in_flight, verifying, snap.running, on_disk);
    let (progress_pct, done, total) = if in_flight && bytes_total > 0 {
        (
            Some(((bytes_done as f64 / bytes_total as f64) * 1000.0).round() / 10.0),
            Some(bytes_done),
            Some(bytes_total),
        )
    } else {
        (None, None, None)
    };
    BuiltinStatus {
        state,
        progress_pct,
        bytes_done: done,
        bytes_total: total,
        model_name: BUILTIN_MODEL_NAME.to_string(),
        size_bytes: Some(BUILTIN_MODEL_SIZE),
        path: if on_disk {
            model_path().ok().map(|p| p.display().to_string())
        } else {
            None
        },
        port: if snap.running { snap.port } else { None },
    }
}

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

/// Hosts the downloader may talk to, INCLUDING every redirect hop (the HF
/// resolver 302s to cdn-lfs.huggingface.co / *.hf.co; GitHub release assets
/// 302 to *.githubusercontent.com). Mirrors the bb4791b SSRF posture: never
/// fetch an arbitrary host.
fn host_allowed(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    const EXACT: [&str; 3] = ["huggingface.co", "github.com", "hf.co"];
    const SUFFIX: [&str; 3] = [".huggingface.co", ".hf.co", ".githubusercontent.com"];
    EXACT.contains(&h.as_str()) || SUFFIX.iter().any(|s| h.ends_with(s))
}

fn download_client() -> Result<reqwest::Client> {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > 8 {
            return attempt.error("too many redirects");
        }
        let ok = attempt.url().host_str().map(host_allowed).unwrap_or(false);
        if ok {
            attempt.follow()
        } else {
            attempt.error("redirect target host not in download allowlist")
        }
    });
    reqwest::Client::builder()
        .redirect(policy)
        .connect_timeout(Duration::from_secs(30))
        .build()
        .context("build download http client")
}

/// Verify a completed `.part` against the pinned digest, then atomically
/// rename it into place. On mismatch the part file is DELETED (poisoned data
/// never survives) and `Ok(false)` is returned so the caller can retry once.
fn finalize_verified(part: &Path, dest: &Path, digest_hex: &str, expected: &str) -> Result<bool> {
    if !digest_hex.eq_ignore_ascii_case(expected) {
        let _ = std::fs::remove_file(part);
        return Ok(false);
    }
    std::fs::rename(part, dest)
        .with_context(|| format!("rename {} → {}", part.display(), dest.display()))?;
    Ok(true)
}

/// Streaming SHA256 of an on-disk file (used to re-hash an existing `.part`
/// before resuming, and by tests).
fn sha256_hex_of_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf).context("read for hashing")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // Infallible: writing to a String never errors.
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Best-effort free-space lookup for the disk holding `path` (longest
/// matching mount point wins). `None` when the disk cannot be identified —
/// the download proceeds rather than false-blocking.
fn free_disk_space_for(path: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut best: Option<(usize, u64)> = None;
    for d in disks.list() {
        let mp = d.mount_point();
        if path.starts_with(mp) {
            let len = mp.as_os_str().len();
            if best.map(|(l, _)| len > l).unwrap_or(true) {
                best = Some((len, d.available_space()));
            }
        }
    }
    best.map(|(_, space)| space)
}

/// Refresh and return currently available physical memory.
///
/// A zero value is treated as unavailable telemetry so callers fail closed.
fn available_ram_bytes() -> Option<u64> {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let available = system.available_memory();
    (available > 0).then_some(available)
}

/// Pure, independently testable RAM admission seam.
fn ensure_available_ram(available: Option<u64>) -> Result<()> {
    let available = available
        .context("could not determine available RAM; refusing to start the local model safely")?;
    anyhow::ensure!(
        available >= MIN_AVAILABLE_RAM_BYTES,
        "not enough available RAM for Jan-nano 4B: need at least {:.1} GB available, have {:.1} GB",
        MIN_AVAILABLE_RAM_BYTES as f64 / 1_000_000_000.0,
        available as f64 / 1_000_000_000.0
    );
    Ok(())
}

/// Mark a download as started. Returns the fresh cancel flag, or None when a
/// download is already in flight (idempotence guard — never start a second).
fn begin_download(shared: &BuiltinShared) -> Option<Arc<AtomicBool>> {
    let mut d = shared.download.lock().unwrap_or_else(|p| p.into_inner());
    if d.in_flight {
        return None;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    d.in_flight = true;
    d.verifying = false;
    d.bytes_done = 0;
    d.bytes_total = BUILTIN_MODEL_SIZE;
    d.cancel = Some(cancel.clone());
    d.last_error = None;
    Some(cancel)
}

fn end_download(shared: &BuiltinShared, error: Option<String>) {
    let mut d = shared.download.lock().unwrap_or_else(|p| p.into_inner());
    d.in_flight = false;
    d.verifying = false;
    d.cancel = None;
    d.last_error = error;
}

/// Request cancellation of the in-flight download (checked per chunk). The
/// `.part` file survives so the next download resumes.
pub fn cancel_download(shared: &BuiltinShared) {
    let d = shared.download.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(cancel) = d.cancel.as_ref() {
        cancel.store(true, Ordering::SeqCst);
    }
}

/// Poll until no download is in flight (used by delete so an in-flight writer
/// cannot hold the `.part` open while we unlink it on Windows).
pub async fn await_download_idle(shared: &BuiltinShared, ceiling: Duration) {
    let started = std::time::Instant::now();
    loop {
        let in_flight = {
            let d = shared.download.lock().unwrap_or_else(|p| p.into_inner());
            d.in_flight
        };
        if !in_flight || started.elapsed() > ceiling {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Fire-and-forget download entry point. Progress flows over the
/// `builtin-model-event` channel; terminal phase is "ready" or "error".
/// Idempotent: a second call while one is in flight is a no-op.
pub fn spawn_download(shared: Arc<BuiltinShared>, emit: EmitFn) {
    let Some(cancel) = begin_download(&shared) else {
        return; // already downloading
    };
    tauri::async_runtime::spawn(async move {
        let outcome = run_download(&shared, &emit, &cancel).await;
        match outcome {
            Ok(()) => {
                end_download(&shared, None);
                emit(BuiltinModelEvent {
                    phase: "ready".into(),
                    pct: 100.0,
                    bytes_done: BUILTIN_MODEL_SIZE,
                    bytes_total: BUILTIN_MODEL_SIZE,
                    message: None,
                });
            }
            Err(e) => {
                let msg = format!("{e:#}");
                log::warn!("builtin model download failed: {msg}");
                end_download(&shared, Some(msg.clone()));
                emit(BuiltinModelEvent {
                    phase: "error".into(),
                    pct: 0.0,
                    bytes_done: 0,
                    bytes_total: BUILTIN_MODEL_SIZE,
                    message: Some(msg),
                });
            }
        }
    });
}

async fn run_download(
    shared: &BuiltinShared,
    emit: &EmitFn,
    cancel: &Arc<AtomicBool>,
) -> Result<()> {
    let dest = model_path()?;
    if model_on_disk() {
        return Ok(()); // already downloaded + verified
    }

    // On mismatch delete + redownload once, then error (spec §E.3).
    for attempt in 0..2 {
        if cancel.load(Ordering::SeqCst) {
            anyhow::bail!("download cancelled");
        }
        match download_once(shared, emit, cancel, &dest).await {
            Ok(true) => return Ok(()),
            Ok(false) if attempt == 0 => {
                log::warn!("builtin model sha256 mismatch — re-downloading once");
                continue;
            }
            Ok(false) => anyhow::bail!(
                "downloaded model failed SHA256 verification twice — aborting (expected {BUILTIN_MODEL_SHA256})"
            ),
            Err(e) => return Err(e),
        }
    }
    unreachable!("download attempt loop covers all exits")
}

/// One download attempt. `Ok(true)` = verified + renamed into place;
/// `Ok(false)` = hash mismatch (part deleted, caller may retry).
async fn download_once(
    shared: &BuiltinShared,
    emit: &EmitFn,
    cancel: &Arc<AtomicBool>,
    dest: &Path,
) -> Result<bool> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let part = part_path()?;

    // Host allowlist on the ORIGIN too, not just redirects.
    let origin_host = BUILTIN_MODEL_URL
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split(['/', ':']).next())
        .unwrap_or("");
    anyhow::ensure!(
        host_allowed(origin_host),
        "download origin host not in allowlist: {origin_host}"
    );

    // Resume support: hash whatever partial bytes we already hold.
    let mut hasher = Sha256::new();
    let mut offset: u64 = 0;
    if let Ok(meta) = std::fs::metadata(&part) {
        let existing = meta.len();
        if existing > 0 && existing <= BUILTIN_MODEL_SIZE {
            hash_file_into(&part, &mut hasher).await?;
            offset = existing;
        } else {
            let _ = std::fs::remove_file(&part);
        }
    }

    // Preflight free-disk check (best effort).
    if let Some(free) = free_disk_space_for(&part) {
        let needed = BUILTIN_MODEL_SIZE
            .saturating_sub(offset)
            .saturating_add(FREE_DISK_MARGIN_BYTES);
        anyhow::ensure!(
            free >= needed,
            "not enough free disk space for the Jan-nano download: need ~{} MB, have {} MB",
            needed / (1024 * 1024),
            free / (1024 * 1024)
        );
    }

    let client = download_client()?;
    let mut req = client.get(BUILTIN_MODEL_URL);
    if offset > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let res = req.send().await.context("model download request")?;
    let status = res.status();
    anyhow::ensure!(
        status.is_success(),
        "model download HTTP {status} from {BUILTIN_MODEL_URL}"
    );
    // If we asked for a Range but the server replied 200 (full body), the
    // partial bytes are useless — start over.
    if offset > 0 && status != reqwest::StatusCode::PARTIAL_CONTENT {
        let _ = std::fs::remove_file(&part);
        hasher = Sha256::new();
        offset = 0;
    }

    // Windows: an append-mode handle only holds FILE_APPEND_DATA, so
    // `set_len(0)` on it fails with Access Denied (os error 5). Pick the open
    // mode by intent instead: resume appends, fresh start truncates at open.
    let mut file = if offset > 0 {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&part)
            .await
            .with_context(|| format!("open part file for resume {}", part.display()))?
    } else {
        tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&part)
            .await
            .with_context(|| format!("create part file {}", part.display()))?
    };

    let mut bytes_done = offset;
    let mut last_emit = bytes_done;
    set_progress(shared, bytes_done, false);
    emit(progress_event("downloading", bytes_done, None));

    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            file.flush().await.ok();
            anyhow::bail!("download cancelled"); // .part survives for resume
        }
        let chunk = chunk.context("model download stream")?;
        hasher.update(&chunk);
        file.write_all(&chunk).await.context("write model chunk")?;
        bytes_done += chunk.len() as u64;
        set_progress(shared, bytes_done, false);
        if bytes_done - last_emit >= EMIT_EVERY_BYTES {
            last_emit = bytes_done;
            emit(progress_event("downloading", bytes_done, None));
        }
    }
    file.flush().await.context("flush model file")?;
    drop(file);

    anyhow::ensure!(
        bytes_done == BUILTIN_MODEL_SIZE,
        "model download ended early: got {bytes_done} of {BUILTIN_MODEL_SIZE} bytes"
    );

    set_progress(shared, bytes_done, true);
    emit(progress_event("verifying", bytes_done, None));
    let digest = hex_lower(&hasher.finalize());
    finalize_verified(&part, dest, &digest, BUILTIN_MODEL_SHA256)
}

async fn hash_file_into(path: &Path, hasher: &mut Sha256) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("open {} for resume hashing", path.display()))?;
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf).await.context("read part for hashing")?;
        if n == 0 {
            return Ok(());
        }
        hasher.update(&buf[..n]);
    }
}

fn set_progress(shared: &BuiltinShared, bytes_done: u64, verifying: bool) {
    let mut d = shared.download.lock().unwrap_or_else(|p| p.into_inner());
    d.bytes_done = bytes_done;
    d.bytes_total = BUILTIN_MODEL_SIZE;
    d.verifying = verifying;
}

fn progress_event(phase: &str, bytes_done: u64, message: Option<String>) -> BuiltinModelEvent {
    BuiltinModelEvent {
        phase: phase.to_string(),
        pct: ((bytes_done as f64 / BUILTIN_MODEL_SIZE as f64) * 1000.0).round() / 10.0,
        bytes_done,
        bytes_total: BUILTIN_MODEL_SIZE,
        message,
    }
}

/// Remove the model (and any partial download). Caller stops the sidecar and
/// cancels the download first.
pub fn delete_model() -> Result<()> {
    let model = model_path()?;
    if model.exists() {
        std::fs::remove_file(&model).with_context(|| format!("delete {}", model.display()))?;
    }
    let part = part_path()?;
    if part.exists() {
        std::fs::remove_file(&part).with_context(|| format!("delete {}", part.display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Sidecar lifecycle
// ---------------------------------------------------------------------------

/// Bind 127.0.0.1:0, read the OS-assigned port, drop the listener. The small
/// TOCTOU window is acceptable on localhost; a fresh port per start is also
/// what makes stale orphans harmless.
fn alloc_free_port() -> Result<u16> {
    let listener =
        std::net::TcpListener::bind(("127.0.0.1", 0)).context("bind 127.0.0.1:0 for free port")?;
    let port = listener.local_addr().context("read local addr")?.port();
    drop(listener);
    Ok(port)
}

/// Ensure llama-server is up and healthy; returns the OpenAI-compatible
/// endpoint (`http://127.0.0.1:PORT/v1`). Reuses a live sidecar; restarts a
/// dead one. Serialized by the tokio Mutex so concurrent chat sends cannot
/// double-spawn.
pub async fn ensure_running(shared: &Arc<BuiltinShared>, exe: PathBuf) -> Result<String> {
    let mut guard = shared.sidecar.lock().await;

    // Reuse a still-alive child.
    if let Some(proc) = guard.as_mut() {
        match proc.child.try_wait() {
            Ok(None) => {
                let port = proc.port;
                drop(guard);
                return Ok(endpoint_for(port));
            }
            _ => {
                // exited or unknowable — clear and respawn below
                *guard = None;
                shared.set_snapshot(false, None);
            }
        }
    }

    anyhow::ensure!(
        model_on_disk(),
        "Jan-nano 4B is not downloaded yet — download and verify it before first use"
    );
    anyhow::ensure!(
        exe.exists(),
        "bundled llama-server not found at {}",
        exe.display()
    );
    // This check is deliberately immediately before process preparation. A
    // missing/failed memory reading vetoes startup instead of guessing.
    ensure_available_ram(available_ram_bytes())?;
    let model = model_path()?;
    let port = alloc_free_port()?;

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args([
        "-m",
        &model.display().to_string(),
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "--jinja",
        "-c",
        &BUILTIN_CTX.to_string(),
        "--no-webui",
    ])
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .kill_on_drop(true);
    // llama-server needs its co-located ggml*.dll — run from its own dir.
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawn llama-server from {}", exe.display()))?;

    // Readiness: poll /health until 200 (503 = still loading the GGUF).
    match wait_healthy(port, &mut child).await {
        Ok(()) => {
            *guard = Some(SidecarProc { child, port });
            shared.set_snapshot(true, Some(port));
            Ok(endpoint_for(port))
        }
        Err(e) => {
            let _ = child.start_kill();
            shared.set_snapshot(false, None);
            Err(e)
        }
    }
}

fn endpoint_for(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1")
}

async fn wait_healthy(port: u16, child: &mut tokio::process::Child) -> Result<()> {
    let url = format!("http://127.0.0.1:{port}/health");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("health client")?;
    let started = std::time::Instant::now();
    let mut delay = Duration::from_millis(250);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            anyhow::bail!("llama-server exited during startup (status {status})");
        }
        if let Ok(res) = client.get(&url).send().await {
            if res.status().is_success() {
                return Ok(());
            }
        }
        if started.elapsed() > HEALTH_POLL_CEILING {
            anyhow::bail!(
                "llama-server did not become healthy within {}s",
                HEALTH_POLL_CEILING.as_secs()
            );
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(2));
    }
}

/// Stop the sidecar if running. Graceful kill + reap; snapshot cleared.
pub async fn stop(shared: &Arc<BuiltinShared>) -> Result<()> {
    let mut guard = shared.sidecar.lock().await;
    if let Some(mut proc) = guard.take() {
        proc.child.kill().await.context("kill llama-server")?;
    }
    shared.set_snapshot(false, None);
    Ok(())
}

// ---------------------------------------------------------------------------
// End-to-end proof tests (D3 §G). #[ignore] — they need network, 2.5 GB of
// disk, and spawn a real llama-server. Run explicitly, in order:
//   cargo test --lib -- --ignored --exact --nocapture \
//       harness::builtin::e2e_tests::builtin_e2e_download
//   cargo test --lib -- --ignored --exact --nocapture \
//       harness::builtin::e2e_tests::builtin_e2e
// ---------------------------------------------------------------------------
#[cfg(test)]
mod e2e_tests {
    use super::*;
    use std::time::Instant;

    /// PROOF STEP 1 — the REAL download path: `spawn_download` → streaming
    /// SHA256 → verify → atomic rename → `compute_status` reports `ready`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn builtin_e2e_download() {
        if model_on_disk() {
            let status = compute_status(&BuiltinShared::new());
            assert_eq!(status.state, BuiltinStateKind::Ready);
            println!("model already on disk — state=ready path={:?}", status.path);
            return;
        }
        // spawn_download rides tauri::async_runtime — point it at this test's
        // tokio runtime (exactly what tauri does at app boot).
        tauri::async_runtime::set(tokio::runtime::Handle::current());
        let shared = Arc::new(BuiltinShared::new());
        let emit: EmitFn = Arc::new(|ev: BuiltinModelEvent| {
            println!(
                "[builtin-model-event] phase={} pct={:.1} bytes={}/{}{}",
                ev.phase,
                ev.pct,
                ev.bytes_done,
                ev.bytes_total,
                ev.message.map(|m| format!(" msg={m}")).unwrap_or_default()
            );
        });
        let started = Instant::now();
        spawn_download(shared.clone(), emit);
        // Mid-download the status machine must read downloading/verifying.
        tokio::time::sleep(Duration::from_secs(3)).await;
        {
            let st = compute_status(&shared);
            assert!(
                matches!(
                    st.state,
                    BuiltinStateKind::Downloading | BuiltinStateKind::Verifying
                ),
                "mid-flight state must be downloading/verifying, got {:?}",
                st.state
            );
        }
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let (in_flight, err) = {
                let d = shared.download.lock().unwrap();
                (d.in_flight, d.last_error.clone())
            };
            if !in_flight {
                if let Some(e) = err {
                    panic!("download failed: {e}");
                }
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(3600),
                "download exceeded 60 min ceiling"
            );
        }
        let secs = started.elapsed().as_secs_f64();
        println!(
            "download+verify finished in {secs:.1}s ({:.1} MB/s avg over {} bytes)",
            BUILTIN_MODEL_SIZE as f64 / (1024.0 * 1024.0) / secs,
            BUILTIN_MODEL_SIZE
        );
        assert!(model_on_disk(), "verified GGUF must sit at the final path");
        let status = compute_status(&shared);
        assert_eq!(
            status.state,
            BuiltinStateKind::Ready,
            "status must report ready after the verified rename"
        );
        println!("state=ready path={:?}", status.path);
    }

    /// Tiny TCP splice proxy that records every byte the client sends so the
    /// test can assert on the actual HTTP request line rig emits.
    async fn start_capture_proxy(upstream_port: u16) -> (u16, Arc<std::sync::Mutex<Vec<u8>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind capture proxy");
        let port = listener.local_addr().expect("proxy addr").port();
        let captured: Arc<std::sync::Mutex<Vec<u8>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let cap_outer = captured.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut inbound, _)) = listener.accept().await else {
                    break;
                };
                let cap = cap_outer.clone();
                tokio::spawn(async move {
                    let Ok(mut outbound) =
                        tokio::net::TcpStream::connect(("127.0.0.1", upstream_port)).await
                    else {
                        return;
                    };
                    let (mut ri, mut wi) = inbound.split();
                    let (mut ro, mut wo) = outbound.split();
                    let upstreamward = async {
                        let mut buf = [0u8; 16384];
                        loop {
                            let n = match ri.read(&mut buf).await {
                                Ok(0) | Err(_) => break,
                                Ok(n) => n,
                            };
                            {
                                let mut c = cap.lock().unwrap();
                                if c.len() < 8 * 1024 * 1024 {
                                    c.extend_from_slice(&buf[..n]);
                                }
                            }
                            if wo.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                        let _ = wo.shutdown().await;
                    };
                    let clientward = async {
                        let mut buf = [0u8; 16384];
                        loop {
                            let n = match ro.read(&mut buf).await {
                                Ok(0) | Err(_) => break,
                                Ok(n) => n,
                            };
                            if wi.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                        let _ = wi.shutdown().await;
                    };
                    tokio::join!(upstreamward, clientward);
                });
            }
        });
        (port, captured)
    }

    /// PROOF STEPS 2–4 — real sidecar spawn via `ensure_running`, a REAL
    /// agentic harness turn through `run_turn` (all 10 tools attached, the
    /// production preamble), request-path assertion, and clean shutdown.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn builtin_e2e() {
        use crate::agents::scanner::scan_all_skills;
        use crate::harness::agent::{run_turn, TurnRequest};
        use crate::harness::config::{HarnessSettings, ProviderConfig};
        use crate::harness::memory::conversation::ConversationMemory;
        use crate::harness::providers;
        use crate::harness::session;
        use crate::harness::telemetry::{self, TelemetryWriter};
        use crate::harness::tools::HarnessContext;

        assert!(
            model_on_disk(),
            "model missing — run builtin_e2e_download first"
        );

        // -- 2. spawn the sidecar via the real lifecycle fn ------------------
        let exe = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("llama")
            .join("llama-server.exe");
        let shared = Arc::new(BuiltinShared::new());
        let t_spawn = Instant::now();
        let endpoint = ensure_running(&shared, exe)
            .await
            .expect("sidecar spawn + health");
        let spawn_ms = t_spawn.elapsed().as_millis();
        let status = compute_status(&shared);
        assert_eq!(status.state, BuiltinStateKind::Running);
        let port = status.port.expect("running sidecar exposes its port");
        assert_eq!(endpoint, format!("http://127.0.0.1:{port}/v1"));
        println!("sidecar up: endpoint={endpoint} port={port} model_loaded_in={spawn_ms}ms");

        // Direct /health probe + latency.
        let t_h = Instant::now();
        let res = reqwest::get(format!("http://127.0.0.1:{port}/health"))
            .await
            .expect("health request");
        let health_ms = t_h.elapsed().as_millis();
        assert_eq!(res.status().as_u16(), 200, "/health must answer 200");
        println!("/health -> 200 in {health_ms}ms");

        // PID for the orphan check after stop().
        let pid = {
            let guard = shared.sidecar.lock().await;
            guard
                .as_ref()
                .and_then(|p| p.child.id())
                .expect("live child pid")
        };

        // Splice proxy so we can assert the exact request path rig uses.
        let (proxy_port, captured) = start_capture_proxy(port).await;

        // -- 3. the real harness turn ----------------------------------------
        let settings = HarnessSettings::default();
        let config = ProviderConfig {
            id: BUILTIN_PROVIDER_ID.into(),
            kind: "builtin".into(),
            label: "Built-in model (e2e)".into(),
            // Same shape harness_builtin_start persists, but through the
            // capture proxy so the wire path is provable.
            endpoint: Some(format!("http://127.0.0.1:{proxy_port}/v1")),
            api_key: None,
            deployment: None,
            api_version: None,
            default_model: Some(BUILTIN_MODEL_ID.into()),
            notes: None,
        };
        let client = Arc::new(providers::resolve(config).expect("resolve builtin"));
        assert_eq!(client.kind(), "builtin");
        let scan = scan_all_skills();
        println!("local skill registry: {} skills", scan.skills.len());
        let ctx = HarnessContext::new(scan.skills, settings.clone()).expect("harness ctx");
        let session_id = format!("builtin-e2e-{}", std::process::id());
        let telemetry_writer = Arc::new(TelemetryWriter::open(&session_id).expect("telemetry"));
        let mut memory = ConversationMemory::new(&session_id);
        let req = TurnRequest {
            session_id: session_id.clone(),
            provider_id: BUILTIN_PROVIDER_ID.into(),
            model_id: BUILTIN_MODEL_ID.into(),
            model_profile: None,
            user_text: "find me skills for testing rust".into(),
        };
        let t_turn = Instant::now();
        let summary = run_turn(
            client,
            ctx,
            settings,
            telemetry_writer,
            None,
            &mut memory,
            req,
        )
        .await
        .expect("turn");
        let turn_secs = t_turn.elapsed().as_secs_f64();

        // -- transcript dump ---------------------------------------------------
        println!("--- transcript (memory records) ---");
        for m in &memory.messages {
            let head: String = m.text.chars().take(220).collect();
            println!(
                "[{}] {}{}",
                m.role,
                m.name
                    .as_ref()
                    .map(|n| format!("tool={n} args={} -> ", m.args_json.as_deref().unwrap_or("")))
                    .unwrap_or_default(),
                head.replace('\n', " | ")
            );
        }
        println!("--- final answer ---\n{}", summary.final_text);
        println!(
            "tool_calls_made={} in_tokens={} out_tokens={} duration_ms={} wall={:.1}s ({:.1} out-tok/s)",
            summary.tool_calls_made,
            summary.in_tokens,
            summary.out_tokens,
            summary.duration_ms,
            turn_secs,
            summary.out_tokens as f64 / turn_secs
        );

        // -- assertions (a)-(d) -------------------------------------------------
        let wire = {
            let c = captured.lock().unwrap();
            String::from_utf8_lossy(&c).to_string()
        };
        let request_lines: Vec<&str> = wire
            .lines()
            .filter(|l| l.contains("HTTP/1.1") && !l.starts_with("HTTP/1.1"))
            .collect();
        println!("captured request lines: {request_lines:?}");
        assert!(
            wire.contains("POST /v1/chat/completions HTTP/1.1"),
            "(a) rig must hit /v1/chat/completions — captured: {request_lines:?}"
        );
        assert!(
            !wire.contains("/v1/responses"),
            "(a) the Responses API must never be hit — captured: {request_lines:?}"
        );
        assert!(
            summary.tool_calls_made >= 1,
            "(b) at least one real tool call must fire"
        );
        assert!(
            summary.final_text.trim().len() >= 40,
            "(c) final answer must be substantive, got: {:?}",
            summary.final_text
        );
        assert!(
            summary.in_tokens > 0 && summary.out_tokens > 0,
            "(d) usage tokens must be non-zero (in={} out={})",
            summary.in_tokens,
            summary.out_tokens
        );

        // Session + telemetry artifacts out of the user's real history.
        let _ = session::delete(&session_id);
        if let Ok(p) = telemetry::telemetry_path(&session_id) {
            let _ = std::fs::remove_file(p);
        }

        // -- 4. lifecycle: stop() must leave no orphan ---------------------------
        stop(&shared).await.expect("stop sidecar");
        tokio::time::sleep(Duration::from_millis(700)).await;
        let sys = sysinfo::System::new_all();
        let alive = sys.process(sysinfo::Pid::from_u32(pid));
        assert!(
            alive.is_none(),
            "llama-server pid {pid} must be gone after stop()"
        );
        let dead = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("client")
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await;
        assert!(
            dead.is_err() || !dead.expect("resp").status().is_success(),
            "port {port} must stop answering after stop()"
        );
        let status = compute_status(&shared);
        assert_eq!(status.state, BuiltinStateKind::Ready, "back to ready");
        println!("sidecar stopped cleanly: pid {pid} gone, port {port} closed, state=ready");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- sha verify + finalize --------------------------------------------

    #[test]
    fn sha256_of_known_bytes_matches() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("data.bin");
        std::fs::write(&p, b"skill deck builtin model test payload").expect("write");
        let got = sha256_hex_of_file(&p).expect("hash");
        // Independently computed with sha2 in-process to pin the helper.
        let expected = hex_lower(&Sha256::digest(b"skill deck builtin model test payload"));
        assert_eq!(got, expected);
        assert_eq!(got.len(), 64);
    }

    #[test]
    fn finalize_renames_on_matching_digest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let part = dir.path().join("m.gguf.part");
        let dest = dir.path().join("m.gguf");
        std::fs::write(&part, b"model-bytes").expect("write");
        let digest = sha256_hex_of_file(&part).expect("hash");
        let ok = finalize_verified(&part, &dest, &digest, &digest).expect("finalize");
        assert!(ok, "matching digest must finalize");
        assert!(dest.exists(), "renamed into place");
        assert!(!part.exists(), "part gone after rename");
    }

    #[test]
    fn finalize_accepts_uppercase_expected_digest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let part = dir.path().join("m.gguf.part");
        let dest = dir.path().join("m.gguf");
        std::fs::write(&part, b"model-bytes").expect("write");
        let digest = sha256_hex_of_file(&part).expect("hash");
        let ok =
            finalize_verified(&part, &dest, &digest, &digest.to_ascii_uppercase()).expect("ok");
        assert!(ok, "digest compare must be case-insensitive");
    }

    #[test]
    fn finalize_deletes_part_on_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let part = dir.path().join("m.gguf.part");
        let dest = dir.path().join("m.gguf");
        std::fs::write(&part, b"corrupted-bytes").expect("write");
        let ok = finalize_verified(&part, &dest, "deadbeef", BUILTIN_MODEL_SHA256)
            .expect("finalize call itself succeeds");
        assert!(!ok, "mismatch reported");
        assert!(!part.exists(), "poisoned part deleted");
        assert!(!dest.exists(), "dest never created on mismatch");
    }

    // -- free port ----------------------------------------------------------

    #[test]
    fn alloc_free_port_returns_bindable_port() {
        let port = alloc_free_port().expect("alloc");
        assert!(port > 0);
        // The port must be immediately re-bindable (we dropped the listener).
        let rebind = std::net::TcpListener::bind(("127.0.0.1", port));
        assert!(rebind.is_ok(), "port {port} not bindable after alloc");
    }

    #[test]
    fn alloc_free_port_avoids_an_occupied_port() {
        // Hold a listener open; a fresh alloc must never return ITS port.
        let held = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let held_port = held.local_addr().expect("addr").port();
        for _ in 0..16 {
            let port = alloc_free_port().expect("alloc");
            assert_ne!(port, held_port, "OS must not hand out an occupied port");
        }
    }

    // -- available-RAM sidecar admission --------------------------------------

    #[test]
    fn ram_preflight_accepts_the_documented_threshold() {
        assert!(ensure_available_ram(Some(MIN_AVAILABLE_RAM_BYTES)).is_ok());
        assert!(ensure_available_ram(Some(MIN_AVAILABLE_RAM_BYTES + 1)).is_ok());
    }

    #[test]
    fn ram_preflight_rejects_below_threshold() {
        let error = ensure_available_ram(Some(MIN_AVAILABLE_RAM_BYTES - 1))
            .expect_err("one byte below the threshold must fail closed")
            .to_string();
        assert!(error.contains("not enough available RAM"));
        assert!(error.contains("Jan-nano 4B"));
    }

    #[test]
    fn ram_preflight_rejects_unknown_telemetry() {
        let error = ensure_available_ram(None)
            .expect_err("unknown available RAM must fail closed")
            .to_string();
        assert!(error.contains("could not determine available RAM"));
    }

    // -- status state machine ------------------------------------------------

    #[test]
    fn derive_state_precedence() {
        use BuiltinStateKind::*;
        // download in flight always wins, verifying flag refines it
        assert_eq!(derive_state(true, false, false, false), Downloading);
        assert_eq!(derive_state(true, true, false, false), Verifying);
        assert_eq!(derive_state(true, false, true, true), Downloading);
        // sidecar beats on-disk
        assert_eq!(derive_state(false, false, true, true), Running);
        // on-disk without sidecar = ready
        assert_eq!(derive_state(false, false, false, true), Ready);
        // nothing anywhere
        assert_eq!(derive_state(false, false, false, false), NotDownloaded);
    }

    #[test]
    fn status_serializes_camelcase_contract() {
        let status = BuiltinStatus {
            state: BuiltinStateKind::NotDownloaded,
            progress_pct: Some(12.5),
            bytes_done: Some(10),
            bytes_total: Some(80),
            model_name: BUILTIN_MODEL_NAME.into(),
            size_bytes: Some(BUILTIN_MODEL_SIZE),
            path: None,
            port: None,
        };
        let v = serde_json::to_value(&status).expect("serialize");
        assert_eq!(v["state"], "notDownloaded");
        assert_eq!(v["progressPct"], 12.5);
        assert_eq!(v["bytesDone"], 10);
        assert_eq!(v["bytesTotal"], 80);
        assert_eq!(v["modelName"], BUILTIN_MODEL_NAME);
        assert_eq!(v["sizeBytes"], BUILTIN_MODEL_SIZE);
        // absent optionals are omitted, not null
        assert!(v.get("path").is_none());
        assert!(v.get("port").is_none());
        // every state kind uses the camelCase wire literals
        for (kind, wire) in [
            (BuiltinStateKind::NotDownloaded, "notDownloaded"),
            (BuiltinStateKind::Downloading, "downloading"),
            (BuiltinStateKind::Verifying, "verifying"),
            (BuiltinStateKind::Ready, "ready"),
            (BuiltinStateKind::Running, "running"),
        ] {
            assert_eq!(serde_json::to_value(kind).expect("kind"), wire);
        }
    }

    #[test]
    fn event_serializes_camelcase_contract() {
        let ev = BuiltinModelEvent {
            phase: "downloading".into(),
            pct: 42.0,
            bytes_done: 42,
            bytes_total: 100,
            message: None,
        };
        let v = serde_json::to_value(&ev).expect("serialize");
        assert_eq!(v["phase"], "downloading");
        assert_eq!(v["pct"], 42.0);
        assert_eq!(v["bytesDone"], 42);
        assert_eq!(v["bytesTotal"], 100);
        assert!(v.get("message").is_none(), "None message omitted");
    }

    // -- download bookkeeping -------------------------------------------------

    #[test]
    fn begin_download_is_exclusive_and_end_releases() {
        let shared = BuiltinShared::new();
        let cancel = begin_download(&shared).expect("first begin succeeds");
        assert!(begin_download(&shared).is_none(), "second begin refused");
        cancel_download(&shared);
        assert!(
            cancel.load(Ordering::SeqCst),
            "cancel flag reaches the task"
        );
        end_download(&shared, None);
        assert!(begin_download(&shared).is_some(), "free after end");
    }

    // -- host allowlist ---------------------------------------------------------

    #[test]
    fn host_allowlist_admits_model_and_runtime_hosts() {
        for host in [
            "huggingface.co",
            "cdn-lfs.huggingface.co",
            "cdn-lfs-us-1.hf.co",
            "cas-bridge.xethub.hf.co",
            "github.com",
            "objects.githubusercontent.com",
            "release-assets.githubusercontent.com",
        ] {
            assert!(host_allowed(host), "should allow {host}");
        }
    }

    #[test]
    fn host_allowlist_rejects_everything_else() {
        for host in [
            "evil.com",
            "huggingface.co.evil.com",
            "notgithub.com",
            "localhost",
            "127.0.0.1",
            "xhf.co",       // suffix must match on a dot boundary
            "fakehf.co.uk", // unrelated TLD
        ] {
            assert!(!host_allowed(host), "must reject {host}");
        }
    }

    // -- provider injection ------------------------------------------------------

    #[test]
    fn ensure_builtin_provider_injects_first_and_is_idempotent() {
        let mut cfg = crate::models::AppConfig::default();
        assert!(ensure_builtin_provider(&mut cfg), "first call injects");
        assert_eq!(cfg.harness_providers.len(), 1);
        let p = &cfg.harness_providers[0];
        assert_eq!(p.id, BUILTIN_PROVIDER_ID);
        assert_eq!(p.kind, "builtin");
        assert_eq!(p.default_model.as_deref(), Some(BUILTIN_MODEL_ID));
        assert!(p.endpoint.is_none(), "endpoint set only at sidecar start");
        assert!(!ensure_builtin_provider(&mut cfg), "second call no-ops");
        assert_eq!(cfg.harness_providers.len(), 1);
    }

    #[test]
    fn ensure_builtin_provider_prepends_before_user_providers() {
        let mut cfg = crate::models::AppConfig::default();
        cfg.harness_providers
            .push(crate::harness::config::ProviderConfig {
                id: "my-ollama".into(),
                kind: "ollama".into(),
                label: "Ollama".into(),
                endpoint: None,
                api_key: None,
                deployment: None,
                api_version: None,
                default_model: None,
                notes: None,
            });
        assert!(ensure_builtin_provider(&mut cfg));
        assert_eq!(
            cfg.harness_providers[0].kind, "builtin",
            "builtin listed first"
        );
        assert_eq!(cfg.harness_providers[1].id, "my-ollama");
    }

    // -- manifest sanity -----------------------------------------------------------

    #[test]
    fn pinned_manifest_is_well_formed() {
        assert_eq!(BUILTIN_MODEL_SHA256.len(), 64);
        assert!(BUILTIN_MODEL_SHA256.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(BUILTIN_MODEL_URL.starts_with("https://huggingface.co/"));
        assert!(BUILTIN_MODEL_URL.contains(BUILTIN_MODEL_REVISION));
        assert!(!BUILTIN_MODEL_URL.contains("/resolve/main/"));
        assert_eq!(MIN_AVAILABLE_RAM_BYTES, 5 * 1024 * 1024 * 1024);
        assert_eq!(endpoint_for(8080), "http://127.0.0.1:8080/v1");
    }
}
