// @agent-context: Parsers for all skill/rule file formats.
//
// ARCHITECTURE: One base parser (frontmatter.rs) handles 90% of formats since
// almost every coding agent uses Markdown + optional YAML frontmatter.
// Format-specific parsers (skill_md.rs, mdc.rs, etc.) layer on top.
//
// ADDING A NEW FORMAT:
// 1. Create a new parser file (e.g., new_format.rs)
// 2. Implement the `parse` function that returns a ParsedSkill
// 3. Register it in the agent's adapter (agents/your_agent.rs)

pub mod frontmatter;
pub mod skill_md;
pub mod claude_hooks;

pub use frontmatter::*;
