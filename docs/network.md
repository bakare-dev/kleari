# What Kleari sends over the internet

Your Mac's data stays on your Mac. Kleari uses the internet for one thing only: checking for
updates and, when you choose **Install**, downloading one. This page lists every address it
contacts, what it sends, how the code is kept to that, and how you can check it yourself.

## Every connection Kleari makes

| When | Address | Sent | Received |
|---|---|---|---|
| **Update check**: at launch (unless Settings › About › Check for updates automatically is off) and when you choose **Check now** | `https://github.com/bakare-dev/kleari/releases/latest/download/latest.json`, or with Beta updates on, `https://github.com/bakare-dev/kleari/releases/download/beta-channel/latest.json` | An HTTPS request for that file, with the user agent `tauri-plugin-updater/<version>`. No version, model, identifier or cookie is added. | `latest.json`: the newest version's number, release notes, download address and signature |
| **Update download**: only when you choose **Install** | The `Kleari.app.tar.gz` named in `latest.json`, which must be under `https://github.com/bakare-dev/kleari/releases/download/`, or Kleari refuses it | The same kind of request | The update, which installs only if it's signed with Kleari's updater key for that version |

GitHub answers both requests by redirecting to its own download host,
`release-assets.githubusercontent.com` (in the past `objects.githubusercontent.com`). Like
any download, GitHub sees your IP address. Kleari has no server, account, analytics or
telemetry of its own, so nothing else is collected. GitHub's public download counts for each
release file are the only usage figures there are.

## What isn't Kleari going online

- **Links you open.** "See what's new" opens the release page in your browser, the legal
  links open Kleari's documents on GitHub, and Settings › About › Report a problem opens a new
  issue on `github.com/bakare-dev/kleari/issues/new`, filled in with Kleari's version, the
  macOS version and the kind of chip (never the log, which you attach yourself if you choose).
  Your browser makes those requests, not Kleari, and nothing is sent until you submit.
- **macOS itself.** When you first open an app, macOS may check its signature with Apple
  (Gatekeeper). That's macOS's own check, made for every app.
- **Local connections.** Kleari's window talks to its own engine over an in-app channel, and
  the Developer page asks Docker's engine for disk usage over a Unix socket file on your Mac
  (`~/.docker/run/docker.sock` or `/var/run/docker.sock`). Neither leaves the Mac.

## How the code is held to this

`pnpm check:network` ([`apps/desktop/scripts/check-network-boundary.mjs`](../apps/desktop/scripts/check-network-boundary.mjs))
runs in CI on every change and fails if any of these stops being true:

1. The only Rust code able to make HTTP or internet connections is reached through Tauri's
   updater plugin, as Cargo resolves Kleari's dependencies for macOS. Kleari depends on no
   socket library directly. Tauri's async runtime has its own, which Kleari's code can't use
   for network sockets (see 4).
2. The updater is the only Tauri plugin. No capability, whether in a file or written inside
   `tauri.conf.json`, grants a plugin that could fetch, open sockets or run programs. A
   capability file the check can't read counts as a failure.
3. The updater's addresses are Kleari's own GitHub releases, over HTTPS.
4. The Rust code opens no internet sockets, uses no native networking API, and shows no web
   page but Kleari's own. It runs only a fixed few Apple programs. AppleScript appears only
   in the two files reviewed for it (browser tabs and maintenance tasks). It may run only the
   reviewed shell commands, exactly as written, and may never reach Cocoa, other scripts or
   JavaScript for Automation, or send a browser to an address. Every web address in it
   is Kleari's GitHub, Apple's plist identifier (a label, never fetched) or a reserved test
   domain.
5. The interface uses no `fetch`, XHR, WebSocket, EventSource, beacon or HTTP library, loads
   no outside file, and names no web address but Kleari's GitHub.
6. The window's content security policy lets it load and connect to nothing outside the
   app: `default-src 'self'; connect-src 'self' ipc: http://ipc.localhost`. Objects, frames,
   forms and base URLs must stay blocked by name.

The check has its own tests (`pnpm test:network`), which give it a violation of each kind and
make sure it fails. Its lists of networking libraries and patterns are kept up to date by
hand. Pattern checks catch what is written in the code; they aren't a proof against code
built to hide. What does the heavy lifting is that only the updater has a way to connect at
all.

The download address in `latest.json` is checked again in Rust before an update is offered
and before it is downloaded (`apps/desktop/src-tauri/src/updates.rs`).

## Check it yourself

- **A network monitor** such as [LuLu](https://objective-see.org/products/lulu.html) or
  Little Snitch: allow Kleari only `github.com` and `release-assets.githubusercontent.com`;
  everything should keep working.
- **No checks at all:** turn off Settings › About › Check for updates automatically and
  don't choose Check now. Kleari then makes no internet connection at all. You can watch
  with `nettop -p $(pgrep -x kleari)` in Terminal.

This describes the current code. Any change to it updates this page, and CI fails the change
if the code no longer matches.
