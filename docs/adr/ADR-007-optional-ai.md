# ADR-007: Optional AI and offline-capable core

- **Status:** Accepted
- **Context:** AI may help explain findings, but must not become a safety authority or privacy prerequisite.
- **Decision:** Optional AI behind a provider abstraction. Deterministic offline functionality remains authoritative. No default file-content uploads or telemetry; provider use requires consent and disclosure.
- **Consequences:** AI output is untrusted advisory text, cannot authorize deletion, bypass policy or run arbitrary commands. Providers and data retention remain future decisions; no AI functionality exists.
