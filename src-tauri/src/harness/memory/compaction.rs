// @agent-context: Compaction pipeline — three strategies, applied in order,
// short-circuit as soon as we are below the floor again.
//
// 1. `ClearStaleToolResults` — replace tool_result bodies older than N records
//    with a one-line stub. Cheap, recovers a lot of context fast.
// 2. `AnchoredSummarize` — feed running_summary + oldest half of records into
//    a summarizing model call; merge the new summary into running_summary;
//    drop the summarized records. The records are dropped ONLY AFTER the
//    summarizer succeeds — a flaky provider must never lose transcript.
// 3. `SpillToDisk` — for any single record over `tool_result_spill_threshold_chars`,
//    write to `{config_dir}/skill-deck/spill/<session>/<record_id>.txt` and
//    replace the in-memory body with a `[spilled to <path>]` pointer.
//
// Worst-case bounds:
// - When the model's context window is UNKNOWN (no directory profile, Ollama
//   /api/show unreachable), compaction still runs against
//   `FALLBACK_CONTEXT_TOKENS` instead of never firing — a session must not
//   grow unboundedly just because metadata was missing.
// - The block handed to the summarizer is capped at
//   `SUMMARIZE_BLOCK_MAX_CHARS` (head + tail slices) so a 10k-message session
//   cannot construct a summarization prompt bigger than the model itself.

use crate::harness::config::HarnessSettings;
use crate::harness::memory::conversation::{ConversationMemory, SpillRef};
use crate::harness::model_directory::ModelProfile;
use crate::harness::tokens::Counter;
use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::future::Future;
use std::path::PathBuf;

/// Assumed context ceiling (tokens) when the model's real window is unknown.
/// Conservative middle ground: small enough that genuinely small local models
/// aren't overflowed for long, large enough not to thrash summarization on
/// short chats.
pub const FALLBACK_CONTEXT_TOKENS: u32 = 16_384;

/// Cap on the flattened text block sent to the summarizer in one call.
/// Head + tail are kept (earliest context anchors the summary, latest
/// preserves continuity with the retained half).
pub const SUMMARIZE_BLOCK_MAX_CHARS: usize = 48_000;

/// Deterministic in-process summary used by default. This is intentionally an
/// extractive compactor, not an LLM call: transcript text never leaves the
/// machine merely because the conversation crossed the compaction floor.
pub fn local_summary(prior: &str, block: &str, max_tokens: u32) -> String {
    let max_chars = usize::try_from(max_tokens)
        .unwrap_or(usize::MAX / 3)
        .saturating_mul(3)
        .max(256);
    let header = "[local extractive compaction]\n";
    let prior_section = if prior.trim().is_empty() {
        String::new()
    } else {
        format!(
            "Prior anchor:\n{}\n\n",
            bounded_excerpt(prior, max_chars / 4)
        )
    };
    let available = max_chars
        .saturating_sub(header.chars().count())
        .saturating_sub(prior_section.chars().count());
    format!(
        "{header}{prior_section}Older transcript evidence:\n{}",
        bounded_excerpt(block, available)
    )
}

fn bounded_excerpt(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let marker = format!("\n[... {} local chars compacted ...]\n", count - max_chars);
    let keep = max_chars.saturating_sub(marker.chars().count());
    let head_count = keep / 2;
    let tail_count = keep.saturating_sub(head_count);
    let head: String = text.chars().take(head_count).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(tail_count)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{head}{marker}{tail}")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionEvent {
    pub strategy: String,
    pub before_tokens: u32,
    pub after_tokens: u32,
    pub freed_tokens: u32,
    pub records_touched: usize,
}

