# Product requirements

## Status and principles

Kleari is a local-first macOS maintenance and developer productivity app. Current phase is foundation only: the desktop shell is scaffolding and performs no scans or operations. Guiding principle: **Scan freely. Explain clearly. Execute only with explicit authorisation.**

## Milestones

### MVP (planned)

- Show supported CPU, memory, disk, battery readings and process summaries with source, freshness and unavailable states.
- Scan only user-authorized storage locations read-only; support large-file filtering and explainable classification of known caches and selected developer artifacts.
- Show reasons, uncertainty, risk classification, eligibility, exact planned effect and recovery limits in a dry-run preview.
- Require human approval; have Rust independently revalidate policy, location, identity and permissions immediately before execution.
- Quarantine first, restore where feasible without overwriting conflicts, and require a distinct confirmation for permanent deletion.
- Persist settings and operation history locally in SQLite with explicit migrations.
- Inspect Docker inventory and make recommendations read-only.
- Foreground monitoring by default. Optional background monitoring only after lifecycle and permission design.
- Optional AI explanation provider behind consent and disclosure; deterministic offline core remains authoritative.

### Later milestones

Design and validate background lifecycle/notifications, expanded developer artifact coverage, configurable AI-tool paths, public distribution and signing/notarization, and optional AI providers. Each needs its own acceptance and privacy review.

### Exclusions for v1

Autonomous/AI-authorized deletion; root daemon; unrestricted privileged operations; automatic cleanup of system paths; Docker pruning or mutation; deletion of source code, Git internals, credentials, databases or persistent volumes; required hosted services, telemetry or cloud AI; unsupported performance or battery improvement claims; Windows and Linux.

## Functional requirements (planned)

1. The user can inspect readings and process summaries without initiating cleanup.
2. The user can explicitly grant access to scan selected locations. Scan is read-only, cancellable and reports inaccessible, skipped and partial results.
3. A discovered item carries stable identity evidence, classification reason, uncertainty, risk, policy eligibility and scan freshness. Risk and eligibility are distinct; a low-risk label alone never grants permission.
4. Preview shows the specific items and effects. Any stale identity, policy ambiguity, protected path, inaccessible resource or revoked permission blocks execution.
5. Every operation requires a user confirmation bound to a preview. Backend revalidation is mandatory. Failure is visible and recorded.
6. Quarantine failure never silently falls back to permanent deletion. Restore conflicts never overwrite. Permanent deletion requires separate explicit confirmation.
7. History reports successes, partial failures, cancellation and recovery limits without exposing sensitive path details unnecessarily.
8. Docker integration reads inventory only; no stop/remove/prune/volume mutation.
9. AI output is advisory, treated as untrusted data, and cannot execute commands or influence authorization.

## User stories and acceptance criteria

- As a user, I can understand storage findings before acting: each item shows why it was found, uncertainty, risk and whether policy permits an operation.
- As a user, I can cancel a scan or preview without side effects; scanner tests prove no writes occur.
- As a user, I can approve only the preview I reviewed; changing/replacing an item or revoking access makes the approval stale and execution fail closed.
- As a user, I can restore quarantined items when safe; an existing destination is preserved and reported as a conflict.
- As a privacy-conscious user, I can use the deterministic core offline; no telemetry or content upload occurs by default.
- As a user, I can inspect Docker state without the app changing it.

## Permissions, errors and recovery

Request only narrowly scoped macOS user-granted access when needed. Permission denial/revocation is a visible partial/error state, never an implicit broader-access attempt. Cancellation stops safely and records partial results. Disk-full, crash, migration failure and restore conflicts must be surfaced with recovery guidance. Incomplete operations are not represented as successful or reversible.

## Privacy and offline behavior

Data remains local by default. No account, default telemetry, default content upload or mandatory AI. AI use requires provider-specific consent, data disclosure and user-selected content. See [privacy](privacy.md).

## Product decisions still open

- Final quarantine retention period, storage cap and low-disk-space behavior. Thirty days is only a proposed target; never silently delete to enforce retention.
- Exact supported metrics and permission behavior across macOS versions.
- Specific classifier rules, eligible paths and user-configurable AI tool locations.
- Background lifecycle and notification permissions.
- AI providers, data minimization, retention and consent UX.
- Audit-history retention, export and sensitive-path redaction level.
- Signing identity, distribution channel and final public support matrix.
