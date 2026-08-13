export type WizardMode = "product" | "team";
export type TargetAgent = "Claude Code" | "Codex" | "Cursor" | "Copilot" | "OpenCode" | "Any Agent";

export interface WizardOption {
  key: string;
  label: string;
  hint: string;
}

export interface WizardTemplate {
  title: string;
  scenario: string;
  repeatedWork: string;
  inconsistentOutput: string;
  successCriteria: string;
  selectedDomains: Record<string, boolean>;
}

export interface SkillLibraryPromptInput {
  mode: WizardMode;
  title: string;
  targetAgent: TargetAgent;
  selectedDomainLabels: string;
  selectedSourceLabels: string;
  scenario: string;
  repeatedWork: string;
  inconsistentOutput: string;
  successCriteria: string;
  constraints: string;
  skillCount: number;
  includeLocal: boolean;
  includeMarketplace: boolean;
  includeWeb: boolean;
  includeDrafts: boolean;
  includeGovernance: boolean;
}

export const WIZARD_MODES: { key: WizardMode; label: string; eyebrow: string; body: string }[] = [
  {
    key: "product",
    label: "Product stack",
    eyebrow: "launch / discovery / GTM",
    body: "Plan the skills a product needs to move from idea to launch, support, and iteration.",
  },
  {
    key: "team",
    label: "Team operating system",
    eyebrow: "function / ritual / playbook",
    body: "Plan the skills a team needs to make its repeated expert work consistent.",
  },
];

export const SOURCE_OPTIONS: WizardOption[] = [
  { key: "docs", label: "Docs", hint: "old docs, wikis, SOPs" },
  { key: "slack", label: "Slack", hint: "threads, decisions, escalations" },
  { key: "calls", label: "Calls", hint: "customer calls, review rituals" },
  { key: "onboarding", label: "Onboarding", hint: "training notes, starter guides" },
  { key: "experts", label: "Expert heads", hint: "top performer judgment" },
];

export const DOMAIN_OPTIONS: WizardOption[] = [
  { key: "product", label: "Product", hint: "PRDs, research, roadmap" },
  { key: "engineering", label: "Engineering", hint: "code review, tests, release" },
  { key: "support", label: "Support", hint: "escalations, triage, answers" },
  { key: "sales", label: "Sales", hint: "call prep, objections, renewals" },
  { key: "marketing", label: "Marketing", hint: "campaigns, voice, launch" },
  { key: "security", label: "Security", hint: "risk, approval, audit" },
];

export const TARGET_AGENTS: TargetAgent[] = [
  "Claude Code",
  "Codex",
  "Cursor",
  "Copilot",
  "OpenCode",
  "Any Agent",
];

export function createWizardTemplate(kind: WizardMode): WizardTemplate {
  if (kind === "team") {
    return {
      title: "Engineering team skill library",
      scenario:
        "An engineering team is adopting AI coding agents and needs shared skills for code review, test planning, release checks, incident postmortems, documentation updates, and security review.",
      repeatedWork:
        "Senior engineers keep explaining the same review standards, test expectations, release rituals, and incident-writing rules.",
      inconsistentOutput:
        "Agents miss repo-specific conventions, over-suggest generic changes, skip verification, and produce reviews with uneven quality.",
      successCriteria:
        "A team-owned skill stack that raises the floor for code review, testing, release readiness, incident learning, and safe tool use.",
      selectedDomains: {
        engineering: true,
        security: true,
        product: false,
        support: false,
        sales: false,
        marketing: false,
      },
    };
  }

  return {
    title: "New product skill library",
    scenario:
      "We are preparing to launch a new AI product and need repeatable skills for research, PRD writing, competitive analysis, launch planning, release notes, customer feedback, and post-launch iteration.",
    repeatedWork:
      "The team keeps re-explaining launch briefs, product positioning, research synthesis, and release checklists to AI tools.",
    inconsistentOutput:
      "Outputs sound generic, miss company-specific decision rules, and vary by whoever prompts the agent.",
    successCriteria:
      "A compact, prioritized skill library with owners, trigger phrases, private skill drafts, public registry candidates, eval prompts, and safety/governance checks.",
    selectedDomains: {
      product: true,
      engineering: true,
      support: true,
      marketing: true,
      sales: false,
      security: false,
    },
  };
}

