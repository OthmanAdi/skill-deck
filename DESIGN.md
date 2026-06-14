---
version: alpha
name: Skill Deck
description: >-
  The design system for Skill Deck, a keyboard-first command-palette overlay
  that unifies skills across 56 AI coding agents. Monochrome dark surfaces, a
  single teal/cyan command signal, and a physical-keycap motif. Inherits the
  company skeleton (Geist type, 4px grid, expo-out motion, Vercel-grade
  restraint); differentiated by the teal signal and developer-tool density.
colors:
  primary: "#5eead4"
  secondary: "#22d3ee"
  on-primary: "#042b25"
  canvas: "#06080d"
  surface: "#0c111b"
  elevated: "#121826"
  border: "#232b39"
  selection: "#0e2724"
  ink: "#eef2f6"
  ink-muted: "#9aa7b4"
  success: "#4ade80"
  danger: "#fb7185"
typography:
  display:
    fontFamily: Geist
    fontSize: 3.5rem
    fontWeight: 800
    lineHeight: 1.02
    letterSpacing: -0.04em
  h1:
    fontFamily: Geist
    fontSize: 2.5rem
    fontWeight: 800
    lineHeight: 1.05
    letterSpacing: -0.035em
  h2:
    fontFamily: Geist
    fontSize: 1.75rem
    fontWeight: 700
    lineHeight: 1.12
    letterSpacing: -0.03em
  h3:
    fontFamily: Geist
    fontSize: 1.125rem
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: -0.01em
  body:
    fontFamily: Geist
    fontSize: 1rem
    fontWeight: 400
    lineHeight: 1.6
    letterSpacing: 0em
  body-sm:
    fontFamily: Geist
    fontSize: 0.875rem
    fontWeight: 400
    lineHeight: 1.55
    letterSpacing: 0em
  label:
    fontFamily: Geist Mono
    fontSize: 0.75rem
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: 0.08em
    fontFeature: "calt"
  mono:
    fontFamily: Geist Mono
    fontSize: 0.8125rem
    fontWeight: 500
    lineHeight: 1.5
    letterSpacing: 0em
    fontFeature: "tnum"
  keycap:
    fontFamily: Geist Mono
    fontSize: 0.75rem
    fontWeight: 600
    lineHeight: 1
    letterSpacing: 0em
rounded:
  sm: 6px
  md: 10px
  lg: 14px
  xl: 20px
  full: 9999px
spacing:
  xs: 4px
  sm: 8px
  md: 16px
  lg: 24px
  xl: 32px
  2xl: 48px
  3xl: 64px
components:
  palette:
    backgroundColor: "{colors.elevated}"
    textColor: "{colors.ink}"
    rounded: "{rounded.xl}"
    padding: 8px
  palette-input:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    typography: "{typography.mono}"
    rounded: "{rounded.md}"
    padding: 16px
  row:
    backgroundColor: "{colors.elevated}"
    textColor: "{colors.ink-muted}"
    typography: "{typography.body-sm}"
    rounded: "{rounded.md}"
    padding: 10px
  row-selected:
    backgroundColor: "{colors.selection}"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
    padding: 10px
  keycap:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    typography: "{typography.keycap}"
    rounded: "{rounded.sm}"
    padding: 4px
  tag-agent:
    backgroundColor: "{colors.elevated}"
    textColor: "{colors.primary}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: 4px
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.on-primary}"
    rounded: "{rounded.md}"
    padding: 12px
  button-primary-hover:
    backgroundColor: "{colors.secondary}"
    textColor: "{colors.on-primary}"
    rounded: "{rounded.md}"
    padding: 12px
  divider:
    backgroundColor: "{colors.border}"
    height: 1px
  badge-success:
    backgroundColor: "{colors.success}"
    textColor: "{colors.on-primary}"
    typography: "{typography.label}"
    rounded: "{rounded.full}"
    padding: 4px
  badge-danger:
    backgroundColor: "{colors.danger}"
    textColor: "{colors.canvas}"
    typography: "{typography.label}"
    rounded: "{rounded.full}"
    padding: 4px
---

## Overview

Skill Deck is an **operator's deck for the keyboard.** It is a floating command
palette that drops over any window on a global hotkey, lists every skill,
command, hook, and rule across 56 AI coding agents, and gets out of the way the
instant you fire one. The design exists to make that loop feel **instant,
dense, and quiet** — a precision instrument, not a brochure.

The system inherits the **company skeleton** shared across our products: the
Geist type family, a 4px spacing grid, expo-out motion, and Vercel-grade
restraint (high contrast, generous negative space, no decoration that data can
already carry). Skill Deck's own identity is two things layered on top:

