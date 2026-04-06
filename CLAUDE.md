# Claude Code Instructions for Skill Deck

This is a Tauri v2 desktop app with a Rust backend and Svelte 5 frontend.

## Build Commands
- `pnpm install` — install frontend dependencies
- `pnpm tauri dev` — run in development mode
- `pnpm tauri build` — create production binary
- `cd src-tauri && cargo test` — run Rust tests
- `cd src-tauri && cargo check` — quick compilation check
- `pnpm check` — Svelte/TypeScript type check

## Architecture Rules

The app uses an **adapter pattern** for supporting 15+ coding agents. The flow is:
1. `agents/registry.rs` defines each agent's file paths and format
2. `agents/scanner.rs` globs those paths and passes files to the right parser
3. `parsers/frontmatter.rs` is the base parser (Markdown + YAML frontmatter)
4. Everything normalizes into `models/skill.rs` — the universal Skill struct
5. The frontend only knows about the universal types, never agent-specific formats

## When Adding Features

- **New agent support**: Only modify `models/skill.rs` (AgentId enum) and `agents/registry.rs` (paths + format)
- **New parser**: Add to `parsers/`, implement `parse_*` that returns `Result<Skill>`
- **New IPC command**: Add to `commands/`, register in `lib.rs` `generate_handler![]`
- **New UI component**: Add to `src/lib/components/`, use Svelte 5 runes

## Code Style

- Rust comments with `@agent-context:` prefix are for LLM comprehension — preserve them
- All Tauri IPC types use `camelCase` serialization (`#[serde(rename_all = "camelCase")]`)
- Frontend calls backend via `invoke('command_name', { params })` from `@tauri-apps/api/core`
- Use Svelte 5 runes: `$state()`, `$derived()`, `$effect()` — NOT legacy `writable()`/`derived()`

## Do NOT

- Remove `@agent-context:` comments — they're structural documentation for agents
- Change the `AgentId` enum without updating `registry.rs`
- Add `.unwrap()` in production Rust code
- Use Svelte 4 reactive syntax ($ prefix on variables, reactive declarations)
- Modify `.research/` files — they're reference docs, not source
