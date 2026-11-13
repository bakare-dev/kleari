# ADR-001: Desktop architecture and technology stack

- **Status:** Accepted
- **Context:** Kleari is a local-first macOS desktop application with a safety-critical filesystem boundary.
- **Decision:** Use Tauri 2, React, TypeScript and Rust. Rust owns safety decisions and filesystem operations; frontend uses narrow typed, validated IPC and has no unrestricted filesystem or shell access.
- **Consequences:** Small desktop footprint and explicit native boundary; contracts and validation must be maintained across languages. Shell scaffold exists; product commands do not.
