# Skill Deck Release Plan

Last updated: 2026-05-01
Target: open-source production readiness

## Objective

Ship a trustworthy and maintainable v0.1 open-source release with clear platform boundaries, secure defaults, verified quality gates, and transparent roadmap communication.

## Release Criteria

The release is allowed only when all conditions below are true:

1. Security
- No unrestricted file read IPC endpoints
- CSP enabled for production webview
- Terminal injection guarded against non-terminal targets

2. Product correctness
- Drag to inject works on supported platforms
- Tree view behaves correctly for focus and nested items
- Update checker reports reliable results for supported repo providers

3. Quality
- `pnpm check` passes
- `cargo test` passes
- `cargo clippy -- -D warnings` passes
- CI runs on pull requests and pushes

4. OSS readiness
- `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md` exist
- README platform support language matches actual behavior
- future roadmap is documented for community visibility

## Milestones

## M1 Security and Core Reliability

Status: in progress

Scope:
- Harden `read_skill_content` access model
- Enable production CSP
- Strengthen injection command validation
- fix update checker comparison model and timeout behavior

Exit criteria:
- Security P0 items closed

## M2 UX and Interaction Correctness

Status: in progress

Scope:
- fix drag/drop injection flow
- fix tree indexing and deep hierarchy behavior
- fix keyboard navigation consistency in overlay

Exit criteria:
- P1 interaction bugs closed

## M3 OSS and Process Professionalization

Status: pending

Scope:
- add governance docs
- add changelog baseline and release notes policy
- add CI workflow for PR and push validation
- align README claims with real support matrix

Exit criteria:
- open-source repo professionalism checklist complete

## M4 Release Candidate

Status: pending

Scope:
- run full verification suite
- close remaining P2 gaps or explicitly defer with issue links
- prepare tagged release notes from changelog

Exit criteria:
- all release criteria met and signed off

## Risk Register

1. Platform support mismatch creates user trust debt
- Mitigation: clear Windows-first labeling until parity lands

2. Security regressions on IPC commands
- Mitigation: constrain commands to IDs and scanner-derived allowlists

3. Drift between docs and code behavior
- Mitigation: release checklist gates README updates with every feature change

4. Unstable update semantics across non-GitHub providers
- Mitigation: explicit provider support policy with roadmap to expand

## Communication Cadence

- Weekly engineering update in `FUTURE_IMPLEMENTATION_PLAN.md`
- Every merged user-facing fix reflected in `CHANGELOG.md`
- Milestone close summary posted in GitHub release draft

## Ownership

- Maintainer: repository owner
- Engineering execution: coding agents + maintainer review
- Release authority: maintainer only after criteria pass
