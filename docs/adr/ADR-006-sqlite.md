# ADR-006: SQLite and local persistence

- **Status:** Accepted. Implemented for the quarantine history (ADR-012), the activity log
  and preferences (ADR-013).
- **Context:** Settings and operation history need local persistence without a hosted account/backend.
- **Decision:** Use SQLite with explicit schema migrations and keep data local.
  - `rusqlite` with `bundled` SQLite, so the schema doesn't depend on the macOS library's version.
  - The file is `history.sqlite3` in the app's data folder: owner-only (0600) in a 0700 folder, with a symlinked folder refused.
  - Migrations are numbered, each in a transaction with its `user_version` bump, and only ever appended.
  - The connection uses WAL and `synchronous = FULL`, with foreign keys on.
- **Consequences:**
  - A file that fails `quick_check`, has a newer schema, or fails a migration is reported as unavailable and left byte-for-byte as it is. It is never repaired, recreated or downgraded, and every file-changing operation is refused until it is resolved.
  - Activity is kept for the period chosen in Settings and deleted after it, or on Clear log;
    quarantine records stay while their items are needed (ADR-013). Backup and recovery of
    a damaged file, and export, remain open.
  - Preferences and excluded folders live in the database (ADR-013); the appearance
    preference still lives in WebView storage, so the theme applies before the first paint.
