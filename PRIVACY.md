# Privacy

**Your Mac's data stays on your Mac.** Kleari has no account, no analytics and no telemetry,
and no server of its own. It uses the internet for one thing only: checking for updates and, when
you choose Install, downloading one.

## What Kleari reads, and what happens to it

Kleari reads what it needs to show you your Mac: system readings (CPU, memory, battery and so
on), the names, sizes and dates of files and folders it scans, the apps you have installed, and
your settings. All of it stays on your Mac:

- **Shown on screen,** and kept in memory while Kleari is open.
- **Kept on disk only where it has to be:** your settings, the activity log, and what Kleari moved
  into quarantine, with where it came from so it can be put back. These are kept in files only
  your account can read, in `~/Library/Application Support/` under Kleari's folder.
- **Never sent anywhere:** not your files, their contents, their names or anything about your Mac.

Kleari opens a file's contents only where a feature needs them, and
[docs/privacy.md](docs/privacy.md) names each one. Some examples:

- an app's `Info.plist`, for its name;
- launch agents' settings, for the program each starts;
- a Git repository's last commit time, for Developer;
- when you look for duplicates or similar pictures, the files being compared, only to compare
  them. Nothing about them is kept.

## The internet

| When | What Kleari contacts | What it sends |
|---|---|---|
| At launch, unless you turn off Settings › About › Check for updates automatically, and when you choose Check now | Kleari's release feed on GitHub | A plain request for the feed file. Like any download, it carries your IP address |
| Only when you choose Install | The update file on GitHub | The same kind of request |

That is all. Every address, how the code is kept to it, and how you can check it yourself with a
network monitor are in [docs/network.md](docs/network.md).

## Permissions

macOS asks before Kleari can use Full Disk Access, Accessibility, folder access, notifications or
browser tabs. Each is optional and is used only by the feature that needs it. You can turn any of
them off in System Settings › Privacy & Security.

## More detail

- [docs/privacy.md](docs/privacy.md) lists exactly what each screen reads and keeps.
- [docs/network.md](docs/network.md) lists every internet connection.
- [DISCLAIMER.md](DISCLAIMER.md) explains the risks of what Kleari can do.

This describes the current code. If it ever changes, this page changes with it.
