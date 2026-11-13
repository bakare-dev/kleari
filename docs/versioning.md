# Versioning

Kleari's versions follow [Semantic Versioning](https://semver.org): `MAJOR.MINOR.PATCH`, with
`-beta.N` for betas. The same version is set in `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml` and `apps/desktop/src-tauri/tauri.conf.json`.

## Which number changes

Ask in this order; the first "yes" decides.

**MAJOR (1.x → 2.0)** when someone on the previous version can't simply update and carry on:

- Kleari drops a macOS version or Mac type it supported (for example, no longer macOS 14).
- Data saved by the previous version (history, quarantine records, settings) can't be read
  without a one-way change, or is reset.
- The app's identity or updater signing key changes, so the previous version can't update
  in-app and must be reinstalled.
- A feature people rely on is removed, or something that was read-only starts changing
  files.
- A required permission is added (for example, Kleari starts needing Full Disk Access to
  work at all).

**MINOR (1.0 → 1.1)** for anything new that keeps everything working as before:

- A new screen, feature or setting (new settings default to how things already behave).
- A new cleanup category or scan source; a new optional permission behind an explained
  request.
- Data saved by the previous version is still read, with any migration automatic and
  safe.

**PATCH (1.1.0 → 1.1.1)** for fixes only:

- Bug and security fixes, wording, translations and performance, with no new feature and no
  change to what is saved.

Before 1.0, versions are `0.y.z`, and a MINOR change may break things (SemVer's rule for
initial development). 1.0.0 is the first stable release.

## Betas

A beta previews the next version, numbered for that version: `1.1.0-beta.1`, `-beta.2`, and so
on, restarting at `beta.1` for each new version. Betas are published as GitHub pre-releases,
offered only with Settings › About › Beta updates on (on by default when the installed copy is
itself a beta, off for a stable release). A beta that is ready
becomes the stable version (`1.1.0`) with no further changes, from the `release` branch. A
fix to a stable release (`1.1.1`) skips betas when it is small and urgent.

## Examples

| From | Change | To |
|---|---|---|
| 1.0.0 | Fix a wrong size on the Storage page | 1.0.1 |
| 1.0.1 | Add a new Developer cache | 1.1.0 |
| 1.1.0 | Preview the next features | 1.2.0-beta.1 |
| 1.2.0-beta.3 | Beta judged ready | 1.2.0 |
| 1.2.0 | Require macOS 15 | 2.0.0 |
| 1.2.0 | New history format that 1.x can't read back | 2.0.0 |
