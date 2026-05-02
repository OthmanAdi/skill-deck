# Theme Redesign Trace

Date: 2026-05-02
Source design: `DESIGN.md` (imported from `C:\Users\oasrvadmin\Downloads\DESIGN-linear.app.md`)

## Goal

Unify the app visual system around the imported design tokens while implementing exactly three theme choices:

- System
- Dark
- Light

And remove Obsidian naming from UX and docs.

## Scope Boundaries

- In scope: theme tokens, theme mode selection, theme persistence compatibility, micro animations, cross-platform-safe color syntax, related docs.
- Out of scope: unrelated functional rewrites, feature additions not directly tied to theming or motion.

## Findings Before Changes

1. Theme mode names are `obsidian` and `obsidian-light` in frontend state and CSS selectors.
2. Config defaults to `dark` in Rust, while frontend currently expects obsidian IDs.
3. Theme menu has no system mode.
4. Styling uses `oklch()` and `color-mix()` heavily, which may have uneven WebView support depending on OS/browser engine versions.
5. Animation behavior is split across CSS keyframes and Svelte transitions, but no shared duration/easing tokens.

## File Map, Planned Modifications

- `src/lib/stores/theme.svelte.ts`
  - Replace theme IDs with `system`, `dark`, `light`.
  - Add migration mapping from legacy values (`obsidian`, `obsidian-light`).
  - Resolve system mode from `prefers-color-scheme` and react to system changes.

- `src/app.css`
  - Replace theme selectors with `dark` and `light` resolved theme names.
  - Rebase tokens to imported design palette.
  - Replace risky color expressions (`oklch`, `color-mix`) with broadly supported hex/rgba.
  - Tune animation timings/easing and add shared motion variables.

- `src/lib/components/ThemeMenu.svelte`
  - Show System, Dark, Light choices.
  - Keep persistence behavior through `setTheme()`.

- `src/lib/components/Overlay.svelte`
  - Remove Obsidian wording from comments.
  - Keep drag-region behavior intact.
  - Ensure logo/icon colors are token-based.

- `src/lib/components/AgentBadge.svelte`
  - Remove `color-mix` usage.

- `src/lib/components/AgentGroup.svelte`
  - Remove `color-mix` usage.

- `src/lib/types/index.ts`
  - Replace `AGENT_COLORS` values with hex values.

- `src-tauri/src/models/config.rs`
  - Update theme documentation and default to `system`.

- `src-tauri/src/commands/preferences.rs`
  - Update command docs to reflect supported theme values.

- `README.md`
  - Update feature wording from Obsidian-specific names to System/Dark/Light.

## Risks and Edge Cases to Validate

1. Existing users with saved `obsidian` values should be migrated without breakage.
2. System mode should update when OS theme changes while app is running.
3. No hardcoded dark-only colors in components that violate light mode readability.
4. No regression in drag-and-drop/drag-region interactions while animation tuning is applied.

## Applied Changes

1. Imported external spec as canonical design source:
   - Added `DESIGN.md` from the provided file.

2. Rebuilt theme architecture to System, Dark, Light:
   - `src/lib/stores/theme.svelte.ts`
   - `src-tauri/src/models/config.rs`
   - `src-tauri/src/commands/preferences.rs`
   - `src/lib/components/ThemeMenu.svelte`

3. Migrated legacy saved theme values:
   - `obsidian` -> `dark`
   - `obsidian-light` -> `light`
   - unknown values -> `system`

4. Replaced Obsidian naming in user-visible docs and comments:
   - `README.md`
   - `src/lib/components/Overlay.svelte`

5. Replaced unstable color syntax with broad compatibility colors:
   - Removed runtime `oklch()` and `color-mix()` usage from frontend CSS/components.
   - Switched runtime tokens to hex and rgba in `src/app.css`.
   - Updated agent color maps to hex in frontend and registry.

6. Motion polish and micro interaction tuning:
   - Unified duration and easing tokens in `src/app.css`.
   - Added `card-enter` animation and adjusted delays.
   - Tightened tab, toast, and overlay transition timings.
   - Added reduced-motion safety branch.

7. Fixed grouped list delayed row visibility:
   - Root cause: row and card animation delays were based on global list index, so agents after large groups (for example Codex and Gemini after hundreds of Claude entries) were delayed by several seconds.
   - Fix: cap and localize entry delays.
   - Files:
     - `src/lib/components/SkillRow.svelte`
     - `src/lib/components/AgentGroup.svelte`
     - `src/lib/components/SkillCard.svelte`

8. Located and fixed path readability issue in detail blocks:
   - Root cause: full file path text used low-contrast muted styling and extra opacity.
   - Fix: switch to stronger semantic text color and high opacity for path rows.
   - Files:
     - `src/lib/components/SkillRow.svelte`
     - `src/lib/components/SkillCard.svelte`

9. Upgraded skill content rendering pipeline:
   - Replaced line-by-line simple highlighter with a lightweight read-only renderer.
   - Supports frontmatter rows, headings, bullet and numbered lists, quotes, rules, fenced code blocks, inline code, and truncation metadata.
   - Files:
     - `src/lib/utils/renderSkillContent.ts`
     - `src/lib/components/SkillRow.svelte`
     - `src/lib/components/SkillCard.svelte`
     - `src/app.css`
   - Removed legacy renderer:
     - `src/lib/utils/highlight.ts`

## Verification Results

Successful checks:

- `pnpm check` passed, 0 errors, 0 warnings.
- `cargo test` passed, 51 tests.
- `cargo clippy -- -D warnings` passed.

Cross-platform compile checks from this Windows host:

- Installed targets:
  - `x86_64-unknown-linux-gnu`
  - `x86_64-apple-darwin`
- `cargo check --target x86_64-unknown-linux-gnu` reached dependency build and failed on missing OpenSSL cross-compilation sysroot/tooling.
- `cargo check --target x86_64-apple-darwin` reached dependency build and failed on missing C cross-compiler (`cc`) for Darwin target.

Conclusion:

- Code level changes compile and test cleanly on Windows.
- Linux and macOS target checks are blocked by host toolchain dependencies, not by Rust source errors in touched files.
