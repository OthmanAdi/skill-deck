# Changelog

All notable changes to this project are documented in this file.

The format follows Keep a Changelog and this project uses Semantic Versioning.

## [Unreleased]

### Added
- Added structured release planning docs: `RELEASE_PLAN.md`, `CHANGELOG_PLAN.md`, and `FUTURE_IMPLEMENTATION_PLAN.md`.
- Added production readiness audit reference document: `PRODUCTION_READINESS_AUDIT.md`.
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

### Fixed
- Fixed tree focus index mismatch caused by multiple index increments per row.
- Fixed configuration persistence path to return and propagate write/serialize errors instead of silently ignoring failures.
- Fixed clippy-denied warnings in backend so strict lint passes.
- Fixed Ctrl+Shift+K activation reliability across shortcut string variants.
- Fixed auto-hide behavior that previously failed to hide overlay on focus loss in some flows.

### Security
- Removed unrestricted path-based file read behavior from skill content command path.
- Enabled CSP in app security config to improve WebView hardening.
