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

## Amendment: alert groups, more checks, and the menu

- **Thresholds and checks** (`assess`, pure and unit-tested): the startup disk is low below 15%
  free or 15 GB, and critical below 5%; memory pressure warning is shown and notified once it
  lasts two checks, critical at once; the processor is busy when the 15-minute load average
  (`getloadavg`) reaches 90% of its cores; the Mac's battery is low at 20% and critical at 10%
  on battery, hot from 45 °C (`AppleSmartBattery` `Temperature`), near its life at 90% of
  `DesignCycleCount9C`, and needs service when macOS says so; more than 15 login items and
  agents is "many"; no Cleanup scan for 7 days is a reminder.
- **Battery health is macOS's own figure.** "Maximum Capacity" and "Condition" come from
  `system_profiler SPPowerDataType -json -detailLevel mini`, run like `ps` (ADR-010): an
  absolute path, fixed arguments, no shell, an empty environment, a time limit and bounded
  output parsed as untrusted JSON. The IORegistry capacities can read several percent higher
  than macOS's figure, so they are only the fallback.
- **One switch per group** in Settings › Notifications › Alerts: Storage, Performance,
  Battery, Login items, Reminders (`notify_storage` … `notify_reminders`). A group never saved
  follows the old single switch, so turning alerts on before keeps every group on.
- **Notified:** each alert at most once a day (reminders, many login items and cycle counts
  once a week); battery low, critical, hot and critical memory again as soon as they come back
  after clearing. Also, without being an issue: a login item or agent that wasn't there at the
  previous check, and the battery becoming full while plugged in. The battery's charge is read
  every minute while a group is on, everything else every 15 minutes.
- **The menu-bar popover** shows the same alerts (two, and how many more are on the
  Dashboard) and the network's download and upload rates, in place of the latest activity.
- **Not offered**, for want of a public API or because it would need the network, Location or
  Full Disk Access: app updates, internet or VPN state, Wi-Fi strength, per-app bandwidth, apps
  not responding, permission changes, iPhone backups, and chip temperatures.
