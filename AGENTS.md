# Skill Deck — Agent Instructions

> Universal coding agent skill browser. Tauri v2 + Rust backend + Svelte 5 frontend.

## Quick Context

**What this is**: A desktop overlay app (triggered by global hotkey) that discovers and displays skills/rules from ALL major AI coding agents (Claude Code, Cursor, Copilot, Codex, Windsurf, Gemini, and 10+ more).

**Stack**: Tauri v2 (Rust) backend, Svelte 5 + Tailwind CSS v4 frontend, cross-platform (Windows/macOS/Linux).

**License**: MIT

## Build & Test

```bash
pnpm install              # install frontend deps
pnpm tauri dev            # run in dev mode (hot reload)
pnpm tauri build          # create production binary
cd src-tauri && cargo test # run Rust unit tests
pnpm check                # run Svelte type checks
```

## Architecture

```
src-tauri/src/           # Rust backend
├── lib.rs               # Entry point — wires plugins, commands, state
├── models/              # Data structures (Skill, AgentInfo, AppConfig)
│   ├── skill.rs         # Universal Skill struct — all adapters produce this
│   ├── agent.rs         # AgentInfo: paths, format, brand color per agent
│   └── config.rs        # User preferences (stars, hotkey, theme)
├── parsers/             # File format parsers
│   ├── frontmatter.rs   # Base YAML+MD parser (covers 90% of formats)
│   └── skill_md.rs      # SKILL.md parser (Claude Code / Codex)
├── agents/              # Agent adapter system
│   ├── registry.rs      # ALL 15+ agents: names, paths, formats, colors
│   └── scanner.rs       # Filesystem glob → parse → Vec<Skill>
├── commands/            # Tauri IPC commands (called from frontend)
│   ├── skills.rs        # scan_skills, list_agents, read_skill_content
│   ├── context.rs       # detect_terminal_context (CWD of focused terminal)
│   └── preferences.rs   # toggle_star, set_hotkey, get_config
└── detection/           # (reserved) Platform-specific terminal detection

src/                     # Svelte 5 frontend
├── app.css              # Tailwind theme tokens (dark-first overlay design)
├── app.html             # Shell HTML
├── lib/                 # Shared components, stores, types
│   ├── components/      # Svelte 5 components (SkillCard, Overlay, etc.)
│   ├── stores/          # Svelte runes stores ($state, $derived)
│   └── types/           # TypeScript interfaces matching Rust models
└── routes/
    └── +page.svelte     # Main overlay page
```

## Key Design Decisions

1. **Adapter pattern**: Each coding agent is an entry in `agents/registry.rs` with paths and format. Adding a new agent is 1 struct, not a new module.
2. **One base parser**: `parsers/frontmatter.rs` handles Markdown + YAML frontmatter. 90% of agents use this format with slight variations.
3. **Universal Skill model**: All agents' formats normalize into `models/skill.rs`. The frontend never sees agent-specific types.
4. **Overlay window**: Borderless, transparent, always-on-top, positioned at bottom-right. Shown/hidden by global hotkey.
5. **CWD detection**: Detects the focused terminal's working directory to show project-relevant skills. Platform-specific code in `commands/context.rs`.

## Adding a New Agent

1. Add a variant to `AgentId` enum in `models/skill.rs`
2. Add an entry to the `Vec` in `agents/registry.rs` with: display_name, paths (using $HOME/$PROJECT), format, brand color
3. If the format is truly novel, add a parser in `parsers/`. Otherwise, the existing `frontmatter.rs` handles it.
4. The scanner picks it up automatically. No other changes needed.

## Conventions

- Rust: snake_case, `thiserror` for typed errors, `anyhow` for ad-hoc errors
- TypeScript: camelCase, Svelte 5 runes (`$state`, `$derived`, `$effect`)
- Comments: `@agent-context:` prefix on module-level comments = context for LLM agents
- All structs that cross IPC boundary: `#[derive(Serialize, Deserialize)]` + `#[serde(rename_all = "camelCase")]`
- No `.unwrap()` in production code paths — use `?` or `.ok()`
- Test files co-located in the same file (`#[cfg(test)] mod tests`)

## Files to NOT modify without understanding impact

- `src-tauri/tauri.conf.json` — window config, capabilities, bundle settings
- `src-tauri/capabilities/default.json` — filesystem scope permissions
- `agents/registry.rs` — every agent's paths; incorrect paths = missed skills
