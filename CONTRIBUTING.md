# Contributing

Thanks for contributing to Skill Deck.

## Development Setup

1. Install prerequisites:
- Node.js 22+
- pnpm 10+
- Rust stable

2. Install dependencies:

```bash
pnpm install
```

3. Run app in dev mode:

```bash
pnpm tauri dev
```

## Quality Gates

Before opening a pull request, run:

```bash
pnpm check
cd src-tauri && cargo test
cd src-tauri && cargo clippy -- -D warnings
```

## Pull Request Guidelines

Include in each PR:
- concise problem statement
- why the change is needed
- test and verification notes
- docs updates when user-facing behavior changes

If behavior changes, update:
- `CHANGELOG.md`
- relevant docs in README or planning docs

## Scope Guidance

High-impact areas require extra care:
- `src-tauri/src/commands/`
- `src-tauri/src/agents/registry.rs`
- `src-tauri/tauri.conf.json`
- `src-tauri/capabilities/default.json`

## Commit and Review Expectations

- Keep changes focused and reviewable
- Prefer additive and reversible changes
- Avoid unrelated refactors in feature PRs

## Code Style

- Rust: no `unwrap()` in production paths
- Frontend: keep Svelte runes patterns consistent
- Keep comments brief and useful for non-obvious logic
