# Testing strategy

By the product owner's decision, Kleari keeps automated tests only where a mistake could harm
a user's files, permissions or data, all in Rust. The interface has no automated tests; it is
checked by TypeScript, ESLint, the build and the release-bundle check, and by hand.

## What is tested (Rust, `cargo test`)

- **Cleanup and quarantine** (`cleanup/tests.rs`, `quarantine/tests.rs`): a swapped item or
  symlink is never moved, a change after the preview moves nothing, an app opened after the
  scan blocks its cache, forged and inspect-only items are never planned, nothing moves
  without a working history, restores never overwrite, a missing original folder blocks a
  restore, interrupted moves are reconciled at launch, and permanent delete stays inside
  quarantine.
- **History** (`persistence/tests.rs`): migrations, a failed migration rolled back, a corrupt
  or newer database refused and left unchanged, owner-only permissions.
- **Reading without changing or escaping:** storage scans never follow symlinks, refuse a
  replaced or linked root and change nothing on disk; developer and startup-item reads don't
  follow symlinks or touch network mounts; directory walks (`storage/fsdir.rs`) open only
  without following links.
- **Quitting apps** (`system_monitor/process_list.rs`, `running_apps.rs`): never Kleari,
  macOS's own apps, another account's or an unidentified process, nor a reused PID.
- **Mouse** (`mouse/engine.rs`, `config.rs`, `probe.rs`): what each button, wheel and
  combination does, left click always reaching apps, presses paired with their releases, and
  only the settings the page offers accepted.
- **Boundaries:** uninstall touching only Kleari's own folders, updates only from Kleari's
  releases and feeds, System Settings panes and menu-bar destinations from fixed lists, and
  the commands each window may call.

## Rules

- All test data is synthetic and disposable: temporary folders only, never the home folder,
  personal files, credentials or persistent Docker volumes.
- A change to anything listed above comes with a test; no test is weakened or skipped to make
  CI pass.
- Behaviour that only shows on a real Mac (macOS privacy prompts, the mouse event tap,
  cross-volume moves, the updater) is checked by hand before a release, and the PR says so.
