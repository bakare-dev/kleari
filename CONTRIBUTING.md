# Contributing to Kleari

Thank you for helping. Kleari moves people's files and reads a lot about their Mac, so every
change, from anyone (people and AI agents alike), follows the rules below. A pull request
that doesn't follow them is asked to, or closed. They exist so users can trust Kleari.

## 1. Before you start

- **Open or pick an issue first** for anything beyond a typo. Say what problem it solves, what
  is in and out of scope, and any safety or privacy risk. Wait for a maintainer to agree
  before large work.
- **Read what applies:** the [product requirements](docs/product-requirements.md),
  [architecture](docs/architecture.md), [threat model](docs/threat-model.md),
  [privacy](docs/privacy.md) and the [decisions](docs/adr/) touching your area. For UI work,
  [the theme](docs/design/THEME.md) and the [design specifications](docs/design/README.md);
  reuse existing tokens and components, and don't redesign unrelated screens.
- **Security issues are never reported in public issues or PRs.** See [SECURITY.md](SECURITY.md).

## 2. Branches

Three protected branches; nobody pushes to them directly, and none is force-pushed or deleted.

| Branch | Holds | Receives |
|---|---|---|
| `main` | development: every contribution lands here first, so it can hold unreleased work | pull requests from any branch or fork |
| `beta` | the next beta; beta releases are built from it | a pull request from `main`, by a maintainer, when a beta is cut |
| `release` | the latest stable release's code; stable releases are built from it | a pull request from `beta`, by a maintainer, when a beta is promoted |

Work on your own branch (in a fork, or a branch named `feat/…`, `fix/…`, `docs/…` or
`chore/…`) cut from `main`, and open your pull request **into `main`**, GitHub's default.
The required **Branch flow** check rejects any other route into `beta` or `release`.

Maintainers only: an urgent fix to a stable release goes from a `hotfix/…` branch into
`release`, and a stable release's version change from a `release-prep/…` branch; `release` is
then merged back into `main`, and reaches `beta` with the next promotion. Promotions
(`main` → `beta`, `beta` → `release`, `release` → `main`) use **Create a merge commit**, never
squash or rebase, so the branches keep sharing history.

## 3. Rules every change follows

**Safety (non-negotiable).**
- Scanning only reads. Nothing changes, moves or deletes a user's file except Cleanup's move
  into quarantine, Quarantine's restore and its separately confirmed permanent delete, and
  Apps moving a confirmed app to the Trash through macOS
  ([ADR-017](docs/adr/ADR-017-apps-and-leftovers.md)), and Maintenance's fixed tasks, each
  confirmed and run through macOS's own administrator prompt
  ([ADR-020](docs/adr/ADR-020-maintenance-tasks.md)).
- Rust decides everything that touches files; the interface never sends a path to act on.
- Never follow symlinks out of scope, never overwrite on restore, and fail closed when unsure.
- No root helper, no broad new permission, no shell command built from data. Administrator
  rights only through macOS's own prompt, per action, for a constant script (ADR-020).

**Privacy (non-negotiable).**
- Nothing about the user's Mac or files leaves it. No telemetry, analytics, crash upload,
  account or new network request. The update check to Kleari's own releases is the only
  internet access; adding any other needs a decision record (ADR) and the maintainer's
  agreement first.
- Never log or store file contents, credentials or more paths than a feature needs.

**Code.**
- Keep to one objective per PR, and the existing patterns. Don't refactor or reformat
  unrelated code, and don't add dependencies without saying why.
- TypeScript: no `any`, no unchecked casts. Rust: no `unwrap`, and `expect` outside tests
  only for a stated invariant that can't fail; no `unsafe` without a `SAFETY:` comment;
  clippy clean with warnings as errors.
- No placeholders, stubs or fake data presented as working, and no errors swallowed without
  a stated reason.
- Rust changes to anything guarding files, permissions or user data come with a test, on
  disposable temporary folders only, never real user files.

**Docs.**
- User-visible changes get an entry under `Unreleased` in [CHANGELOG.md](CHANGELOG.md)
  (Added, Changed, Deprecated, Removed, Fixed or Security), in plain words, describing only
  what is implemented and checked.
- Keep [privacy](docs/privacy.md), the [threat model](docs/threat-model.md) and any ADR you
  affect true.

## 4. Checks

Every one must pass on your machine and in CI:

```sh
pnpm install --frozen-lockfile
pnpm typecheck && pnpm lint && pnpm build && pnpm check:release-bundle
cd apps/desktop/src-tauri
cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked
```

Report honestly in the PR what you ran and what you couldn't (for example, behaviour that
only shows on real hardware).

## 5. Pull requests

- Fill in the [pull request template](.github/pull_request_template.md): what and why, user
  impact, scope, exact checks and results, and safety, privacy and compatibility effects.
- Include screenshots for visible UI changes.
- Commit messages: `type: summary` in the imperative (`feat`, `fix`, `docs`, `chore`,
  `refactor`), for example `fix: keep the review's order after a restore`.
- Aim for a few hundred changed lines. Above roughly 500, split it, or explain why it can't be
  split and in what order to review it.
- CI must be green, every review conversation resolved, and a code owner
  ([CODEOWNERS](.github/CODEOWNERS)) must approve. Pushing new commits after approval
  dismisses it, so the final version is what gets approved. Only maintainers merge, and
  auto-merge is never used.

## 6. Versions and releases

Anyone can contribute code; **releases are made by the maintainer**, who also creates release
tags and promotes `main` to `beta` and `beta` to `release`. Versions follow [docs/versioning.md](docs/versioning.md);
releases follow the [release checklist](docs/release-checklist.md).

## 7. AI-assisted contributions

You may use AI tools, but you are responsible for every line: review it, test it and follow
these rules as if you wrote it. Don't submit generated changes you don't understand.

## 8. Conduct

Be kind and assume good faith. Harassment or abuse of any kind isn't tolerated, and leads to
removal from the project.
