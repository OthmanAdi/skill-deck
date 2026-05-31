# Changelog

All notable changes to this project are documented in this file.

The format follows Keep a Changelog and this project uses Semantic Versioning.

## [Unreleased]

## [0.1.6] - 2026-05-30

### Added
- Project-scope scanning. `project_paths` now resolve against configured project roots and an auto-detected git working directory, so AGENTS.md, Amazon Q, JetBrains AI, and every project-scoped artifact can finally appear (previously the entire project registry was dead). New `set_project_roots` command.
- Press-to-capture global shortcut editor with live key chips that auto-saves on the main key.

### Changed
- Skill identity is keyed on the canonical file path instead of file content, so byte-identical but unrelated skills no longer false-merge and a global copy never collapses with a project copy. The prior content-based id is recorded as a legacy id so stars, icons, and overrides migrate.
- Scan patterns shared across agents (the `~/.agents/skills` store, declared by roughly 14 agents) are walked once per refresh instead of once per agent, and the redundant directory walk is skipped when the glob already matched.
- Per-agent `skillCount` now reports a real count instead of zero.
- README and package metadata now state the real agent count (56 coding agents plus the universal AGENTS.md format) rather than "15+".
- Removed the font-scale feature in favor of native window sizing.

### Fixed
- Keyboard focus is no longer reset by the 30 second background rescan, so arrow-key navigation survives a refresh.
- The lavender accent now uses an AA-contrast variant for small text on links, pills, and active labels.
- Corrected Replit, Amp, Kimi, and Trae CN detection against their real skill store paths.
- Agent-agnostic empty-state hint, AgentGroup `aria-controls`, ConfirmPopover focus management, and instant (non-janky) list scrolling.
- Canonical repository URL casing in package metadata.

### Security
- The public release workflow refuses to build or publish when premium (pro) source is present or the tag ends with `-pro`.

## [0.1.5] - 2026-05-20

### Added
- Marketplace registry. Search public skills across skills.sh and ClawHub behind one fan-out query, copy the `npx skills add` command, and browse via a rebuilt Registry tab with a provider selector.
- Archive history. Every update writes a content-hashed snapshot, with a side-by-side diff (markdown plus per-line syntax highlighting) and one-click restore.
- Claude Code plugin discovery, including plugin manifest hooks and `hooks.json` sidecars.
- Installed and updated timestamp pills, an archive indicator, and history actions on cards and rows.
- Body-portaled instant tooltips that survive overflow-hidden ancestors.

### Fixed
- GitHub sponsor and reserved site paths are ignored during repo detection so update checks stop returning 404.
- Reverted the skills.sh client to the open `/api/search` route used by `npx skills`.

## [0.1.4] - 2026-05-03

### Added
- Added structured release planning docs: `RELEASE_PLAN.md`, `CHANGELOG_PLAN.md`, and `FUTURE_IMPLEMENTATION_PLAN.md`.
- Added an internal production readiness audit (kept local, not shipped).
- Added support for scanning `customScanPaths` from config in backend scanner flow.
- Added repo override application in scan output pipeline.
- Added update-check provider guard for GitHub-only support path.
- Added configurable overlay interaction modes, pinned and auto-hide, persisted in app config.
- Added tray context controls to switch overlay mode directly from the tray menu.
- Added window capability permissions required for focus and always-on-top state control.
- Added skill discovery enrichment model, `discoveryTags`, `useCases`, and `discoveryHints` fields in scan output.
- Added deterministic discovery classifier and integrated it in scan pipeline for all parsed skills.
- Added intent-first FacetBar UI with use-case and tag filters.
- Added skill discovery documentation at `docs/skill-discovery.md`.
- Added on-demand Finder panel with keyboard entry points, `Ctrl+F` and `/`, plus a header toggle button.
- Added persisted Finder open-state preference in app config and IPC command surface.
- Added artifact-type classification in scan output, skill, command, hook, rule, workflow, prompt, config, and other.
- Added Claude settings hook extraction parser from `settings.json` and `settings.local.json`.
- Added artifact-type chips to FacetBar while keeping existing search and intent filters.
- Added slash and hook command aware copy resolution, slash command preferred for invocable artifacts, hook command preferred for hook entries.

### Changed
- Removed terminal context and terminal injection command surfaces from backend.
- Removed drag-and-drop injection and graph mode from frontend overlay flows.
- Updated scan flow to global-only discovery with short soft-cache behavior in UI refresh loop.
- Updated documentation to match current feature surface and architecture.
- Updated `read_skill_content` IPC command to resolve file content by `skillId` scoped to scanner results instead of direct arbitrary path input.
- Updated update-check logic to compare remote references against cached remote references instead of local file hash.
- Updated update checker HTTP client to include connect and request timeouts.
- Updated agent listing command to return meaningful `skillCount` values from scan data.
- Updated overlay keyboard navigation to avoid global Tab hijack and use visible option indices.
- Updated tree rendering to support filtered orphan roots and recursive-depth visual output flow via flattened visible order.
- Updated Tauri security config to enable production CSP and dedicated dev CSP.
- Updated settings dropdown to expose window behavior mode and active shortcut display.
- Updated overlay hotkey registration with fallback candidates and explicit failure notification.
- Updated overlay auto-hide handling with focus-change, window blur, and active focus guard paths.
- Updated overlay always-on-top state to follow selected interaction mode at startup and runtime.
- Updated generic markdown parser metadata extraction to include category, tags, use-cases, trigger, globs, and language where available.
- Updated UI terminology from tree view label to Card View.
- Updated overlay layout so search and intent filters open only on demand, while keeping existing filtering and search behavior intact.
- Updated grouped row and card metadata display to surface artifact type, slash command, and hook details.
- Updated docs and contributor guidance for command and hook coverage in open source workflows.

### Fixed
- Fixed tree focus index mismatch caused by multiple index increments per row.
- Fixed configuration persistence path to return and propagate write/serialize errors instead of silently ignoring failures.
- Fixed clippy-denied warnings in backend so strict lint passes.
- Fixed Ctrl+Shift+K activation reliability across shortcut string variants.
- Fixed auto-hide behavior that previously failed to hide overlay on focus loss in some flows.

### Security
- Removed unrestricted path-based file read behavior from skill content command path.
- Enabled CSP in app security config to improve WebView hardening.
