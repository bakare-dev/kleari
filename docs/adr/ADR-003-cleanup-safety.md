# ADR-003: Cleanup approval and recovery safety model

- **Status:** Accepted
- **Context:** Incorrect cleanup can cause irreversible personal data loss.
- **Decision:** Read-only scanning and dry-run preview precede every operation. Require explicit human approval and independent backend revalidation. Quarantine first and restore where feasible. Quarantine failure never silently falls through to permanent deletion. Permanent deletion requires separate explicit confirmation. Restore never overwrites conflicts.
- **Consequences:** Operations may be blocked or partial and must report why. Quarantine is never emptied automatically; there is no retention period or cap, and low-space behavior remains unresolved. Implemented as described in [ADR-012](ADR-012-cleanup-and-quarantine.md).
