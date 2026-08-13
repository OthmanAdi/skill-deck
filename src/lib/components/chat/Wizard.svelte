<!--
  @agent-context: Skill Library Wizard. Deterministic Svelte UI that
  turns a product/team scenario into a structured harness prompt via
  buildSkillLibraryPrompt() — the prompt output contract is unchanged; this
  file is presentation only. PHOSPHOR stepper redesign: four steps (Scenario /
  Capture / Policy / Result) shown ONE at a time behind a segmented progress
  control with a Back/Next footer. Fields start EMPTY with placeholders; the
  mode cards are explicit example seeds (clicking one loads an editable
  template). The Result step shows a compact human summary + Copy/Generate,
  with the raw prompt behind a collapsed disclosure instead of a <pre> dump.
-->
<script lang="ts">
  import { harnessStore, sendMessage } from "$lib/stores/harness.svelte";
  import {
    DOMAIN_OPTIONS,
    SOURCE_OPTIONS,
    TARGET_AGENTS,
    WIZARD_MODES,
    buildSkillLibraryPrompt,
    createWizardTemplate,
    labelsFor,
    toggleSelection,
    type TargetAgent,
    type WizardMode,
  } from "$lib/utils/skillLibraryWizard";

  const STEPS = [
    { key: "scenario", label: "Scenario" },
    { key: "capture", label: "Capture" },
    { key: "policy", label: "Policy" },
    { key: "result", label: "Result" },
  ] as const;

  let step = $state(0);
  let promptOpen = $state(false);

  // Brief fields start EMPTY — the mode cards seed an editable example.
  let mode = $state<WizardMode>("product");
  let title = $state("");
  let scenario = $state("");
  let repeatedWork = $state("");
  let inconsistentOutput = $state("");
  let successCriteria = $state("");
  let constraints = $state("");
  let selectedDomains = $state<Record<string, boolean>>({});
  let selectedSources = $state<Record<string, boolean>>({});
  let targetAgent = $state<TargetAgent>("Claude Code");
  let skillCount = $state(8);
  let includeLocal = $state(true);
  let includeMarketplace = $state(true);
  let includeWeb = $state(true);
  let includeGovernance = $state(true);
  let includeDrafts = $state(true);
  let copyState = $state<"idle" | "copied" | "failed">("idle");

  const providerReady = $derived(Boolean(harnessStore.activeProvider && harnessStore.activeModel));
  const selectedDomainLabels = $derived(labelsFor(DOMAIN_OPTIONS, selectedDomains));
  const selectedSourceLabels = $derived(labelsFor(SOURCE_OPTIONS, selectedSources));
  const generatedPrompt = $derived.by(() => buildPrompt());

  const modeLabel = $derived(WIZARD_MODES.find((m) => m.key === mode)?.label ?? mode);
  const selectedDomainList = $derived(DOMAIN_OPTIONS.filter((o) => selectedDomains[o.key]));
  const selectedSourceList = $derived(SOURCE_OPTIONS.filter((o) => selectedSources[o.key]));
  const policySummary = $derived([
    { label: "Local skills", on: includeLocal },
    { label: "Registries", on: includeMarketplace },
    { label: "Web/news", on: includeWeb },
    { label: "Drafts", on: includeDrafts },
    { label: "Governance", on: includeGovernance },
  ]);

  function loadTemplate(kind: WizardMode) {
    mode = kind;
    const template = createWizardTemplate(kind);
    title = template.title;
    scenario = template.scenario;
    repeatedWork = template.repeatedWork;
    inconsistentOutput = template.inconsistentOutput;
    successCriteria = template.successCriteria;
    selectedDomains = template.selectedDomains;
  }

  function buildPrompt(): string {
    return buildSkillLibraryPrompt({
      mode,
      title,
      targetAgent,
      selectedDomainLabels,
      selectedSourceLabels,
      scenario,
      repeatedWork,
      inconsistentOutput,
      successCriteria,
      constraints,
      skillCount,
      includeLocal,
      includeMarketplace,
      includeWeb,
      includeDrafts,
      includeGovernance,
    });
  }

  async function generatePlan() {
    if (!providerReady || harnessStore.isStreaming) return;
    harnessStore.view = "chat";
    await sendMessage(generatedPrompt);
  }

  async function copyPrompt() {
    try {
      await navigator.clipboard?.writeText(generatedPrompt);
      copyState = "copied";
      window.setTimeout(() => (copyState = "idle"), 1400);
    } catch {
      copyState = "failed";
      window.setTimeout(() => (copyState = "idle"), 1400);
    }
  }

  function back() {
    step = Math.max(0, step - 1);
  }
  function next() {
    step = Math.min(STEPS.length - 1, step + 1);
  }
