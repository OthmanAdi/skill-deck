# Future Implementation Plan

Last updated: 2026-05-01
Horizon: next 3 release cycles

## Vision

Make Skill Deck the fastest and most reliable multi-agent skill overlay, with transparent support boundaries and strong open-source maintainer ergonomics.

## Principles

1. Security before convenience
2. Honest platform support claims
3. Predictable keyboard interactions
4. Documentation as part of the product
5. Incremental delivery with measurable quality gates

## Release Cycle Roadmap

## Cycle A: Stabilize v0.1

Goals:
- close all known P0 and P1 issues
- enforce strict local and CI quality gates
- establish OSS contribution baseline docs

Feature work:
- secure read path by skill ID only
- repo override integration in scan pipeline
- update checker reliability improvements
- tree and keyboard interaction correctness fixes

Success metrics:
- 0 P0, 0 P1 open
- all checks green in CI

## Cycle B: Cross-Platform Maturity

Goals:
- maintain strong cross-platform consistency for active features

Feature work:
- improve scan performance on large global skill catalogs
- continue hardening update and history flows
- expand command and hook source coverage beyond Claude settings while preserving local-only processing
- add richer risk labeling for command and hook artifacts in UI

Success metrics:
- documented parity matrix in README
- no misleading feature claims

## Cycle C: Ecosystem and Maintainer Velocity

Goals:
- improve community onboarding and release ops
- improve update provider coverage

Feature work:
- richer changelog automation
- issue templates and PR templates
- support policy for GitHub-first then optional GitLab extension

Success metrics:
- faster PR review cycle
- clear contributor onboarding path

## Backlog by Theme

## Security
- add tests for invalid path attempts on content read command
- evaluate additional command hardening patterns for IPC endpoints

## UX
- optimize perceived smoothness of overlay transitions
- keep keyboard-only flow predictable in grouped and tree modes

## Platform
- keep behavior parity for active feature set across Windows, macOS, Linux

## Documentation
- maintain living changelog and release notes discipline
- keep release criteria in sync with real pipeline behavior

## Operating Model

- Every feature starts with acceptance criteria
- Every merge updates at least one of: tests, docs, or both
- Every release candidate includes explicit deferred-items section

## Definition of Done for Future Work

A task is complete only when:
1. code passes local checks
2. tests cover critical behavior changes
3. user-facing docs reflect new behavior
4. no contradiction exists between README claims and implementation