pub fn spill_dir(session_id: &str) -> Result<PathBuf> {
    let base = dirs::config_dir().context("no config dir")?;
    let dir = base
        .join("skill-deck")
        .join("spill")
        .join(sanitize(session_id));
    fs::create_dir_all(&dir).context("create spill dir")?;
    Ok(dir)
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn estimate_tokens(memory: &ConversationMemory, model: Option<&ModelProfile>) -> u32 {
    let body = memory.corpus_for_tokens();
    let kind = model
        .map(|m| m.tokenizer_hint.clone())
        .unwrap_or(crate::harness::tokens::TokenizerKind::CharsApproximation);
    Counter::count(&kind, &body)
}

/// Top-level compaction entry. Runs the strategy chain until the corpus is
/// back under the floor (or strategies are exhausted). `summarize_fn` is a
/// caller-supplied async closure that produces a fresh summary given the
/// running_summary + a flattened block of older messages.
pub async fn compact_if_needed<F, Fut>(
    memory: &mut ConversationMemory,
    model: Option<&ModelProfile>,
    settings: &HarnessSettings,
    summarize_fn: F,
) -> Result<Vec<CompactionEvent>>
where
    F: Fn(String, String) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    // Unknown context window → assume a conservative ceiling instead of
    // skipping compaction entirely (unbounded growth was the old behavior).
    let ceiling = model
        .and_then(|m| m.context_window)
        .unwrap_or(FALLBACK_CONTEXT_TOKENS);
    let floor_tokens = (ceiling as f64 * settings.memory_compaction_floor).floor() as u32;
    let mut events = Vec::new();
    let mut before = estimate_tokens(memory, model);
    if before <= floor_tokens {
        return Ok(events);
    }

    // 1. Clear stale tool results
    let (touched, after) = clear_stale_tool_results(memory, model, /*keep_recent*/ 4);
    events.push(CompactionEvent {
        strategy: "clear-stale-tool-results".into(),
        before_tokens: before,
        after_tokens: after,
        freed_tokens: before.saturating_sub(after),
        records_touched: touched,
    });
    before = after;
    if before <= floor_tokens {
        return Ok(events);
    }

    // 2. Anchored summarize. One pass halves the transcript, which is not
    //    enough for an adversarially long session — loop (bounded) until the
    //    corpus is under the floor or the transcript is too short to halve.
    //    MAX_SUMMARIZE_ROUNDS = 8 turns 10k records into ~40 in one call.
    const MAX_SUMMARIZE_ROUNDS: usize = 8;
    for _ in 0..MAX_SUMMARIZE_ROUNDS {
        let touched = anchored_summarize(memory, &summarize_fn).await?;
        if touched == 0 {
            break;
        }
        let after = estimate_tokens(memory, model);
        events.push(CompactionEvent {
            strategy: "anchored-summarize".into(),
            before_tokens: before,
            after_tokens: after,
            freed_tokens: before.saturating_sub(after),
            records_touched: touched,
        });
        before = after;
        if before <= floor_tokens {
            return Ok(events);
        }
    }

    // 3. Spill large records
    let (touched, after) = spill_large_records(memory, settings, model)?;
    events.push(CompactionEvent {
        strategy: "spill-to-disk".into(),
        before_tokens: before,
        after_tokens: after,
        freed_tokens: before.saturating_sub(after),
        records_touched: touched,
    });

    Ok(events)
}

fn clear_stale_tool_results(
    memory: &mut ConversationMemory,
    model: Option<&ModelProfile>,
    keep_recent: usize,
) -> (usize, u32) {
    let total = memory.messages.len();
    let mut touched = 0;
    for (i, m) in memory.messages.iter_mut().enumerate() {
        let from_end = total - i;
        if from_end > keep_recent && m.role == "tool" && !m.cleared {
            m.text = "[old tool result cleared by compaction]".to_string();
            m.cleared = true;
            touched += 1;
        }
    }
    (touched, estimate_tokens(memory, model))
}

