# Security policy

## Reporting a vulnerability

Report it privately: on GitHub, go to this repository's **Security** tab and choose **Report a
vulnerability**. Never open a public issue, discussion or pull request for one, and don't
publish details until a fix is released.

Include the Kleari version (Settings › About), your macOS version, what an attacker could do,
and steps to reproduce, using made-up files and folders rather than your own. Don't send
credentials, file contents or real paths unless they're essential.

The maintainer acknowledges a report as soon as practical, keeps you informed, and credits
you in the release notes unless you'd rather not be named. There is no paid bounty.

## Supported versions

Fixes go into the latest stable release and the current beta. Older versions don't get
fixes; update from Settings › About.

## How Kleari is protected

In short (the [threat model](docs/threat-model.md) has the detail):

- **One way in from the network, and it is signed.** Kleari runs no server and listens on no
  port. Its only internet request is the update check, to fixed HTTPS addresses of Kleari's
  own GitHub releases. An update installs only after you choose it, only if its signature
  matches the key built into Kleari and names the version it claims (so an old build can't
  be passed off as new), and only from Kleari's own release downloads.
- **Nothing from outside runs.** The interface loads only Kleari's own bundled code (a strict
  content security policy, no remote scripts), and it can only call the narrow commands
  each window is granted.
- **Your files are moved only by Rust, only as approved.** The interface never sends a path to
  act on; every item is re-checked (identity, size, place, policy) right before it moves,
  into quarantine, never deleted on the way. Symlinks aren't followed, network volumes
  aren't touched, and restores never overwrite.
- **Local data is kept small and private.** History and settings live in a file only your
  account can read, with no file contents, credentials or telemetry. A damaged or edited
  history makes Kleari refuse to move anything rather than guess.
- **Builds are guarded.** Locked dependencies, dependency and secret scanning in CI, and
  protected branches with review.

Kleari can't protect against malware already running as your user account: it could read the
same files Kleari can. Keep macOS up to date and grant Accessibility, Full Disk Access and
folder access only to apps you trust.
