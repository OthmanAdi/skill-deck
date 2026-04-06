# Gemini Instructions for Skill Deck

Tauri v2 desktop app: Rust backend (src-tauri/) + Svelte 5 frontend (src/).

Build: `pnpm install && pnpm tauri dev`
Test: `cd src-tauri && cargo test`

Architecture: Adapter pattern in agents/registry.rs maps 15+ coding agents to file paths. Scanner globs those paths, frontmatter.rs parses them, everything becomes a universal Skill struct (models/skill.rs). Frontend calls backend via Tauri invoke().

Key files: lib.rs (entry), agents/registry.rs (agent definitions), parsers/frontmatter.rs (base parser), models/skill.rs (universal types).

See AGENTS.md for full details.