/// Flatten the oldest `cut` records into a bounded block for the summarizer.
/// When the raw block exceeds `SUMMARIZE_BLOCK_MAX_CHARS`, keep head + tail
/// halves with an elision marker — the summary keeps chronological anchors on
/// both ends without ever building an unbounded prompt.
fn bounded_block(records: &[crate::harness::memory::conversation::MessageRecord]) -> String {
    let lines: Vec<String> = records
        .iter()
        .map(|m| format!("{}: {}", m.role, m.text))
        .collect();
    let total_chars: usize = lines.iter().map(|l| l.chars().count() + 1).sum();
    if total_chars <= SUMMARIZE_BLOCK_MAX_CHARS {
        return lines.join("\n");
    }
    let half = SUMMARIZE_BLOCK_MAX_CHARS / 2;
    let mut head = String::new();
    for line in &lines {
        if head.chars().count() + line.chars().count() + 1 > half {
            break;
        }
        head.push_str(line);
        head.push('\n');
    }
    let mut tail_lines: Vec<&String> = Vec::new();
    let mut tail_chars = 0usize;
    for line in lines.iter().rev() {
        let len = line.chars().count() + 1;
        if tail_chars + len > half {
            break;
        }
        tail_chars += len;
        tail_lines.push(line);
    }
    tail_lines.reverse();
    let tail: String = tail_lines
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{head}\n…[{} chars elided for summarization]…\n\n{tail}",
        total_chars.saturating_sub(head.chars().count() + tail.chars().count())
    )
}

async fn anchored_summarize<F, Fut>(
    memory: &mut ConversationMemory,
    summarize_fn: &F,
) -> Result<usize>
where
    F: Fn(String, String) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let total = memory.messages.len();
    if total < 4 {
        return Ok(0);
    }
    let cut = total / 2;
    // Build the block WITHOUT draining, so a summarizer failure (flaky
    // provider mid-compaction) leaves the transcript fully intact.
    let block = bounded_block(&memory.messages[..cut]);
    let prior = memory.running_summary.clone().unwrap_or_default();
    let new_summary = summarize_fn(prior, block).await?;
    memory.running_summary = Some(new_summary);
    memory.messages.drain(..cut);
    Ok(cut)
}

