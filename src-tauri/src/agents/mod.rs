// @agent-context: Agent adapter system — the core of Skill Deck's universality.
//
// ARCHITECTURE:
// - registry.rs defines ALL known agents with their paths and formats
// - scanner.rs walks the filesystem, using the registry to find skills
// - Each agent's paths use $HOME and $PROJECT placeholders, resolved at runtime
//
// ADDING A NEW AGENT:
// 1. Add an AgentId variant in models/skill.rs
// 2. Add an entry to AGENT_REGISTRY in registry.rs (paths, format, color)
// 3. If the format is truly novel, add a parser in parsers/
// 4. That's it — scanner.rs picks it up automatically

pub mod registry;
pub mod scanner;

pub use scanner::*;