1. **A single teal/cyan command signal.** Everything is monochrome graphite
   except one rationed accent (`primary` → `secondary`) that marks the one
   thing you can act on right now. Color is a verb, never a garnish.
2. **The keycap motif.** Keyboard keys are first-class objects — rendered as
   physical, bottom-weighted caps in mono type. The product is operated by
   keystroke, so the interface is built out of keys.

The aesthetic reference class is the command-palette launcher (Raycast's
surface-ladder darkness) crossed with the engineering-native calm of Linear:
a near-black canvas, hairline structure, and one status-light accent. No light
theme competes for the brand; the dark surface *is* the product.

### Personality

Terse. Fast. Built for someone who already knows the shortcut. Copy is
lowercase and matter-of-fact. Motion is sub-200ms and snaps like a key
bottoming out. If an element does not earn its pixels at 460×640, it is cut.

## Colors

The palette is a **graphite ladder with one teal signal.** Hue is spent only
where the user acts.

- **Primary `#5eead4` (teal):** The command signal. Primary buttons, the
  selected-row marker, agent tags, the search caret. If something is teal, it
  is the single most actionable element on screen.
- **Secondary `#22d3ee` (cyan):** The signal's hover/energy state. Pairs with
  `primary` in the `135deg` brand gradient used on the principal call to
  action and the wordmark. Never used as a second competing accent.
- **On-primary `#042b25`:** Deep teal-black ink that rides on top of the teal
  and cyan fills so call-to-action text stays legible (10:1+).
- **Canvas `#06080d`:** The near-black base. The desktop behind the overlay,
  the page behind the marketing site.
- **Surface `#0c111b`:** One notch up. Search field, keycaps, inset wells.
- **Elevated `#121826`:** The palette body and skill rows float here. The
  brightest structural surface.
- **Border `#232b39`:** Hairline structure. 1px dividers and card edges. Depth
  comes from this line and the surface ladder, not from heavy shadow.
- **Selection `#0e2724`:** A dark teal wash behind the highlighted row — the
  signal at 8% strength so the keyboard cursor reads without shouting.
- **Ink `#eef2f6` / Ink-muted `#9aa7b4`:** Primary and secondary text. Names in
  `ink`, descriptions and metadata in `ink-muted` (7.4:1 on `elevated`).
- **Success `#4ade80` / Danger `#fb7185`:** Semantic only — "installed / up to
  date" and "destructive / out of date / overwrite warning." Reserved for state,
  never for emphasis.

**Rationing rule:** at most one teal primary action per view. Everything else
is graphite. Scarcity is what gives the signal its meaning.

## Typography

Two families, deliberately paired:

- **Geist** (sans) — display, headings, body. The Vercel-native grotesk that
  anchors the company skeleton. Tight negative tracking on display sizes
  (`-0.04em`) for an engineered, condensed feel.
- **Geist Mono** — labels, metadata, code, and **keycaps**. Mono is the voice
  of the machine: agent ids, slash commands, file paths, shortcut keys, and the
  live search query are all mono. The eye learns that *mono = a literal thing
  the system knows.*

The ramp runs `display → h1 → h2 → h3 → body → body-sm → label → mono → keycap`.
`label` is uppercase mono with `0.08em` tracking for section eyebrows. `mono`
carries `tnum` for aligned counts ("4 / 312 skills"). Keycaps are the smallest,
heaviest mono — short, centered, unmissable.

Set the body at `1rem`/`1.6`; never below `0.875rem` for reading text. Headlines
may use the `primary→secondary` gradient as a text fill, but only once per view.

## Layout

A **4px grid** governs everything; spacing steps are `xs 4 · sm 8 · md 16 ·
lg 24 · xl 32 · 2xl 48 · 3xl 64`.

The product canvas is the **460×640 overlay** — a tall, dense column. Design for
that constraint first: single column, list-led, no element wider than it needs
to be. The marketing site echoes it: a centered `1080px` measure, a sticky
blur nav, and the command palette reproduced full-fidelity as the hero object
(the UI *is* the screenshot).

A faint **56px dither grid** sits behind the canvas, masked to fade at the
edges — texture you feel before you see. Two soft radial teal glows (top-center
and bottom-right, 8–12% alpha) lift the near-black without introducing a second
hue. Alignment is left-led for content, centered only for hero and CTA blocks.

## Elevation & Depth

Depth is built from the **surface ladder, not drop shadows.**
`canvas → surface → elevated` is the entire stack; each step up the ladder is a
step toward the user. A row that "lifts" on hover does so by climbing from
`elevated` toward `surface`, plus a 1px `border` edge — not by casting a shadow.

Two exceptions earn real shadow:

- **The floating palette** casts one deep, soft shadow (`0 30px 80px` at ~60%
  black) plus a 1px teal-tinted ring, because it genuinely floats above the OS.