fn spill_large_records(
    memory: &mut ConversationMemory,
    settings: &HarnessSettings,
    model: Option<&ModelProfile>,
) -> Result<(usize, u32)> {
    let threshold = settings.tool_result_spill_threshold_chars;
    let mut touched = 0;
    let dir = spill_dir(&memory.session_id)?;
    for m in memory.messages.iter_mut() {
        if !m.cleared && m.spilled_path.is_none() && m.text.chars().count() > threshold {
            let block_id = format!("{}.txt", m.id);
            let path = dir.join(&block_id);
            let byte_count = m.text.len() as u64;
            fs::write(&path, &m.text).context("spill write")?;
            let path_str = path.to_string_lossy().to_string();
            m.text = format!("[spilled to {path_str}]");
            m.spilled_path = Some(path_str.clone());
            memory.spilled_blocks.push(SpillRef {
                block_id: m.id.clone(),
                path: path_str,
                byte_count,
            });
            touched += 1;
        }
    }
    Ok((touched, estimate_tokens(memory, model)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::config::HarnessSettings;

    fn settings() -> HarnessSettings {
        HarnessSettings::default()
    }

    fn filled_memory(records: usize, chars_each: usize) -> ConversationMemory {
        let mut m = ConversationMemory::new("compaction-test");
        for i in 0..records {
            let body = format!("msg {i} {}", "y".repeat(chars_each));
            if i % 2 == 0 {
                m.push_user(body);
            } else {
                m.push_assistant(body);
            }
        }
        m
    }

    fn ok_summarizer(
    ) -> impl Fn(String, String) -> std::pin::Pin<Box<dyn Future<Output = Result<String>>>> {
        |prior: String, block: String| {
            Box::pin(async move {
                Ok(format!(
                    "[merged summary: prior={} block={} chars]",
                    prior.chars().count(),
                    block.chars().count()
                ))
            })
        }
    }

    #[tokio::test]
    async fn no_compaction_below_floor() {
        let mut m = filled_memory(4, 20);
        let events = compact_if_needed(&mut m, None, &settings(), ok_summarizer())
            .await
            .expect("compact");
        assert!(events.is_empty());
        assert_eq!(m.messages.len(), 4);
    }

    #[tokio::test]
    async fn compaction_fires_without_a_model_profile() {
        // Old behavior: unknown context window → compaction never ran and the
        // session grew forever. Now the fallback ceiling engages.
        // FALLBACK floor = 0.7 * 16384 ≈ 11468 tokens ≈ ~35k chars via the
        // chars/4×1.3 approximation. 40 records × 2000 chars ≈ 80k chars.
        let mut m = filled_memory(40, 2_000);
        let events = compact_if_needed(&mut m, None, &settings(), ok_summarizer())
            .await
            .expect("compact");
        assert!(
            !events.is_empty(),
            "fallback ceiling must trigger compaction"
        );
        assert!(
            m.messages.len() < 40,
            "anchored summarize must have dropped older records"
        );
        assert!(m.running_summary.is_some());
        let final_tokens = estimate_tokens(&m, None);
        assert!(
            final_tokens < 40 * 2_000,
            "corpus must shrink, got {final_tokens}"
        );
    }

    #[tokio::test]
    async fn summarizer_failure_loses_no_transcript() {
        let mut m = filled_memory(40, 2_000);
        let before_records = m.messages.len();
        let failing = |_prior: String, _block: String| async move {
            Err::<String, _>(anyhow::anyhow!("provider hiccup mid-compaction"))
        };
        let err = compact_if_needed(&mut m, None, &settings(), failing)
            .await
            .expect_err("summarizer failure surfaces");
        assert!(err.to_string().contains("hiccup"));
        // The drained-half must still be present: no data loss on failure.
        assert_eq!(m.messages.len(), before_records);
        assert!(m.running_summary.is_none());
    }

    #[test]
    fn bounded_block_caps_adversarial_input() {
        // 30 records × 10k chars = 300k chars raw — 6x over the cap.
        let m = filled_memory(30, 10_000);
        let block = bounded_block(&m.messages);
        let block_chars = block.chars().count();
        assert!(
            block_chars <= SUMMARIZE_BLOCK_MAX_CHARS + 200,
            "block must be capped, got {block_chars} chars"
        );
        assert!(block.contains("chars elided for summarization"));
        // Head keeps the oldest record, tail keeps the newest of the slice.
        assert!(block.contains("msg 0 "));
        assert!(block.contains("msg 29 "));
    }

    #[test]
    fn bounded_block_passes_small_input_through() {
        let m = filled_memory(4, 50);
        let block = bounded_block(&m.messages);
        assert!(!block.contains("elided"));
        assert!(block.contains("msg 0"));
        assert!(block.contains("msg 3"));
    }

    #[test]
    fn local_summary_is_bounded_deterministic_and_keeps_anchors() {
        let block = format!("first\n{}\nlast", "secret-local-body ".repeat(2_000));
        let a = local_summary("prior-id", &block, 384);
        let b = local_summary("prior-id", &block, 384);
        assert_eq!(a, b);
        assert!(a.contains("prior-id"));
        assert!(a.contains("first"));
        assert!(a.contains("last"));
        assert!(a.chars().count() <= 384 * 3 + 64);
    }

    #[tokio::test]
    async fn ten_thousand_message_session_stays_bounded() {
        // Adversarial marathon: 10k small records. The strategy chain must
        // bring the corpus under the fallback floor without the summarizer
        // ever seeing an unbounded block.
        let mut m = filled_memory(10_000, 40);
        let max_block = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let max_block_c = max_block.clone();
        let summarizer = move |_prior: String, block: String| {
            let max_block = max_block_c.clone();
            async move {
                max_block.fetch_max(block.chars().count(), std::sync::atomic::Ordering::SeqCst);
                Ok("[compact summary]".to_string())
            }
        };
        let events = compact_if_needed(&mut m, None, &settings(), summarizer)
            .await
            .expect("compact");
        assert!(!events.is_empty());
        assert!(
            max_block.load(std::sync::atomic::Ordering::SeqCst) <= SUMMARIZE_BLOCK_MAX_CHARS + 200,
            "summarizer block bounded even for 10k messages"
        );
        // The whole point: one compaction call converges under the floor.
        let floor = (FALLBACK_CONTEXT_TOKENS as f64 * settings().memory_compaction_floor) as u32;
        let final_tokens = estimate_tokens(&m, None);
        assert!(
            final_tokens <= floor,
            "10k-message session must end under the floor: {final_tokens} > {floor}"
        );
        // The loop halves until the TOKEN floor is met (that is the contract);
        // with 40-char records that lands well under 1k records.
        assert!(
            m.messages.len() < 1_000,
            "records collapsed, got {}",
            m.messages.len()
        );
    }
}
