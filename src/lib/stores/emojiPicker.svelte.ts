/**
 * @agent-context: Single source of truth for the emoji picker.
 *
 * Previously each SkillCard owned its own EmojiPickerPopover, which meant
 * clicking icon A then icon B left BOTH pickers open. This store enforces a
 * singleton — only one skill is "open" at a time, and a second requestOpen()
 * closes the first before re-pointing at the new anchor.
 *
 * The popover itself is rendered ONCE at the Overlay root so the
 * positioning math only needs to live in one place.
 */

import type { Skill } from "$lib/types";

class EmojiPickerStore {
  open = $state(false);
  skill = $state<Skill | null>(null);
  anchorRect = $state<DOMRect | null>(null);
}

export const emojiPicker = new EmojiPickerStore();

/**
 * Request opening the emoji picker for `skill`, anchored at `triggerRect`.
 * If a picker is already open for a different skill, it is closed first.
 * If it is already open for the SAME skill (user re-clicked), this toggles
 * it shut.
 */
export function requestOpenEmojiPicker(skill: Skill, triggerRect: DOMRect): void {
  if (emojiPicker.open && emojiPicker.skill?.id === skill.id) {
    closeEmojiPicker();
    return;
  }
  emojiPicker.skill = skill;
  emojiPicker.anchorRect = triggerRect;
  emojiPicker.open = true;
}

export function closeEmojiPicker(): void {
  emojiPicker.open = false;
  emojiPicker.skill = null;
  emojiPicker.anchorRect = null;
}
