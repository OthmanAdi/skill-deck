# Security Policy

## Supported Versions

Security fixes are applied to the latest release line.

| Version | Supported |
|---|---|
| latest | yes |
| older | no |

## Reporting a Vulnerability

Please do not open public issues for security vulnerabilities.

Report privately by contacting the maintainer through GitHub security advisory flow for this repository.

When reporting, include:
- affected version or commit
- reproduction steps
- expected vs actual behavior
- impact assessment if known

## Response Targets

- Initial acknowledgment: within 72 hours
- Triage decision: within 7 days
- Fix timeline: depends on severity and complexity

## Scope Notes

Areas with elevated security sensitivity:
- Tauri IPC commands that access filesystem or window/input APIs
- update-check network logic and provider validation
- parser paths that expose command or hook payloads for copy workflows

## Local Privacy Expectations

- Artifact parsing, discovery enrichment, and filtering are local-only operations.
- Command and hook payloads are displayed and copied locally, they are never uploaded by Skill Deck.
- Contributors should avoid adding sample docs or fixtures that include real secrets, tokens, or production endpoints.

## Disclosure

After a fix is available, coordinated disclosure is preferred with a changelog entry under the Security section.
