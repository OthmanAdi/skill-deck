# Contributing to Skill Deck

Thank you for helping improve Skill Deck. Focused bug fixes, agent adapters, parser coverage, accessibility work, documentation, and reproducible performance improvements are welcome.

By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md). Report security vulnerabilities through the private process in [SECURITY.md](SECURITY.md), not a public issue.

## Development setup

Skill Deck 0.3.0 is currently qualified on Windows x64. Source-level fixes for other platforms are welcome, but do not describe a platform as supported without native validation evidence.

Prerequisites:

- Node.js 22.23.1
- Corepack and the pnpm version pinned in `package.json`
- Rust stable
- Tauri v2 platform prerequisites

```powershell
corepack enable
pnpm install --frozen-lockfile
pnpm tauri dev
```

## Before changing code

- Read `AGENTS.md` and `CLAUDE.md` for architecture and repository rules.
- Keep the universal artifact model aligned across Rust IPC types and frontend TypeScript types.
- Prefer the existing adapter and parser pipeline over agent-specific frontend behavior.
- Do not include real tokens, private prompts, personal paths, or production endpoints in fixtures.
- Do not weaken filesystem scope, CSP, provider credential masking, download verification, or release signing gates.

## Quality gates

Run the smallest focused tests while iterating. Before opening a pull request, run the complete source gate:

```powershell
pnpm check
pnpm compliance:test
node --test scripts/release-compliance/tests/test_run_tauri_remapped.mjs
Push-Location src-tauri
cargo test --locked
cargo clippy --locked -- -D warnings
Pop-Location
```

For a release-affecting change, also run `pnpm tauri build` on Windows x64 and document the result. A local unsigned build is validation evidence, not a distributable project release.

To review the exact public-source boundary without changing the repository, export into a new empty directory and let the script compare every destination file and hash against its allowlist:

```powershell
pwsh -NoProfile -File scripts/public-export/Export-PublicSource.ps1 `
  -Destination C:\path\to\empty\skill-deck-public
```

The export refuses nested or non-empty destinations and rejects private/process directories, generated binaries, archives, PDBs, unexpected files, changed hashes, and known private repository references. Update the manifest deliberately when adding a public top-level path.

This allowlisted export is a review surface for source, documentation, legal, and compliance material. It intentionally omits the vendored llama.cpp `.exe` and `.dll` payloads, so it is not evidence that the managed local-model runtime can be rebuilt or redistributed from the exported directory alone.

## Pull requests

Each pull request should include:

- The problem and why it matters.
- A focused description of the change.
- Commands run and their results.
- User-visible screenshots when the interface changes.
- Documentation and changelog updates when behavior changes.
- Remaining platform, security, or compatibility risk.

Keep changes reviewable and avoid unrelated refactors. Never add `Co-Authored-By` trailers on commits in this repository.

## Contribution license

By submitting a contribution, you agree that it may be distributed under the project's [Mozilla Public License 2.0](LICENSE). Do not submit code, text, media, or model material unless you have the right to contribute it under those terms. Third-party material must retain its original license and attribution.

## Common contribution paths

### Add an agent

1. Add the identifier in `src-tauri/src/models/skill.rs`.
2. Add documented global/project paths and format metadata in `src-tauri/src/agents/registry.rs`.
3. Reuse the frontmatter parser unless the source format is genuinely different.
4. Add scanner and parser tests.
5. Update `docs/skill-discovery.md`.

### Change an IPC type

Update the Rust model and its frontend TypeScript mirror together. IPC structs use `Serialize`, `Deserialize`, and camel-case serialization.

### Change the interface

Use Svelte 5 runes. Preserve keyboard-first search, facets, visible focus, grouped/card navigation, the collapsible Finder, and readable compact layout.

## Release boundary

Maintainers create releases from `v*` tags. The release workflow requires configured signing, verifies every Authenticode signature, and has no unsigned publication path. Contributors should never commit generated installers, portable executables, PDBs, `target/`, `build/`, or `release-evidence/`.

## Review and conduct

Maintainers may request narrower scope, additional tests, or native platform proof. Review code and ideas directly, keep discussion factual, and give actionable feedback.
