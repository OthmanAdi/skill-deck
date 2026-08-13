# Security Policy

## Supported versions

Security fixes are applied to the current source and latest release line.

| Version | Supported |
|---|---|
| 0.3.x | Yes |
| Earlier versions | No |

Windows x64 is the only qualified binary target for 0.3.0. Source availability does not imply qualified packaging on other platforms.

## Report a vulnerability

Do not open a public issue for a suspected vulnerability. Use the repository's [private security advisory form](https://github.com/OthmanAdi/skill-deck/security/advisories/new).

Include the affected version or commit, reproduction steps, expected and actual behavior, impact, and any suggested mitigation. Avoid including real secrets or unrelated personal data.

Response targets:

- Initial acknowledgement within 72 hours.
- Triage decision within 7 days.
- Remediation timing based on severity and complexity.

These are targets, not a service-level agreement.

## Security-sensitive areas

- Tauri IPC commands that access the filesystem, window state, or global input.
- Agent path resolution and the allowlist used to read or write discovered artifacts.
- Parser handling for commands, hooks, prompts, and frontmatter.
- Provider credentials, remote model requests, and web tools.
- Managed model download origin restrictions, pinned hashes, and local sidecar lifecycle.
- Marketplace and skill-update network requests.
- Release signing, installer contents, SBOM generation, checksums, and build-path provenance.

## Privacy boundaries

Artifact discovery, parsing, filtering, preferences, snapshots, chat history, and harness telemetry are local operations. Skill Deck does not run a project telemetry service.

Network access occurs when a user searches a marketplace, checks a skill's GitHub source, invokes web tools, downloads the managed Jan-nano model, or uses a configured remote AI provider. Content sent to a remote provider is governed by that provider's terms and privacy practices.

Provider and web-search credentials are stored in the local application configuration and masked when configuration is returned to the interface. Contributors must preserve that boundary and must not add credentials to logs, errors, fixtures, screenshots, or diagnostic bundles.

## Release integrity

Official Windows artifacts must be Authenticode-signed, checked for prohibited build paths and PDB content, accompanied by checksums and release evidence, and produced by the tag workflow. The project does not publish unsigned binaries as official releases.

## Coordinated disclosure

After a fix is available, coordinated disclosure is preferred. Credit is offered when requested and appropriate, unless anonymity is requested.
