/**
 * @agent-context: Markdown rendering for chat bubbles.
 *
 * Reuses `renderSkillContent` (the existing battle-tested renderer with
 * highlight.js + escape-safe inline replacement) but skips frontmatter
 * detection (chat messages never have one) and adds chat-specific
 * decorations layered on top of the escaped markdown HTML:
 *
 *   - `artifact:xxxxx` IDs collapse to a small "id" hover badge — the user
 *     sees a circular `?` chip and the full id is in the title tooltip + a
 *     `data-id` attribute the UI can hook for click-to-copy / drill-in.
 *   - Install commands (`npx skills add ...`, `npm i`, `pip install`,
 *     `pnpm add`, `yarn add`, `cargo add`, `cargo install`, `brew install`,
 *     `gh repo create`, `git clone`) get wrapped in a copy block with a
 *     click-to-copy button.
 *   - Source-section labels (Local / Marketplace / Web) are tagged so each
 *     bullet group reads with a visual prefix and an accent color.
 *   - "(Not installed)" and "(Score: N)" trailing annotations remain muted
 *     and accent pill badges.
 *
 * The output HTML is safe to drop into a Svelte `{@html ...}` because all
 * inline replacement happens through escapeHtml first; we only add wrappers
 * around already-escaped substrings.
 */

import { renderSkillContent } from "./renderSkillContent";

// Match install / scaffold commands inside an already-rendered <code> span.
// Anchored on the command verb so we don't catch arbitrary code that happens
// to contain "install" in a sentence.
const INSTALL_CMD_RE =
  /<code class="skill-inline-code">((?:npx skills add|npm install|npm i\s|pnpm install|pnpm add|yarn add|pip install|pip3 install|cargo install|cargo add|brew install|gh repo (?:create|clone)|git clone)\s[^<]+)<\/code>/g;

/** Decorate the markdown HTML before it lands in the DOM. */
function decorate(html: string): string {
  let out = html;

  // 1. Bare artifact:xxxxxxxxxxxx FIRST so the inline-code path below doesn't
  //    leave `artifact:` text inside the title attribute we emit (the second
  //    regex would otherwise re-match its own output and shred the HTML).
  //    Title intentionally omits the "artifact:" prefix for the same reason.
  out = out.replace(
    /(^|[^>"a-z0-9_-])artifact:([a-f0-9]{6,32})(?![a-z0-9])/gi,
    '$1<span class="chat-artifact-badge" title="Canonical id $2" data-id="$2">?</span>',
  );
  // Artifact IDs inside `<code class="skill-inline-code">` → same compact
  // badge so they render identically to the bare form.
  out = out.replace(
    /<code class="skill-inline-code">artifact:([a-f0-9]{6,32})<\/code>/g,
    '<span class="chat-artifact-badge" title="Canonical id $1" data-id="$1">?</span>',
  );

  // 2. Install / scaffold commands → copy block.
  out = out.replace(
    INSTALL_CMD_RE,
    '<span class="chat-cmd"><code class="chat-cmd-code">$1</code><button type="button" class="chat-cmd-copy" data-copy="$1" aria-label="Copy command" title="Copy command">copy</button></span>',
  );

  // 3. Source-section labels. Match every phrasing the agent picks up across
  //    models — gpt-5.5 says "Marketplace skills:" and "Web result worth
  //    checking:", gpt-4o says "Skills.sh marketplace results:" and "Open web
  //    finds:", grok says "From the marketplace:". We catch all of them.
  out = out.replace(
    /(?:Local(?:ly installed| installed)? skills:|Local results:|Locally installed:|Installed skills:|Your local skills:|Local:)/g,
    '<span class="chat-source-tag chat-source-local">Local</span>',
  );
  out = out.replace(
    /(?:Skills\.sh marketplace(?: results)?:|Marketplace results:|Marketplace finds:|Marketplace skills:|From the marketplace:|ClawHub results:|Hub results:|Marketplace:)/g,
    '<span class="chat-source-tag chat-source-marketplace">Marketplace</span>',
  );
  out = out.replace(
    /(?:Open web (?:finds|results):|Web results:|Web finds:|From the open web:|Web result(?:s)?(?: worth checking)?:|On the web:|Web:)/g,
    '<span class="chat-source-tag chat-source-web">Web</span>',
  );

  // Inline provider hints like "— skills.sh" or "— clawhub" at the end of a
  // skill name → tiny muted tag so the eye sees provenance without parsing
  // the URL underneath. The em-dash version is what the model emits.
  out = out.replace(
    /\s—\s<code class="skill-inline-code">(skills\.sh|clawhub|claw-hub|clawHub)<\/code>/g,
    ' <span class="chat-source-mini chat-source-marketplace">$1</span>',
  );
  out = out.replace(
    /\s—\s(skills\.sh|clawhub|claw-hub|clawHub)(?=[\s.,)<]|$)/g,
    ' <span class="chat-source-mini chat-source-marketplace">$1</span>',
  );

  // 4. (Not installed) → muted pill (existing behavior).
  out = out.replace(
    /\(Not installed\)/g,
    '<span class="chat-pill chat-pill-muted">not installed</span>',
  );

  // 5. (Score: NN) or (Score: NN.N) → accent pill (existing behavior).
  out = out.replace(
    /\(Score:\s*([0-9]+(?:\.[0-9]+)?)\)/g,
    '<span class="chat-pill chat-pill-score">score $1</span>',
  );

  return out;
}

export interface RenderedChatMarkdown {
  html: string;
}

export function renderChatMarkdown(text: string): RenderedChatMarkdown {
  // Strip any leading [USER_REQUESTED_ITEM_COUNT: N] hint we injected so the
  // bubble doesn't echo it back at the user.
  const cleaned = text.replace(
    /\n*\[USER_REQUESTED_ITEM_COUNT:[^\]]+\][^.\n]*\.?/g,
    "",
  );

  const { html } = renderSkillContent(cleaned, {
    maxLines: Number.MAX_SAFE_INTEGER,
    filePath: null,
  });

  return { html: decorate(html) };
}
