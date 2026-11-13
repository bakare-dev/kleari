# ADR-021: Issue alerts

- **Status:** Accepted, by the product owner's decision.
- **Context:** People asked to be told about battery drain, heavy memory use, hung apps, low
  disk and external-drive space, a full Trash, low Bluetooth device batteries, and apps worth
  uninstalling.
- **Decision:**
  - **Checks** (`issues.rs`), read-only and local: free space of the startup disk (below 10% or
    15 GB) and of local drives in `/Volumes` (below 10%), via `statfs`; memory pressure
    (`kern.memorystatus_vm_pressure_level`: warning shown, critical notified); battery drain
    above 15 W while discharging (`AppleSmartBattery` `InstantAmperage` × `Voltage`); Bluetooth
    devices at 20% or less (`AppleDeviceManagementHIDEventService` `BatteryPercent` and
    `Product`); a Trash of 5 GB or more; and up to ten apps (not Apple's) Spotlight says
    weren't opened for 180 days (`MDItemCopyAttribute` `kMDItemLastUsedDate`).
  - **Shown** on the Dashboard when it opens, each with a link to where to deal with it.
  - **Notified** while Kleari runs, every 15 minutes, only warnings, each at most once a day,
    and only with Settings › Notifications › "Something needs attention" on (off by default,
    like the other notifications).
  - **Hung apps aren't detected:** macOS has no public way to tell an app has stopped
    responding; the page doesn't pretend.
- **Consequences:** A background thread wakes every 15 minutes while Kleari runs; when the
  setting is off it reads only the preferences. Nothing is stored or sent.
