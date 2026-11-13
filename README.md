# Kleari

**Keep your Mac clean and fast, on your Mac only.** Kleari finds space you can reclaim, moves
it to a quarantine you can undo, shows what your Mac is doing, tidies developer caches and
makes a mouse work your way. Nothing leaves your Mac: no account, no telemetry, no cloud.

> **Installing Kleari?** Read [Read me before you install](docs/install.md#read-me-before-you-install)
> first. macOS warns you the first time you open it; the guide shows how to allow it (choose
> **Done**, never **Move to Trash**, then **Open Anyway** in System Settings).

## What it does

- **Dashboard:** storage, memory, CPU and battery at a glance, your last scan and what
  quarantine holds.
- **Cleanup:** scans for caches, logs, temporary files, package caches and developer build
  folders, explains each finding and its risk, and moves only what you approve into
  quarantine. Optional scheduled scans (read-only, then a notification).
- **Quarantine:** restore anything, never overwriting; delete permanently only with a second
  confirmation.
- **Storage:** how your startup disk is used, by folder and category, and files of 1 GB or more.
- **System:** live CPU, memory, swap, battery, thermal state, network and disk activity, this
  Mac's details and its startup items.
- **Processes:** what is running, with a polite, confirmed Quit for regular apps.
- **Developer:** Xcode, simulators, npm, Homebrew, Cargo and other caches, project build
  folders and Docker's disk use (read-only).
- **Mouse:** a mouse wheel that scrolls the classic way while the trackpad stays natural,
  wheel speed and smooth scrolling, button actions, media keys and combinations.
- **Menu bar:** live readings and your last scan, one click away.
- **Activity:** a local log of what Kleari did.

Requires a Mac with Apple silicon and macOS 14 or later.

## Download

Get the latest DMG from [Releases](https://github.com/bakare-dev/kleari/releases). Stable
releases are marked **Latest**; betas are pre-releases. Kleari updates itself from there, and
Settings › About › **Beta updates** chooses whether you also get betas.

## Privacy and security

Everything Kleari reads stays on your Mac. Its only internet request is the update check, to
Kleari's own GitHub releases, and an update installs only if it carries Kleari's signature.
Scans only read; files move only after you approve a preview, into a quarantine you can undo.
See [privacy](docs/privacy.md), the [threat model](docs/threat-model.md) and the
[security policy](SECURITY.md).

## Support Kleari

Kleari is free. If it saves you time or space:

- ⭐ **Star this repository.** It helps others find Kleari.
- ☕ **Buy me a coffee:** [buymeacoffee.com/bakaredev](https://buymeacoffee.com/bakaredev)

<a href="https://buymeacoffee.com/bakaredev"><img src="docs/design/v2/brand/qr-code.png" alt="QR code for buymeacoffee.com/bakaredev" width="160"></a>

## Contributing

Contributions are welcome, and every change follows [CONTRIBUTING.md](CONTRIBUTING.md): the
branch flow, the checks, and the safety and privacy rules that keep Kleari trustworthy.
Report vulnerabilities privately, as described in [SECURITY.md](SECURITY.md).

## Develop

Prerequisites: macOS 14+ on Apple silicon, Node.js `^22.13.0 || >=24 <26`, pnpm 10.12.1,
Rust 1.95 or later (CI uses 1.99.1), Xcode Command Line Tools and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/).

```sh
corepack enable
corepack prepare pnpm@10.12.1 --activate
pnpm install --frozen-lockfile
cd apps/desktop && pnpm tauri dev
```

`pnpm dev` alone starts the interface in a browser, with no Rust backend; development builds
also show the design's sample screens. The checks every change must pass:

```sh
pnpm typecheck && pnpm lint && pnpm build && pnpm check:release-bundle
cd apps/desktop/src-tauri && cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked
```

Stack: Tauri 2, React, TypeScript, Rust and SQLite. React presents and asks through narrow
typed IPC; Rust decides everything that touches files. See the
[architecture](docs/architecture.md), [decisions](docs/adr/), [versioning](docs/versioning.md)
and [release process](docs/release-checklist.md).

## License

Kleari is free software under the [GNU General Public License v3.0 or later](LICENSE). You may
use, study, change and share it; if you distribute a modified version, you must share its
source under the same license.

Official releases are published only on this repository's
[Releases](https://github.com/bakare-dev/kleari/releases) page.
