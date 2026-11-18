# ADR-023: Chip temperatures and fan speeds

- **Status:** Accepted, by the product owner's decision.
- **Context:** The System page's thermal reading showed only macOS's thermal *state*
  (`NSProcessInfo.thermalState`), which reads "Nominal" almost always, and then the battery
  sensor's temperature: neither is the processor's temperature. People asked for the
  processor's temperature and the fans. macOS has no public API for either on Apple silicon.
- **Decision:**
  - **Read-only, through private interfaces**, as Activity Monitor-style tools do
    (`system_monitor/sensors.rs`):
    - temperatures from `IOHIDEventSystemClient`: every service whose primary usage page and
      usage are Apple's temperature sensor (`0xff00` / `5`), read as a temperature event. The
      processor's temperature is the average of its cluster sensors (`pACC`/`eACC MTR`), or
      of its die sensors (`tdie`) on chips that name none (the M4); the hottest is shown too.
      Graphics sensors are shown where the chip names them. Readings outside 5–130 °C (an
      unconnected sensor) are ignored;
    - fans from the `AppleSMC` user client: `FNum`, and each fan's `F<n>Ac`, `F<n>Mn` and
      `F<n>Mx`, with the SMC's read command only (`kSMCReadKey`). The 80-byte record layout is
      checked in a test.
  - **Shown** on the System page (the Temperature gauge leads with the processor, and a
    Cooling health check gives the fans and the hottest sensor) and in the menu-bar popover's
    CPU tile, sampled with the other live readings, only while one of them is on screen.
  - **Fans are never controlled.** Writing SMC keys needs root, which would mean a privileged
    helper; CONTRIBUTING rules that out, and turning a fan off risks the hardware.
  - **Nothing guessed:** a sensor or fan that can't be read is reported as not reported; the
    battery sensor and then the thermal state are the fallbacks.
- **Consequences:** A macOS update can rename sensors or change the SMC, and the readings then
  disappear (never show wrong values silently, as far as the plausibility checks can tell).
  Sensor names are matched by what they contain, so a new chip may need its names added. No
  new permission, privilege or network access; nothing is stored or sent.