export function labelsFor(options: WizardOption[], selected: Record<string, boolean>): string {
  const labels = options.filter((option) => selected[option.key]).map((option) => option.label);
  return labels.length > 0 ? labels.join(", ") : "None selected";
}

export function toggleSelection(
  selected: Record<string, boolean>,
  key: string,
): Record<string, boolean> {
  return { ...selected, [key]: !selected[key] };
}

export function buildSkillLibraryPrompt(input: SkillLibraryPromptInput): string {
  const searchPolicy = [
    input.includeLocal
      ? "Use local installed skills first: call get_skill_stats, search_skills, list_skills, and translate_to_skills where useful."
      : "Do not rely on local installed skills unless needed for context.",
    input.includeMarketplace
      ? "Search public registries with search_marketplace for skills.sh and ClawHub candidates. Treat public skills as discovery or import candidates, not blind installs."
      : "Do not search public registries unless required to answer.",
    input.includeWeb
      ? "Use web_search and web_fetch for current references about AI skills, skill libraries, agent instructions, registries, and related examples."
      : "Avoid web search unless necessary.",
  ].join("\n- ");

  return `You are Skill Deck's Skill Library Architect. Build a researched, company-specific skill-library plan from this wizard brief.

Core philosophy to apply:
- Companies already have skills hidden in docs, Slack threads, onboarding notes, calls, review rituals, old prompts, scripts, and expert operators' heads.
- A skill is more than a prompt: it packages repeatable judgment, process, examples, templates, edge cases, quality bars, and optional scripts/resources.
- Access to data is not enough. Skills teach agents how the company does the work.
- Public skills are useful for discovery and inspiration. The most valuable skills are private because company methods are specific.
- Unsafe or unreviewed skills must not be treated as trusted. Recommend INDEX/security scanning, owner review, eval prompts, and manual invocation for high-risk work.

Wizard brief:
- Mode: ${input.mode === "product" ? "Product skill stack" : "Team operating system"}
- Title: ${input.title}
- Target agent surface: ${input.targetAgent}
- Domains: ${input.selectedDomainLabels}
- Hidden sources to harvest: ${input.selectedSourceLabels}
- Scenario: ${input.scenario}
- Repeated work: ${input.repeatedWork}
- Inconsistency or worry points: ${input.inconsistentOutput}
- Success criteria: ${input.successCriteria}
- Constraints: ${input.constraints}
- Requested skill count: ${input.skillCount}

Search and tool policy:
- ${searchPolicy}

Return a production-grade answer with these exact sections:
1. Executive read: explain the skill-library strategy in 5 sentences or fewer.
2. Worry map: map the user's worries to skill-library design decisions.
3. Public vs private decision: what to search publicly, what to create privately, and what to adapt into private reviewed skills.
4. Recommended skill stack: a table with ${input.skillCount} skills, each with name, owner/team, scope, trigger phrase, private/public/adapted source, risk tier, and why it matters.
5. Registry and web findings: summarize any local skill, marketplace, or web candidates you actually found. Do not fabricate unavailable results.
6. Draft private skills: ${input.includeDrafts ? "include 3 compact SKILL.md drafts with description, instructions, references, eval prompts, and forbidden moves." : "skip full drafts, but list what should be drafted next."}
7. Governance and safety: ${input.includeGovernance ? "include owner/reviewer, allowed tools, manual invocation guidance, INDEX scan recommendations, eval cadence, and provenance rules." : "include only minimal safety notes."}
8. Paste-ready workflow prompt: render a concise prompt for ${input.targetAgent} that the user can paste into their coding agent.
9. Next 48 hours: list the shortest path to a demo or paid pilot.

Be specific. Avoid generic AI transformation language. Tie every recommendation to the user's scenario and to reusable skill-library operations.`;
}
