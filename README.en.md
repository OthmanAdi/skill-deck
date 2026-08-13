# Skill Deck

**A searchable catalog for the skills, rules, prompts, and providers used by AI coding agents.**

[Deutsche Hauptfassung](README.md)

<p align="center">
  <img src="media/banner-v2.png" alt="Skill Deck catalog and local control surface for AI coding agents" width="888">
</p>

[![CI](https://github.com/OthmanAdi/skill-deck/actions/workflows/ci.yml/badge.svg)](https://github.com/OthmanAdi/skill-deck/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-0.3.0-6d5dfc)](CHANGELOG.md)
[![Platform](https://img.shields.io/badge/qualified-Windows%20x64-1677ff)](#platform-support)
[![License](https://img.shields.io/badge/license-MPL--2.0-2f855a)](LICENSE)

AI coding agents rely on skills, rules, commands, hooks, and system prompts. In companies and larger development environments, it is often unclear where those artifacts live, what they can do, which instructions they contain, and which providers or local model paths execute them.

Skill Deck makes that inventory visible. The Windows desktop application discovers documented agent artifacts, normalizes them into a local catalog, and makes their contents, settings, and providers searchable. This supports inventory and manual review, but it is not a security audit and does not automatically detect prompt injection.

Version 0.3.0 is one fully unlocked edition. It has no Free/Pro split, pricing tier, product activation, license-key check, or paid feature gate.

## Why Skill Deck?

- **Agent artifacts are scattered:** Claude Code, Codex, Cursor, Copilot, Windsurf, Gemini CLI, and other tools use different files and directories.
- **Origin and capabilities are hard to trace:** Without a shared catalog, teams struggle to see which skills are installed, where they came from, and which commands or hooks they expose.
- **Prompt-injection risk requires visibility:** Skill Deck displays local instructions for review. It does not automatically rate or certify their security.
- **Providers and models become difficult to track:** The app exposes configured providers, the managed local model, and relevant settings in one place.
- **Local-first workflow:** Search, filters, favorites, snapshots, history, and local harness logs stay on the machine. Intentional network access is documented below.

## Features

- Scans the documented global and project locations for 56 coding agents, plus the universal `AGENTS.md` format.
- Normalizes skills, rules, commands, hooks, prompts, workflows, and configuration into one artifact model.
- Provides live search, facets, grouped and card views, favorites, update checks, and snapshot history.
- Searches public skills on skills.sh and ClawHub.
- Runs skill-aware chat sessions with context compaction, tools, and local session records.
- Supports the managed local Jan-nano 4B model, a local Ollama endpoint, or a user-configured Azure-compatible provider.
- Opens from a configurable global shortcut and supports tray, pinned, and auto-hide modes.

## Screenshots

<table>
  <tr>
    <td><img src="media/screenshots/skill-deck-searchable-catalog.png" alt="Skill Deck searchable catalog of AI coding agent skills and rules"></td>
    <td><img src="media/screenshots/skill-deck-agent-settings-unlocked.png" alt="Skill Deck fully unlocked agent, provider, and local model settings"></td>
  </tr>
  <tr>
    <td align="center"><strong>Searchable skill catalog</strong></td>
    <td align="center"><strong>Agent, provider, and model settings</strong></td>
  </tr>
</table>

The captures show Skill Deck 0.3.0 on Windows x64 with synthetic demonstration artifacts. Capture details are recorded in `media/screenshots/CAPTURE_RECEIPT.md`.

## Platform support

The current qualified target is **Windows 10/11 x64**. The source retains macOS and Linux adapters, but version 0.3.0 does not claim qualified native packages for macOS, Linux, Windows ARM64, or app stores.

A public binary is only released when a signed artifact appears on the GitHub Releases page. Local developer builds are unsigned. The tag workflow cannot create a GitHub Release unless Authenticode configuration is present and every Windows signature is valid.

## Build from source on Windows

### Prerequisites

- Windows 10 or 11 on x64
- [Node.js 22.23.1](https://nodejs.org/)
- Corepack with pnpm 10.13.1, pinned in `package.json`
- [Rust stable](https://rustup.rs/)
- [Tauri's Windows prerequisites](https://v2.tauri.app/start/prerequisites/), including Microsoft C++ Build Tools and WebView2

### Install and run

```powershell
corepack enable
pnpm install --frozen-lockfile
pnpm tauri dev
```

Press `Ctrl+Shift+K` to show or hide the overlay. The shortcut can be changed in Settings.

### Validate and build

```powershell
pnpm check
pnpm compliance:test
Push-Location src-tauri
cargo test --locked
cargo clippy --locked -- -D warnings
Pop-Location
pnpm tauri build
```

Windows build output is written below `src-tauri/target/release/`. A local build is not an official release and is not signed by the project.

## Supported agents and formats

| Ecosystem | Examples of discovered artifacts |
|---|---|
| Claude Code | `SKILL.md`, slash commands, agents, plugins, settings hooks |
| Codex | `SKILL.md`, `AGENTS.md` |
| Cursor | skills, `.mdc` rules, `.cursorrules` |
| GitHub Copilot | `.prompt.md`, `.instructions.md`, `copilot-instructions.md` |
| Windsurf | skills, rules, `.windsurfrules` |
| Gemini CLI | `SKILL.md`, `GEMINI.md` |
| Cline, Roo Code, Continue | skills and agent-specific rule files |
| Aider, Amazon Q, JetBrains AI, Tabnine, Augment | documented global and project artifact stores |
| OpenCode, Crush, Amp, Antigravity, Goose, Kilo, Qwen, Kimi, Droid, Warp, Zencoder, and others | skills under agent stores and the shared `~/.agents/skills` location |
| Universal | `AGENTS.md` |

The full discovery contract is documented in [docs/skill-discovery.md](docs/skill-discovery.md).

## Privacy and network access

Discovery, parsing, filters, favorites, snapshots, chat history, and harness telemetry are processed and stored locally. Skill Deck does not operate its own product telemetry service and does not upload discovered artifacts by default.

Some features intentionally access the network:

- Marketplace search queries skills.sh and ClawHub.
- Skill update checks query the configured GitHub source.
- Web search and web fetch contact the selected search provider or requested page when those tools are used.
- A remote AI provider receives the conversation and selected context when that provider is used. Provider credentials are stored locally and returned to the interface in masked form.
- The managed Jan-nano model is downloaded from a pinned Hugging Face revision on first use, verified with SHA-256, and then served locally through the bundled Windows x64 llama.cpp runtime.

Review provider, web-tool, and model settings before using the harness with sensitive material. See [SECURITY.md](SECURITY.md) for private vulnerability reporting.

## Architecture

```text
documented agent paths
        |
        v
Rust registry + scanners -> parsers -> universal Skill model
        |                                  |
        |                                  v
        +--------------------------> Svelte 5 overlay
                                           |
                                           v
                              local preferences and history
```

| Area | Responsibility |
|---|---|
| `src-tauri/src/agents/` | Agent registry, documented locations, and filesystem scanning |
| `src-tauri/src/parsers/` | Markdown, frontmatter, and settings-hook parsing |
| `src-tauri/src/models/` | Universal IPC data structures |
| `src-tauri/src/detection/` | Discovery metadata, repositories, marketplaces, updates, and snapshots |
| `src-tauri/src/harness/` | Providers, tools, context management, model lifecycle, and local telemetry |
| `src/lib/` | Svelte 5 components, stores, types, and rendering |
| `src/routes/` | Desktop overlay entry surface |

The frontend does not branch on agent-specific source formats. Every adapter produces the same universal model before data crosses the Tauri IPC boundary.

## Add an agent

1. Add the agent identifier to `src-tauri/src/models/skill.rs`.
2. Add documented paths and format metadata to `src-tauri/src/agents/registry.rs`.
3. Add a parser only when the format cannot use the existing Markdown/frontmatter pipeline.
4. Add focused scanner and parser tests and update `docs/skill-discovery.md`.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the complete development and pull request process.

## Releases and integrity

Windows releases are triggered by `v*` tags. The workflow validates source, requires configured Azure Artifact Signing values, signs the portable application before bundling, signs both installers, and then verifies every Authenticode signature. Only after that does it generate checksums and release evidence and create a GitHub Release. There is no unsigned official-binary fallback.

Release evidence includes a CycloneDX SBOM, declared dependency-license inventory, checksums, runtime and notice receipts, and checks for unwanted build paths. This technical evidence does not replace legal review and is not a security certification.

## Third-party and model notices

Skill Deck source code is licensed under the Mozilla Public License 2.0 (`MPL-2.0`). Bundled and downloaded components retain their own licenses and attribution:

- Active third-party notices: `src-tauri/resources/THIRD_PARTY_NOTICES.txt`
- Component license texts: `src-tauri/resources/licenses/`
- Local Windows x64 runtime: pinned llama.cpp build under the MIT license
- Jan-nano 4B Q4_K_M: downloaded on demand from a pinned revision; model-card metadata declares Apache-2.0. Revision, size, SHA-256, attribution, and license text are recorded beside the runtime notices.
- The notice corpus under `release/legal-corpus/` supports compliance review and is not legal advice.

The project license applies to source code published by Skill Deck. It does not replace or modify any third-party, runtime, or model license.

## Author

Skill Deck is developed and published in Berlin by **Ahmad Othman Ammar Adi**, a software developer of Syrian heritage focused on understandable, local tools for practical work with AI coding agents. Full attribution is recorded in [AUTHORS.md](AUTHORS.md).

## Contributing

Bug reports, focused fixes, documentation, new agent adapters, and parser tests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md) before participating.

Do not file security issues publicly. Use the private process in [SECURITY.md](SECURITY.md).

## License

Skill Deck is released under the [Mozilla Public License 2.0](LICENSE). Copyright (c) 2026 Ahmad Othman Ammar Adi. Third-party and model licenses remain unaffected.
