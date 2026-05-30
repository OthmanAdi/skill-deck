<!--
  Mirrors the structure CONTRIBUTING.md asks for. Keep it short and concrete.
-->

## Problem

<!-- What is broken or missing? One or two sentences. -->

## Fix

<!-- What this PR changes, and why this approach. -->

## Files

<!-- The notable files touched and what changed in each. -->

## Verification

<!-- How you confirmed it works. Tick what you ran. -->

- [ ] `cd src-tauri && cargo test`
- [ ] `cd src-tauri && cargo clippy -- -D warnings`
- [ ] `pnpm check`
- [ ] Ran the app (`pnpm tauri dev`) and exercised the change

## Docs

- [ ] Updated `README.md` / `docs/` if behavior or support changed
- [ ] Added a `CHANGELOG.md` entry under `[Unreleased]`

<!--
  New agent? It is one struct entry in registry.rs plus an AgentId variant.
  New parser? Add it under parsers/ and return a Result<Skill>.
-->
