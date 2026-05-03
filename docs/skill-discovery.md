# Skill Discovery Design

Skill Deck includes a discovery layer that helps users decide when to use each artifact.

## Goals

- Keep the feature deterministic and local, no remote inference.
- Work across all supported agent formats.
- Preserve existing grouped and card views.
- Make filtering explainable for open source users.

## Discovery Model

Each artifact is enriched with:

- `discoveryTags`: normalized topic tags used for facets.
- `useCases`: short action labels that answer when to use a skill.
- `discoveryHints`: provenance hints for diagnostics and trust.

The enrichment pass runs in backend scan flow after parsing and before returning scan results.
Artifact type classification is applied before enrichment and contributes to tags and use-cases.

## Signal Sources

Priority order:

1. Explicit frontmatter `tags` and `use-cases` or `use_cases`
2. Frontmatter `category`
3. Frontmatter `trigger`
4. Heuristics from `name`, `description`, `allowed-tools`, `globs`, and language metadata
5. Fallback tags and use-cases (`general`, `explore`)

Artifact-type priors:

- `command` adds `commands` tag and `on-demand` use-case
- `hook` adds `hooks` tag and `auto-run` use-case
- `rule` adds `rules` tag and `govern` use-case
- `workflow` adds `workflow` tag and `automate` use-case
- `prompt` adds `prompt` tag and `assist` use-case
- `config` adds `config` tag and `configure` use-case

## Faceted Filtering

Frontend adds a FacetBar with two dimensions:

- Artifact-type chips
- Use-case chips, primary decision axis
- Tag chips, secondary topic axis

Filters compose with existing controls:

- All or Starred tab
- Agent filter
- Search
- View mode (Agents grouped, Card View hierarchy)

## Open Source Safety

- No artifact content is sent outside local machine for classification.
- No private prompt telemetry is introduced.
- Feature behavior is reproducible from repository code.
