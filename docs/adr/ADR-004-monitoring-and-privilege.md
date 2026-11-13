# ADR-004: Monitoring lifecycle and least privilege

- **Status:** Accepted
- **Context:** Background work and broad privileges increase privacy and safety risks.
- **Decision:** Foreground monitoring by default. Optional background monitoring only after lifecycle and permission design. No root daemon or unrestricted privileged filesystem access.
- **Consequences:** Background behavior is not part of the initial scaffold; any future permission/lifecycle design needs explicit review.
