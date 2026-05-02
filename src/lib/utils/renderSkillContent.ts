/**
 * @agent-context: Lightweight skill content renderer for read-only display.
 * No runtime parser dependency, safe HTML escaping, supports YAML frontmatter,
 * headings, lists, quotes, rules, fenced code blocks, and plain paragraphs.
 */

export interface RenderedSkillContent {
  html: string;
  truncated: boolean;
  hiddenLineCount: number;
  totalLineCount: number;
}

const DEFAULT_MAX_LINES = 260;

function escapeHtml(input: string): string {
  return input
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function sanitizeHref(rawHref: string): string {
  const href = rawHref.trim();
  if (/^(https?:|mailto:|file:|\/|#)/i.test(href)) {
    return href.replace(/"/g, "%22");
  }
  return "#";
}

function renderInline(input: string): string {
  const codeTokens: string[] = [];
  let output = escapeHtml(input).replace(/`([^`]+)`/g, (_match, code: string) => {
    const token = `@@CODE${codeTokens.length}@@`;
    codeTokens.push(`<code class="skill-inline-code">${code}</code>`);
    return token;
  });

  output = output.replace(/\[([^\]]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g, (_match, text: string, href: string) => {
    const safeHref = sanitizeHref(href);
    return `<a class="skill-md-link" href="${safeHref}" target="_blank" rel="noopener noreferrer">${text}</a>`;
  });

  output = output.replace(/\*\*([^*]+)\*\*/g, '<strong class="skill-strong">$1</strong>');
  output = output.replace(/__([^_]+)__/g, '<strong class="skill-strong">$1</strong>');
  output = output.replace(/\*([^*]+)\*/g, '<em class="skill-em">$1</em>');
  output = output.replace(/_([^_]+)_/g, '<em class="skill-em">$1</em>');
  output = output.replace(/~~([^~]+)~~/g, '<s class="skill-strike">$1</s>');

  return output.replace(/@@CODE(\d+)@@/g, (_match, i: string) => codeTokens[Number(i)] ?? "");
}

function indentStyle(line: string): string {
  const leadingSpaces = line.match(/^\s*/)?.[0].length ?? 0;
  const px = Math.min(leadingSpaces * 2, 28);
  return px > 0 ? ` style="padding-left:${px}px"` : "";
}

export function renderSkillContent(content: string, maxLines = DEFAULT_MAX_LINES): RenderedSkillContent {
  const normalized = content.replace(/\r\n?/g, "\n");
  const allLines = normalized.split("\n");
  const totalLineCount = allLines.length;
  const hasLeadingFrontmatter = allLines[0]?.trim() === "---";

  const visibleLines = allLines.slice(0, maxLines);
  const truncated = totalLineCount > visibleLines.length;
  const hiddenLineCount = Math.max(0, totalLineCount - visibleLines.length);

  const html: string[] = [];
  let inFrontmatter = false;
  let frontmatterClosed = false;
  let inCodeFence = false;

  for (let i = 0; i < visibleLines.length; i++) {
    const line = visibleLines[i];
    const trimmed = line.trim();

    if (!inCodeFence && trimmed === "---" && hasLeadingFrontmatter && !frontmatterClosed) {
      if (i === 0) {
        inFrontmatter = true;
        html.push('<div class="skill-line skill-fm-delimiter">---</div>');
        continue;
      }

      if (inFrontmatter) {
        inFrontmatter = false;
        frontmatterClosed = true;
        html.push('<div class="skill-line skill-fm-delimiter">---</div>');
        continue;
      }
    }

    const codeFenceMatch = trimmed.match(/^```(.*)$/);
    if (codeFenceMatch) {
      inCodeFence = !inCodeFence;
      const language = codeFenceMatch[1].trim();
      html.push(
        `<div class="skill-line skill-code-fence">\`\`\`${escapeHtml(language)}</div>`
      );
      continue;
    }

    if (inCodeFence) {
      const escaped = escapeHtml(line);
      html.push(`<div class="skill-line skill-code-line">${escaped || "&nbsp;"}</div>`);
      continue;
    }

    if (inFrontmatter && line.includes(":")) {
      const colonIdx = line.indexOf(":");
      const key = line.slice(0, colonIdx).trim();
      const value = line.slice(colonIdx + 1).trim();
      html.push(
        `<div class="skill-line skill-frontmatter-row"><span class="skill-fm-key">${escapeHtml(key)}</span><span class="skill-fm-delimiter">:</span><span class="skill-fm-value">${renderInline(value)}</span></div>`
      );
      continue;
    }

    if (trimmed.length === 0) {
      html.push('<div class="skill-line skill-empty"></div>');
      continue;
    }

    if (/^(-{3,}|_{3,}|\*{3,})$/.test(trimmed)) {
      html.push('<div class="skill-line skill-rule"></div>');
      continue;
    }

    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      const level = heading[1].length;
      html.push(
        `<div class="skill-line skill-heading skill-heading-${level}">${renderInline(heading[2])}</div>`
      );
      continue;
    }

    const quote = line.match(/^\s*>\s?(.*)$/);
    if (quote) {
      html.push(`<div class="skill-line skill-quote">${renderInline(quote[1])}</div>`);
      continue;
    }

    const unordered = line.match(/^(\s*)([-*])\s+(.*)$/);
    if (unordered) {
      html.push(
        `<div class="skill-line skill-list-item"${indentStyle(line)}><span class="skill-bullet-symbol">${unordered[2]}</span><span>${renderInline(unordered[3])}</span></div>`
      );
      continue;
    }

    const ordered = line.match(/^(\s*)(\d+)\.\s+(.*)$/);
    if (ordered) {
      html.push(
        `<div class="skill-line skill-list-item"${indentStyle(line)}><span class="skill-ordered-index">${ordered[2]}.</span><span>${renderInline(ordered[3])}</span></div>`
      );
      continue;
    }

    html.push(`<div class="skill-line skill-paragraph"${indentStyle(line)}>${renderInline(line)}</div>`);
  }

  return {
    html: html.join("\n"),
    truncated,
    hiddenLineCount,
    totalLineCount,
  };
}
