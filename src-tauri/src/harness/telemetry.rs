// @agent-context: JSONL telemetry writer.
// One file per session at `{config_dir}/skill-deck/telemetry/<session>.jsonl`.
// Each line is a serialized `HarnessEvent` so the CLI can `tail -f` or `jq`
// the run while it is in progress.

use crate::harness::events::HarnessEvent;
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct TelemetryWriter {
    inner: Mutex<TelemetryInner>,
}

struct TelemetryInner {
    path: PathBuf,
    file: BufWriter<File>,
}

pub fn telemetry_dir() -> Result<PathBuf> {
    let base = dirs::config_dir().context("no config dir")?;
    let dir = base.join("skill-deck").join("telemetry");
    std::fs::create_dir_all(&dir).context("create telemetry dir")?;
    Ok(dir)
}

pub fn telemetry_path(session_id: &str) -> Result<PathBuf> {
    let sane: String = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(telemetry_dir()?.join(format!("{sane}.jsonl")))
}

impl TelemetryWriter {
    pub fn open(session_id: &str) -> Result<Self> {
        let path = telemetry_path(session_id)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open telemetry {path:?}"))?;
        Ok(Self {
            inner: Mutex::new(TelemetryInner {
                path,
                file: BufWriter::new(file),
            }),
        })
    }

    pub fn append(&self, event: &HarnessEvent) -> Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|p| anyhow::anyhow!("telemetry mutex poisoned: {p:?}"))?;
        let json = serde_json::to_string(event).context("serialize event")?;
        writeln!(guard.file, "{json}").context("write event")?;
        guard.file.flush().context("flush event")?;
        Ok(())
    }

    pub fn path(&self) -> PathBuf {
        self.inner
            .lock()
            .map(|g| g.path.clone())
            .unwrap_or_default()
    }
}