</script>

<div class="wizard selectable">
  <!-- One-line intro: identity + harness status, no hero. -->
  <header class="intro">
    <div class="intro-copy">
      <h3>Skill Library Architect</h3>
      <p>Company know-how in, governed skill stack out.</p>
    </div>
    {#if providerReady}
      <span class="badge-meta" title="{harnessStore.activeProvider} · {harnessStore.activeModel}">
        {harnessStore.activeModel}
      </span>
    {:else}
      <span class="badge-signal is-warn">provider needed</span>
    {/if}
  </header>

  <!-- Stepper: segmented progress control, one step visible at a time. -->
  <nav class="segmented stepper" aria-label="Wizard steps">
    {#each STEPS as s, i (s.key)}
      <button
        class="seg"
        class:active={step === i}
        class:done={step > i}
        aria-current={step === i ? "step" : undefined}
        onclick={() => (step = i)}
      >
        <span class="seg-num">{i + 1}</span>{s.label}
      </button>
    {/each}
  </nav>

  {#if step === 0}
    <section class="step-panel" aria-label="Scenario">
      <div class="step-head">
        <h4 class="step-title">Scenario</h4>
        <p class="step-help">Define the work the company keeps re-explaining to AI.</p>
      </div>

      <div class="mode-row" role="group" aria-label="Wizard mode">
        {#each WIZARD_MODES as item (item.key)}
          <button
            class="mode-card"
            class:active={mode === item.key}
            aria-pressed={mode === item.key}
            onclick={() => loadTemplate(item.key)}
          >
            <span class="mode-eyebrow">{item.eyebrow}</span>
            <strong>{item.label}</strong>
            <small>Load example brief</small>
          </button>
        {/each}
      </div>
      <p class="seed-hint">Picking a mode seeds an editable example. Everything below is yours to change.</p>

      <label>Library title
        <input bind:value={title} placeholder="e.g. Support escalation stack" />
      </label>
      <label>Scenario
        <textarea bind:value={scenario} rows="3" placeholder="What is the company trying to ship or run?"></textarea>
      </label>
      <label>Repeated work
        <textarea bind:value={repeatedWork} rows="2" placeholder="What does the team keep re-explaining to AI tools?"></textarea>
      </label>
      <label>Inconsistent output
        <textarea bind:value={inconsistentOutput} rows="2" placeholder="Where do agent outputs go generic or wrong?"></textarea>
      </label>
    </section>
  {:else if step === 1}
    <section class="step-panel" aria-label="Capture map">
      <div class="step-head">
        <h4 class="step-title">Capture map</h4>
        <p class="step-help">Where the method is hiding, and what success looks like.</p>
      </div>

      <div class="chip-group" role="group" aria-label="Domains">
        <span class="group-label">Domains</span>
        {#each DOMAIN_OPTIONS as option (option.key)}
          <button
            class="chip"
            class:active={selectedDomains[option.key]}
            aria-pressed={Boolean(selectedDomains[option.key])}
            title={option.hint}
            onclick={() => (selectedDomains = toggleSelection(selectedDomains, option.key))}
          >{option.label}</button>
        {/each}
      </div>

      <div class="chip-group" role="group" aria-label="Hidden sources">
        <span class="group-label">Hidden sources</span>
        {#each SOURCE_OPTIONS as option (option.key)}
          <button
            class="chip"
            class:active={selectedSources[option.key]}
            aria-pressed={Boolean(selectedSources[option.key])}
            title={option.hint}
            onclick={() => (selectedSources = toggleSelection(selectedSources, option.key))}
          >{option.label}</button>
        {/each}
      </div>

      <div class="pair-grid">
        <label>Target agent
          <select bind:value={targetAgent}>
            {#each TARGET_AGENTS as agent (agent)}
              <option value={agent}>{agent}</option>
            {/each}
          </select>
        </label>
        <label>Skill count
          <input type="number" min="3" max="20" bind:value={skillCount} />
        </label>
      </div>

      <label>Success criteria
        <textarea bind:value={successCriteria} rows="2" placeholder="What does a working skill stack change?"></textarea>
      </label>
      <label>Constraints
        <textarea bind:value={constraints} rows="2" placeholder="Privacy, review, or tooling rules the stack must respect"></textarea>
      </label>
    </section>
  {:else if step === 2}
    <section class="step-panel" aria-label="Research policy">
      <div class="step-head">
        <h4 class="step-title">Research policy</h4>
        <p class="step-help">What the agent may search before composing the stack.</p>
      </div>

      <div class="check-list" role="group" aria-label="Research toggles">
        <button class="check-row" class:active={includeLocal} aria-pressed={includeLocal} onclick={() => (includeLocal = !includeLocal)}>
          <span class="check-box" aria-hidden="true">
            {#if includeLocal}<svg width="10" height="10" viewBox="0 0 16 16" fill="none"><path d="M3 8.5l3.2 3.2L13 5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
          </span>
          <span class="check-copy"><strong>Local skills</strong><span>Search installed artifacts first</span></span>
        </button>
        <button class="check-row" class:active={includeMarketplace} aria-pressed={includeMarketplace} onclick={() => (includeMarketplace = !includeMarketplace)}>
          <span class="check-box" aria-hidden="true">
            {#if includeMarketplace}<svg width="10" height="10" viewBox="0 0 16 16" fill="none"><path d="M3 8.5l3.2 3.2L13 5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
          </span>
          <span class="check-copy"><strong>Registries</strong><span>skills.sh and ClawHub candidates</span></span>
        </button>
        <button class="check-row" class:active={includeWeb} aria-pressed={includeWeb} onclick={() => (includeWeb = !includeWeb)}>
          <span class="check-box" aria-hidden="true">
            {#if includeWeb}<svg width="10" height="10" viewBox="0 0 16 16" fill="none"><path d="M3 8.5l3.2 3.2L13 5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
          </span>
          <span class="check-copy"><strong>Web/news</strong><span>Fresh skill-library context</span></span>
        </button>
        <button class="check-row" class:active={includeDrafts} aria-pressed={includeDrafts} onclick={() => (includeDrafts = !includeDrafts)}>
          <span class="check-box" aria-hidden="true">
            {#if includeDrafts}<svg width="10" height="10" viewBox="0 0 16 16" fill="none"><path d="M3 8.5l3.2 3.2L13 5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
          </span>
          <span class="check-copy"><strong>Drafts</strong><span>3 compact SKILL.md drafts</span></span>
        </button>
        <button class="check-row" class:active={includeGovernance} aria-pressed={includeGovernance} onclick={() => (includeGovernance = !includeGovernance)}>
          <span class="check-box" aria-hidden="true">
            {#if includeGovernance}<svg width="10" height="10" viewBox="0 0 16 16" fill="none"><path d="M3 8.5l3.2 3.2L13 5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
          </span>
          <span class="check-copy"><strong>Governance</strong><span>Owner, eval, INDEX scan</span></span>
        </button>
      </div>
    </section>
  {:else}
    <section class="step-panel" aria-label="Result">
      <div class="step-head">
        <h4 class="step-title">Review &amp; generate</h4>
        <p class="step-help">A compact read of the brief the agent will receive.</p>
      </div>

      <dl class="summary">
        <div class="sum-row">
          <dt>Mode</dt>
          <dd>{modeLabel}</dd>
        </div>
        <div class="sum-row">
          <dt>Title</dt>
          <dd>{title.trim() || "Untitled library"}</dd>
        </div>
        <div class="sum-row">
          <dt>Target agent</dt>
          <dd>{targetAgent}</dd>
        </div>
        <div class="sum-row">
          <dt>Skills</dt>
          <dd class="tabular">{skillCount}</dd>
        </div>
        <div class="sum-row">
          <dt>Domains</dt>
          <dd class="chips">
            {#if selectedDomainList.length === 0}
              <span class="none">None selected</span>
            {:else}
              {#each selectedDomainList as o (o.key)}<span class="badge-meta">{o.label}</span>{/each}
            {/if}
          </dd>
        </div>
        <div class="sum-row">
          <dt>Sources</dt>
          <dd class="chips">
            {#if selectedSourceList.length === 0}
              <span class="none">None selected</span>
            {:else}
              {#each selectedSourceList as o (o.key)}<span class="badge-meta">{o.label}</span>{/each}
            {/if}
          </dd>
        </div>
        <div class="sum-row">
          <dt>Policy</dt>
          <dd class="policy">
            {#each policySummary as item (item.label)}
              <span class="pol" class:on={item.on}>
                <span class="pol-mark" aria-hidden="true">{item.on ? "✓" : "–"}</span>{item.label}
              </span>
            {/each}
          </dd>
        </div>
      </dl>

      <div class="prompt-box">
        <button class="disclosure" aria-expanded={promptOpen} onclick={() => (promptOpen = !promptOpen)}>
          <span class="chevron" class:open={promptOpen} aria-hidden="true">
            <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
              <path d="M6 4l4 4-4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
            </svg>
          </span>
          <span class="disclosure-label">View generated prompt</span>
          <span class="disclosure-hint">sent verbatim to the agent</span>
        </button>
        {#if promptOpen}
          <pre class="prompt-pre">{generatedPrompt}</pre>
        {/if}
      </div>
    </section>
  {/if}

  <footer class="step-footer">
    <button class="btn-secondary" disabled={step === 0} onclick={back}>Back</button>
    <span class="step-count">Step {step + 1} of {STEPS.length}</span>
    {#if step < STEPS.length - 1}
      <button class="btn-primary" onclick={next}>Next</button>
    {:else}
      <div class="result-actions">
        <button class="btn-secondary" onclick={copyPrompt}>
          {copyState === "copied" ? "Copied" : copyState === "failed" ? "Copy failed" : "Copy prompt"}
        </button>
        {#if providerReady}
          <button class="btn-primary" disabled={harnessStore.isStreaming} onclick={() => void generatePlan()}>
            {harnessStore.isStreaming ? "Agent running" : "Generate plan"}
          </button>
        {:else}
          <button class="btn-primary" onclick={() => (harnessStore.view = "settings")}>Configure provider</button>
        {/if}
      </div>
    {/if}
  </footer>
</div>

<style>
  .wizard {
    flex: 1;
    overflow-y: auto;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    color: var(--color-text-primary);
    font-family: var(--font-ui);
    background: var(--color-surface-0);
  }

  /* === One-line intro ==================================================== */
  .intro {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .intro-copy {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }

  .intro h3 {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--text-lg);
    line-height: var(--text-lg--line-height);
    font-weight: 600;
    letter-spacing: -0.01em;
    white-space: nowrap;
  }

  .intro p {
    margin: 0;
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    line-height: var(--text-xs--line-height);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* === Stepper (segmented control, DESIGN 8 — same spec as ChatShell) ==== */
  .segmented {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
  }

  .stepper {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
  }

  .seg {
    appearance: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border: none;
    background: transparent;
    color: var(--color-text-muted);
    height: 26px;
    padding: 0 8px;
    border-radius: var(--radius-xs);
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    font-weight: 500;
    line-height: 1;
    cursor: pointer;
    transition-property: background-color, color, box-shadow, scale;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }

  .seg:hover {
    color: var(--color-text-secondary);
  }

  .seg.active {
    background: var(--color-surface-3);
    color: var(--color-text-primary);
    box-shadow: inset 0 -2px 0 var(--color-accent);
  }

  .seg.done {
    color: var(--color-text-secondary);
  }

  .seg:active {
    scale: 0.96;
  }

  .seg-num {
    color: var(--color-accent-text);
    font-size: var(--text-2xs);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  /* === Step panel ========================================================= */
  .step-panel {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-surface-1);
    animation: u-fade-in var(--motion-base) var(--ease-standard);
  }

  .step-head {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .step-title {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--text-lg);
    line-height: var(--text-lg--line-height);
    font-weight: 600;
  }

  .step-help {
    margin: 0;
    color: var(--color-text-muted);
    font-size: var(--text-2xs);
    line-height: var(--text-2xs--line-height);
  }

  /* === Mode cards (explicit example seeds) =============================== */
  .mode-row {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }

  .mode-card,
  .chip,
  .check-row {
    border: 1px solid var(--color-border);
    background: var(--color-surface-1);
    color: var(--color-text-secondary);
    cursor: pointer;
    transition-property: background-color, border-color, color, scale;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }

  .mode-card {
    padding: 10px 12px;
    border-radius: var(--radius-md);
    text-align: left;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .mode-card:hover,
  .chip:hover,
  .check-row:hover {
    border-color: var(--color-border-hover);
    color: var(--color-text-primary);
  }

  .mode-card:active,
  .chip:active,
  .check-row:active {
    scale: 0.96;
  }

  .mode-card.active,
  .chip.active {
    border-color: var(--color-border-active);
    background: var(--color-accent-subtle);
    color: var(--color-text-primary);
  }

  .mode-eyebrow {
    color: var(--color-accent-text);
    font-size: var(--text-2xs);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .mode-card strong {
    font-family: var(--font-display);
    font-size: var(--text-sm);
    line-height: var(--text-sm--line-height);
    font-weight: 600;
  }

  .mode-card small {
    color: var(--color-text-muted);
    font-size: var(--text-2xs);
    line-height: var(--text-2xs--line-height);
  }

  .seed-hint {
    margin: 0;
    color: var(--color-text-muted);
    font-size: var(--text-2xs);
    line-height: var(--text-2xs--line-height);
  }

  /* === Fields (DESIGN 8 field spec) ====================================== */
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--color-text-secondary);
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    line-height: var(--text-xs--line-height);
    font-weight: 500;
  }

  input,
  select,
  textarea {
    width: 100%;
    box-sizing: border-box;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    color: var(--color-text-primary);
    border-radius: var(--radius-sm);
    font-family: var(--font-ui);
    font-size: var(--text-sm);
    line-height: 1.5;
    outline: none;
    transition-property: border-color, box-shadow, background-color;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }

  input,
  select {
    height: 28px;
    padding: 0 8px;
  }

  textarea {
    resize: vertical;
    min-height: 48px;
    padding: 7px 8px;
  }

  input:hover,
  select:hover,
  textarea:hover {
    border-color: var(--color-border-hover);
  }

  input:focus,
  textarea:focus,
  select:focus {
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px var(--color-accent-subtle);
  }

  input::placeholder,
  textarea::placeholder {
    color: var(--color-text-muted);
  }

  .pair-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }

  /* === Chips ============================================================== */
  .chip-group {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .group-label {
    flex-basis: 100%;
    color: var(--color-accent-text);
    font-size: var(--text-2xs);
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .chip {
    border-radius: var(--radius-sm);
    padding: 5px 9px;
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    line-height: 1;
  }

  /* === Policy checklist (vertical full-width rows) ======================== */
  .check-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .check-row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 10px;
    border-radius: var(--radius-md);
    text-align: left;
  }

  .check-row.active {
    border-color: var(--color-border-active);
    color: var(--color-text-primary);
  }

  .check-box {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    flex: 0 0 16px;
    border: 1px solid var(--color-border-hover);
    border-radius: var(--radius-xs);
    background: var(--color-surface-2);
    color: var(--color-on-accent);
    transition-property: background-color, border-color;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }

  .check-row.active .check-box {
    background: var(--color-accent);
    border-color: var(--color-accent);
  }

  .check-copy {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .check-copy strong {
    font-size: var(--text-xs);
    line-height: var(--text-xs--line-height);
    font-weight: 600;
  }

  .check-copy span {
    color: var(--color-text-muted);
    font-size: var(--text-2xs);
    line-height: var(--text-2xs--line-height);
  }

  /* === Result summary ===================================================== */
  .summary {
    margin: 0;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-0);
    overflow: hidden;
  }

  .sum-row {
    display: grid;
    grid-template-columns: 96px minmax(0, 1fr);
    gap: 8px;
    align-items: baseline;
    padding: 8px 10px;
  }

  .sum-row + .sum-row {
    border-top: 1px solid var(--color-border);
  }

  .sum-row dt {
    color: var(--color-text-muted);
    font-size: var(--text-2xs);
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .sum-row dd {
    margin: 0;
    color: var(--color-text-primary);
    font-size: var(--text-sm);
    line-height: var(--text-sm--line-height);
    overflow-wrap: anywhere;
  }

  .sum-row dd.tabular {
    font-variant-numeric: tabular-nums;
  }

  .sum-row dd.chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .sum-row dd.chips .none {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
  }

  .sum-row dd.policy {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
  }

  .pol {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    line-height: var(--text-xs--line-height);
  }

  .pol.on {
    color: var(--color-text-secondary);
  }

  .pol-mark {
    color: var(--color-text-muted);
  }

  .pol.on .pol-mark {
    color: var(--color-ok);
  }

  /* === Prompt disclosure (same pattern as Settings Advanced) ============== */
  .prompt-box {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .disclosure {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    background: var(--color-surface-1);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    color: var(--color-text-secondary);
    cursor: pointer;
    transition-property: background-color, border-color, color;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }

  .disclosure:hover {
    background: var(--color-surface-2);
    border-color: var(--color-border-hover);
    color: var(--color-text-primary);
  }

  .chevron {
    display: inline-flex;
    color: var(--color-text-muted);
    transition: transform var(--motion-base) var(--ease-standard);
  }

  .chevron.open {
    transform: rotate(90deg);
  }

  .disclosure-label {
    font-family: var(--font-ui);
    font-size: var(--text-sm);
    font-weight: 600;
  }

  .disclosure-hint {
    color: var(--color-text-muted);
    font-size: var(--text-2xs);
  }

  .prompt-pre {
    margin: 0;
    max-height: 200px;
    overflow: auto;
    padding: 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-0);
    color: var(--color-text-secondary);
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    line-height: var(--text-xs--line-height);
    white-space: pre-wrap;
    animation: u-fade-in var(--motion-base) var(--ease-standard);
  }

  /* === Footer ============================================================= */
  .step-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-top: 12px;
    border-top: 1px solid var(--color-border);
  }

  .step-count {
    color: var(--color-text-muted);
    font-size: var(--text-2xs);
    line-height: var(--text-2xs--line-height);
    font-variant-numeric: tabular-nums;
  }

  .result-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
</style>
