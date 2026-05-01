# Changelog Plan

Last updated: 2026-05-01
Standard: Keep a Changelog + Semantic Versioning

## Purpose

Define how this repository records changes so users and contributors can track behavior, compatibility, and upgrade impact clearly.

## Format Policy

The project changelog uses:
- Keep a Changelog section structure
- Semantic versioning (`MAJOR.MINOR.PATCH`)
- ISO date format (`YYYY-MM-DD`)

Required top sections:
- `Unreleased`
- released versions in reverse chronological order

Required categories per version:
- Added
- Changed
- Fixed
- Security

Optional categories:
- Deprecated
- Removed

## Writing Rules

1. User-centric wording
- describe impact, not internal commit noise

2. Specificity
- avoid vague entries like “various fixes”

3. Traceability
- include file or subsystem mention when useful

4. Breaking changes
- clearly marked under `Changed` or `Removed`
- include migration notes when applicable

## Update Process

For every merged PR:
1. add or update entry under `Unreleased`
2. categorize correctly
3. include security note when relevant

During release cut:
1. move `Unreleased` items into new version section
2. set release date
3. reset `Unreleased`

## Versioning Guidance

- Patch: bug fix or non-breaking hardening
- Minor: backward-compatible feature additions or UX upgrades
- Major: breaking behavior or API contract changes

## Enforcement

Release should be blocked if:
- changelog is missing release notes for shipped work
- security-impacting changes are not called out

## Related Docs

- `CHANGELOG.md`
- `RELEASE_PLAN.md`
- `FUTURE_IMPLEMENTATION_PLAN.md`
