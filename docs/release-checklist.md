# Release checklist

Releases are made by the maintainer. Contributors' changes reach users only through a
release. Nothing is released by automation. Versions follow [versioning.md](versioning.md).

## Channels and branches

| | Stable | Beta |
|---|---|---|
| Built from | `release` | `beta` |
| Version | `X.Y.Z` | `X.Y.Z-beta.N` |
| GitHub release | regular release, marked **Latest** | **pre-release** |
| Offered to | everyone | people with Settings › About › **Beta updates** on |
| Manifest Kleari reads | `releases/latest/download/latest.json` | `releases/download/beta-channel/latest.json` |

`beta-channel` is one release kept only to hold the beta feed: a pre-release whose
`latest.json` always names the newest of the latest beta and the latest stable release. Every
release, beta or stable, replaces that file. Its own assets are just that manifest; the update
downloads come from the versioned release.

## One-time setup

- [ ] **Updater key.** `pnpm tauri signer generate -w ~/.tauri/kleari-updater.key` (choose a
  strong password). Put the public key (`~/.tauri/kleari-updater.key.pub`) in
  `apps/desktop/src-tauri/tauri.conf.json` under `plugins.updater.pubkey`. Never commit the
  private key or its password; back both up, since without them no later update can be
  signed. A new key means a new major version (installed copies can't update to it).
- [ ] **Signing certificate.** In Keychain Access, create a self-signed code-signing
  certificate named `Kleari Self-Signed` (Certificate Assistant › Create a Certificate…,
  type Code Signing). By the product owner's decision Kleari isn't signed with an Apple
  Developer ID or notarised; the stable self-signed identity keeps macOS permissions
  across versions, and [install.md](install.md) explains Open Anyway.
- [ ] **The `beta-channel` release.** Created with the first beta (step 6 below).

## Each release

1. **Scope.** Review every `Unreleased` entry in [CHANGELOG.md](../CHANGELOG.md) against what
   is merged and verified; remove anything not shipped or not checked. Review security,
   privacy, data-integrity and compatibility effects, and pick the version
   ([versioning.md](versioning.md)).
2. **Prepare** (a pull request into the branch being released): set the version in
   `apps/desktop/package.json`, `apps/desktop/src-tauri/Cargo.toml` (and `Cargo.lock`) and
   `apps/desktop/src-tauri/tauri.conf.json`; move the entries into a dated version heading,
   leaving an empty `Unreleased`; write `docs/releases/<version>.md`, starting with a link to
   [Read me before you install](install.md#read-me-before-you-install). For a stable
   release, first merge `beta` into `release`.
3. **Check** from a clean, up-to-date checkout of that branch: every command in
   [CONTRIBUTING.md](../CONTRIBUTING.md#4-checks), plus a manual pass on Apple silicon with
   macOS 14 or later (permissions, Cleanup move and restore, an update from the previous
   version).
4. **Build and sign**, from `apps/desktop`, typing the key's password without echo:
   ```sh
   read -s TAURI_SIGNING_PRIVATE_KEY_PASSWORD && export TAURI_SIGNING_PRIVATE_KEY_PASSWORD TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/kleari-updater.key"
   APPLE_SIGNING_IDENTITY="Kleari Self-Signed" pnpm tauri build --config src-tauri/tauri.release.conf.json
   unset TAURI_SIGNING_PRIVATE_KEY_PASSWORD TAURI_SIGNING_PRIVATE_KEY
   pnpm tauri signer sign --app-version <version> -f ~/.tauri/kleari-updater.key src-tauri/target/release/bundle/macos/Kleari.app.tar.gz
   ```
   The second signature binds the version (`requireSignedVersion`); an update signed without
   `--app-version` is refused by every installed copy.
5. **Verify**: `codesign --verify --deep --strict` on `Kleari.app` and the DMG, signed by
   `Kleari Self-Signed`, identifier `dev.bakare.kleari`, and the version in `Info.plist`; the
   update signature's key ID matches `kleari-updater.key.pub`; the build commit is the
   branch's head.
6. **Publish** `latest.json` (version, notes, `pub_date`, and under `platforms.darwin-aarch64`
   the `.sig` contents as `signature` and the versioned asset URL of `Kleari.app.tar.gz` as
   `url`):
   - create the release `v<version>` on that commit, with the DMG, `Kleari.app.tar.gz` and
     `latest.json`: a **pre-release** for a beta, or a regular release marked **Latest** for
     stable;
   - replace `latest.json` on the `beta-channel` release with the same file
     (`gh release upload beta-channel latest.json --clobber`), creating `beta-channel` as a
     pre-release the first time.
7. **After a stable release**, fast-forward `main` to `release`, and merge `release` back into
   `beta` if it took a fix.
8. **Confirm**: download the assets back and compare them; check that an installed copy finds
   the update on its channel.

## Downloads

Kleari has no account and no telemetry, so downloads are counted only from GitHub's
public counters, which identify no one:

```sh
gh api repos/bakare-dev/kleari/releases --paginate \
  --jq '.[] | {release: .tag_name, assets: [.assets[] | {name, downloads: .download_count}]}'
```

- A DMG's count is how many times that version was downloaded: new installs and manual
  updates.
- `Kleari.app.tar.gz` counts in-app updates installed.
- `latest.json` counts update checks: every check downloads it once, so it roughly reflects
  how often copies of Kleari are opened, not how many people use it.
- The repository's **Insights › Traffic** shows visitors and clones for the last 14 days,
  and **stars** show interest.
