/**
 * @agent-context: Line-aware diff for the archive Diff/View dialog.
 *
 * Implements a classic Hirschberg / Myers-style LCS over line arrays and emits
 * an ordered list of `{ kind: "equal" | "add" | "remove", left, right }` rows.
 * The UI renders this as a side-by-side pane.
 *
 * Why hand-rolled? We do not want to pull a diff dependency for one feature
 * and the files we diff are short (one skill SKILL.md), so an O(n*m) dynamic
 * program is comfortable. We cap at 4000 lines per side to keep the worst-case
 * memory bounded for accidental binary content.
 */

export type DiffKind = "equal" | "add" | "remove";

export interface DiffRow {
  kind: DiffKind;
  /** Original 1-indexed line in the LEFT (older) input, or null when added. */
  leftNumber: number | null;
  /** Original 1-indexed line in the RIGHT (newer) input, or null when removed. */
  rightNumber: number | null;
  /** Line text — at most one of left/right is meaningful per row. */
  left: string;
  right: string;
}

export interface DiffSummary {
  added: number;
  removed: number;
  unchanged: number;
  truncated: boolean;
}

export interface DiffResult {
  rows: DiffRow[];
  summary: DiffSummary;
}

const MAX_LINES_PER_SIDE = 4000;

function splitLines(input: string): string[] {
  if (!input) return [];
  // Preserve final empty line if the file ends without a newline so the diff
  // does not invent a phantom "removed" row at the bottom.
  const lines = input.split(/\r?\n/);
  return lines;
}

export function diffLines(leftInput: string, rightInput: string): DiffResult {
  const leftAll = splitLines(leftInput ?? "");
  const rightAll = splitLines(rightInput ?? "");

  const truncated =
    leftAll.length > MAX_LINES_PER_SIDE || rightAll.length > MAX_LINES_PER_SIDE;
  const left = truncated ? leftAll.slice(0, MAX_LINES_PER_SIDE) : leftAll;
  const right = truncated ? rightAll.slice(0, MAX_LINES_PER_SIDE) : rightAll;

  const n = left.length;
  const m = right.length;

  // LCS length matrix.
  const lcs: Uint32Array = new Uint32Array((n + 1) * (m + 1));
  const w = m + 1;

  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      if (left[i] === right[j]) {
        lcs[i * w + j] = lcs[(i + 1) * w + (j + 1)] + 1;
      } else {
        const a = lcs[(i + 1) * w + j];
        const b = lcs[i * w + (j + 1)];
        lcs[i * w + j] = a > b ? a : b;
      }
    }
  }

  const rows: DiffRow[] = [];
  let added = 0;
  let removed = 0;
  let unchanged = 0;

  let i = 0;
  let j = 0;

  while (i < n && j < m) {
    if (left[i] === right[j]) {
      rows.push({
        kind: "equal",
        leftNumber: i + 1,
        rightNumber: j + 1,
        left: left[i],
        right: right[j],
      });
      unchanged += 1;
      i += 1;
      j += 1;
    } else if (lcs[(i + 1) * w + j] >= lcs[i * w + (j + 1)]) {
      rows.push({
        kind: "remove",
        leftNumber: i + 1,
        rightNumber: null,
        left: left[i],
        right: "",
      });
      removed += 1;
      i += 1;
    } else {
      rows.push({
        kind: "add",
        leftNumber: null,
        rightNumber: j + 1,
        left: "",
        right: right[j],
      });
      added += 1;
      j += 1;
    }
  }

  while (i < n) {
    rows.push({
      kind: "remove",
      leftNumber: i + 1,
      rightNumber: null,
      left: left[i],
      right: "",
    });
    removed += 1;
    i += 1;
  }

  while (j < m) {
    rows.push({
      kind: "add",
      leftNumber: null,
      rightNumber: j + 1,
      left: "",
      right: right[j],
    });
    added += 1;
    j += 1;
  }

  return {
    rows,
    summary: { added, removed, unchanged, truncated },
  };
}
