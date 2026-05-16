/**
 * @agent-context: Syntax highlighting wrapper around highlight.js.
 *
 * Picks a language from either an explicit code-fence hint ("```bash") or
 * from the source file's extension. Falls back to plaintext so an unknown
 * extension never throws and never produces unsafe HTML. The library escapes
 * every input internally, so the returned HTML is always safe to inject via
 * {@html ...} in Svelte.
 *
 * Languages are imported individually instead of pulling
 * `highlight.js/lib/common`, so the bundle only carries what coding-agent
 * artifacts actually use (JSON, shell, PowerShell, Python, JS/TS, YAML).
 */

import HighlightJS from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import diff from "highlight.js/lib/languages/diff";
import ini from "highlight.js/lib/languages/ini";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import markdown from "highlight.js/lib/languages/markdown";
import plaintext from "highlight.js/lib/languages/plaintext";
import powershell from "highlight.js/lib/languages/powershell";
import python from "highlight.js/lib/languages/python";
import shell from "highlight.js/lib/languages/shell";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

const REGISTRATIONS: [string, Parameters<typeof HighlightJS.registerLanguage>[1]][] = [
  ["bash", bash],
  ["diff", diff],
  ["ini", ini],
  ["javascript", javascript],
  ["json", json],
  ["markdown", markdown],
  ["plaintext", plaintext],
  ["powershell", powershell],
  ["python", python],
  ["shell", shell],
  ["typescript", typescript],
  ["xml", xml],
  ["yaml", yaml],
];

let registered = false;
function ensureRegistered() {
  if (registered) return;
  for (const [name, language] of REGISTRATIONS) {
    HighlightJS.registerLanguage(name, language);
  }
  registered = true;
}

const EXT_TO_LANGUAGE: Record<string, string> = {
  json: "json",
  yaml: "yaml",
  yml: "yaml",
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  ps1: "powershell",
  psm1: "powershell",
  py: "python",
  pyi: "python",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  ts: "typescript",
  tsx: "typescript",
  toml: "ini",
  ini: "ini",
  cfg: "ini",
  md: "markdown",
  mdx: "markdown",
  markdown: "markdown",
  xml: "xml",
  html: "xml",
  htm: "xml",
  diff: "diff",
  patch: "diff",
  txt: "plaintext",
};

const FENCE_ALIASES: Record<string, string> = {
  // Code-fence hints people actually type. Maps to the registered name above.
  sh: "bash",
  shell: "bash",
  zsh: "bash",
  bash: "bash",
  cmd: "powershell",
  pwsh: "powershell",
  ps: "powershell",
  powershell: "powershell",
  py: "python",
  python: "python",
  js: "javascript",
  javascript: "javascript",
  ts: "typescript",
  typescript: "typescript",
  jsonc: "json",
  yml: "yaml",
  yaml: "yaml",
  toml: "ini",
  ini: "ini",
  md: "markdown",
  markdown: "markdown",
  xml: "xml",
  html: "xml",
  diff: "diff",
  patch: "diff",
  text: "plaintext",
  plain: "plaintext",
  txt: "plaintext",
  "": "plaintext",
};

const KNOWN_LANGUAGES = new Set([
  "bash",
  "diff",
  "ini",
  "javascript",
  "json",
  "markdown",
  "plaintext",
  "powershell",
  "python",
  "shell",
  "typescript",
  "xml",
  "yaml",
]);

function normalizeLanguage(hint: string | undefined): string {
  if (!hint) return "plaintext";
  const lower = hint.trim().toLowerCase();
  if (FENCE_ALIASES[lower]) return FENCE_ALIASES[lower];
  if (KNOWN_LANGUAGES.has(lower)) return lower;
  return "plaintext";
}

export function languageFromPath(filePath: string | undefined | null): string | null {
  if (!filePath) return null;
  const cleaned = filePath.split(/[\\/]/).pop() ?? "";
  // Handle conventional dotfiles like `.bashrc`, `Dockerfile`, etc. before
  // falling back to the extension lookup.
  const lowerName = cleaned.toLowerCase();
  if (lowerName === "dockerfile") return "bash";
  if (lowerName.startsWith(".bash") || lowerName.startsWith(".zsh")) return "bash";

  const dot = cleaned.lastIndexOf(".");
  if (dot <= 0) return null;
  const ext = cleaned.slice(dot + 1).toLowerCase();
  return EXT_TO_LANGUAGE[ext] ?? null;
}

export interface HighlightResult {
  /** Sanitized HTML safe for `{@html}` injection. */
  html: string;
  /** Resolved highlight.js language id, useful for debugging / data-attrs. */
  language: string;
}

/**
 * Highlight a raw code string. `hint` is an explicit language id (from a
 * code-fence label or file extension); if it is missing or unknown, returns
 * the escaped source unchanged so the caller still gets HTML-safe output.
 */
export function highlightCode(code: string, hint: string | undefined): HighlightResult {
  ensureRegistered();
  const language = normalizeLanguage(hint);
  try {
    const result = HighlightJS.highlight(code, { language, ignoreIllegals: true });
    return { html: result.value, language };
  } catch {
    return { html: escapeHtml(code), language: "plaintext" };
  }
}

function escapeHtml(input: string): string {
  return input
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}
