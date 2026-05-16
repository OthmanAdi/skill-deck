/**
 * @agent-context: Lightweight skill content renderer for read-only display.
 * No runtime parser dependency, safe HTML escaping, supports YAML frontmatter,
 * headings, lists, quotes, rules, fenced code blocks, and plain paragraphs.
 *
 * When the source `filePath` points to a non-markdown artifact (the JSON
 * files plugins use for hooks, raw shell scripts, Python utilities, etc.),
 * we skip markdown parsing entirely and render the file as a single
 * highlighted code block. That gives hooks proper JSON syntax highlighting
 * instead of dumping them as flat lines.
 */

import { highlightCode, languageFromPath } from "./highlight";

export interface RenderedSkillContent {
  html: string;
  truncated: boolean;
  hiddenLineCount: number;
  totalLineCount: number;
}

export interface RenderSkillOptions {
  maxLines?: number;
  /**
   * Source file path. Used to pick a syntax-highlighting language when the
   * file is not markdown (e.g. `.json` for hook manifests, `.ps1` for hook
   * scripts, `.py` for hook handlers).
   */
  filePath?: string | null;
}

const DEFAULT_MAX_LINES = 260;
const MARKDOWN_EXTENSIONS = new Set(["md", "mdx", "markdown"]);

function extensionOf(filePath: string | null | undefined): string | null {
  if (!filePath) return null;
  const name = filePath.split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return null;
  return name.slice(dot + 1).toLowerCase();
}

function normalizeOptions(options: RenderSkillOptions | number | undefined): RenderSkillOptions {
  if (typeof options === "number") {
    return { maxLines: options };
  }
  return options ?? {};
}

function renderHighlightedFile(
  content: string,
  maxLines: number,
  filePath: string,
): RenderedSkillContent {
  const normalized = content.replace(/\r\n?/g, "\n");
  const allLines = normalized.split("\n");
  const totalLineCount = allLines.length;
  const visibleLines = allLines.slice(0, maxLines);
  const truncated = totalLineCount > visibleLines.length;
  const hiddenLineCount = Math.max(0, totalLineCount - visibleLines.length);

  const language = languageFromPath(filePath) ?? "plaintext";
  const { html: highlighted } = highlightCode(visibleLines.join("\n"), language);

  // Wrap in our own `<pre>` so the modal scroll/typography styles still apply
  // and the highlight.js classes inherit from the existing CSS variables.
  const html = `<pre class="skill-code-block hljs" data-language="${language}"><code>${highlighted}</code></pre>`;

  return { html, truncated, hiddenLineCount, totalLineCount };
}

function escapeHtml(input: string): string {
  return input
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function sanitizeHref(rawHref: string): string {
  const href = rawHref.trim();
  if (href.startsWith("#")) {
    return href;
  }

  if (href.startsWith("/")) {
    return href.replace(/"/g, "%22");
  }

  if (/^(?:\.|\.\.)\//.test(href)) {
    return href.replace(/"/g, "%22");
  }

  if (!/^[a-z][a-z0-9+.-]*:/i.test(href)) {
    return href.replace(/"/g, "%22");
  }

  try {
    const parsed = new URL(href);
    if (parsed.protocol === "http:" || parsed.protocol === "https:" || parsed.protocol === "mailto:") {
      return parsed.toString().replace(/"/g, "%22");
    }
  } catch {
    // fall through to "#"
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

export function renderSkillContent(
  content: string,
  options?: RenderSkillOptions | number,
): RenderedSkillContent {
  const opts = normalizeOptions(options);
  const maxLines = opts.maxLines ?? DEFAULT_MAX_LINES;
  const ext = extensionOf(opts.filePath);

  // For non-markdown files (hook JSON manifests, raw scripts, configs),
  // skip markdown parsing entirely and ship a single highlighted block.
  if (ext && !MARKDOWN_EXTENSIONS.has(ext)) {
    return renderHighlightedFile(content, maxLines, opts.filePath!);
  }

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
  let codeFenceLanguage = "";
  let codeFenceBuffer: string[] = [];

  function flushCodeFence() {
    if (codeFenceBuffer.length === 0) {
      return;
    }
    const { html: highlighted, language } = highlightCode(
      codeFenceBuffer.join("\n"),
      codeFenceLanguage,
    );
    html.push(
      `<pre class="skill-code-block hljs" data-language="${language}"><code>${highlighted}</code></pre>`,
    );
    codeFenceBuffer = [];
  }

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
      if (inCodeFence) {
        // Closing fence — flush the highlighted block before continuing.
        flushCodeFence();
        inCodeFence = false;
        codeFenceLanguage = "";
      } else {
        inCodeFence = true;
        codeFenceLanguage = codeFenceMatch[1].trim();
      }
      continue;
    }

    if (inCodeFence) {
      codeFenceBuffer.push(line);
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

  // If the file ended inside an open code fence, flush whatever we collected
  // so the highlighted block still appears instead of disappearing silently.
  if (inCodeFence) {
    flushCodeFence();
  }

  return {
    html: html.join("\n"),
    truncated,
    hiddenLineCount,
    totalLineCount,
  };
}