- **The focus glow:** the active element gets a `2px` teal focus ring
  (`primary` at ~45% alpha). This is the one place the signal appears as light.
  It is the only acceptable substitute for an outline and must always be visible
  for keyboard users.

## Shapes

Radii are tight and consistent: `sm 6 · md 10 · lg 14 · xl 20 · full`.

- **Palette container:** `xl` (20px) — the one generous curve, because it is the
  hero shape.
- **Cards, rows, buttons, inputs:** `md`–`lg` (10–14px).
- **Keycaps:** `sm` (6px) with a **bottom-weighted border** (`border-bottom`
  thicker than the other sides) so the cap reads as a physical, pressable key.
- **Badges/pills:** `full`.

Borders are always **1px hairlines** in `border`. Curvature and a single
hairline do the structural work that fills and shadows would otherwise clutter.

## Components

Tokens for the recurring surfaces. Variants (hover, selected) are separate
entries keyed by name.

- **palette** — the overlay shell. `elevated` body, `ink` text, `xl` radius, the
  one element allowed a real float shadow + teal ring.
- **palette-input** — the live search field. `surface` well, `mono` query text,
  a blinking `primary` caret. The query is always mono.
- **row** / **row-selected** — a skill entry. Default sits on `elevated` with
  `ink-muted` metadata; the keyboard-selected row washes to `selection` with
  full `ink` and a teal left-marker.
- **keycap** — a physical key. `surface` fill, `keycap` mono type, `sm` radius,
  bottom-weighted border. Used inline ("⌘ ⇧ K") and in the footer hint row.
- **tag-agent** — the source-agent chip (claude / cursor / codex …). `label`
  mono in `primary` on `elevated`, brand-colored per agent at runtime.
- **button-primary** / **button-primary-hover** — the one CTA. Teal fill with
  `on-primary` ink; on hover it shifts toward `secondary` and lifts `-2px`.
  Only one primary button may exist per view.
- **divider** — a 1px `border` hairline. Structure, not decoration.
- **badge-success** / **badge-danger** — semantic state pills (installed /
  update available). Carried by `success` and `danger`, never reused for
  emphasis.

## Do's and Don'ts

**Do**

- Ration the accent: one teal primary action per view; graphite for the rest.
- Render keys as keycaps in mono. Make the keyboard visible — it is the product.
- Use mono for anything literal the system knows (ids, paths, commands, counts).
- Build depth from the surface ladder and 1px hairlines first.
- Keep motion fast (`120–220ms`) and eased `cubic-bezier(.16,1,.3,1)` (expo-out)
  so interactions snap like a key bottoming out.
- Always show a visible teal focus ring; design for keyboard traversal first.
- Honor `prefers-reduced-motion`: disable transforms, blinking carets, and
  glows; keep all functionality and contrast intact.

**Don't**

- Don't introduce a second accent hue or a light theme — the dark surface is the
  brand.
- Don't use teal for decoration, large fills, or more than one action at a time.
- Don't lean on drop shadows for ordinary elevation; let the ladder do it.
- Don't drop reading text below `0.875rem` or put `ink-muted` on `canvas` for
  long copy.
- Don't decorate what the data already says. If a count, tag, or path conveys
  it, no illustration is needed.
- Don't let any element exceed what it needs at 460×640; density is a feature.

## Agent Prompt Guide

When generating or editing any Skill Deck surface (the overlay app or the
marketing site), an agent should:

1. **Start from canvas `#06080d`** and build up the `surface → elevated` ladder.
   Reach for shadow only for the floating palette and the focus ring.
2. **Find the one action.** Make exactly that element teal
   (`button-primary`, gradient `135deg primary→secondary`); render everything
   else in graphite + `ink`/`ink-muted`.
3. **Type literally in mono.** Any agent id, slash command, file path, keyboard
   shortcut, or live count uses Geist Mono (`mono` / `label` / `keycap`). Prose
   and headings use Geist.
4. **Render shortcuts as keycaps** (`keycap` token: `surface` fill, `sm` radius,
   bottom-weighted border), never as plain text.
5. **Respect the grid:** 4px spacing steps, `md`–`lg` radii on cards, 1px
   `border` hairlines, `1080px` max measure on the web.
6. **Animate sparingly:** expo-out, ≤220ms, transform/opacity only, with a
   `prefers-reduced-motion` off-switch.
7. **Validate before shipping:** every color must map to a token, every
   `background/text` pair must clear WCAG AA (4.5:1), and the canonical section
   order in this file must hold. Run `npx @google/design.md lint DESIGN.md`.

The north star for every decision: *does this make the keyboard-first loop
faster and quieter?* If not, cut it.
