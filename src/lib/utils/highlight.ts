/**
 * @agent-context: Syntax highlighter for skill file content preview.
 * Applies minimal color coding to YAML frontmatter keys/values and markdown headings/bullets.
 * Returns HTML-escaped string with <span> wrappers for color classes.
 * Used by SkillRow and SkillCard expanded view.
 */

/**
 * Highlight a single line of skill file content.
 * Returns an HTML string (already escaped).
 */
export function highlightLine(line: string, idx: number, allLines: string[]): string {
  const escaped = line
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

  // Frontmatter delimiter
  if (escaped.trim() === "---") {
    return `<span class="fm-delimiter">${escaped}</span>`;
  }

  // Detect if we're inside frontmatter (between first and second ---)
  let delimCount = 0;
  for (let i = 0; i <= idx; i++) {
    if (allLines[i].trim() === "---") delimCount++;
  }
  const inFrontmatter = delimCount === 1;

  if (inFrontmatter && escaped.includes(":")) {
    const colonIdx = escaped.indexOf(":");
    const key = escaped.substring(0, colonIdx);
    const value = escaped.substring(colonIdx + 1);
    return `<span class="fm-key">${key}</span><span class="fm-delimiter">:</span><span class="fm-value">${value}</span>`;
  }

  // Markdown headings
  if (/^#{1,6}\s/.test(escaped)) {
    return `<span class="md-heading">${escaped}</span>`;
  }

  // Markdown bullets
  const bulletMatch = escaped.match(/^(\s*)([-*])(\s.*)$/);
  if (bulletMatch) {
    return `${bulletMatch[1]}<span class="md-bullet">${bulletMatch[2]}</span>${bulletMatch[3]}`;
  }

  return escaped;
}
