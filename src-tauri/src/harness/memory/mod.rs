// @agent-context: Conversation memory + compaction.
// Anchored summarization: the running_summary is never thrown away, only merged
// with the latest summary block. Large tool_results spill to disk when they
// cross settings.tool_result_spill_threshold_chars.

#![allow(dead_code)]

pub mod compaction;
pub mod conversation;
