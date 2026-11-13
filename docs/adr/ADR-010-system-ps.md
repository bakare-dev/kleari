# ADR-010: Reading other accounts' process figures through Apple's `ps`

- **Status:** Accepted (used by the Processes page)
- **Context:**
  - macOS doesn't give an unprivileged app the memory, CPU time or owner of processes that
    belong to root or other system accounts; `sysinfo` reports them as 0. On the development
    Mac that was about 200 of 750 processes, including WindowServer.
  - No permission an app can request unlocks this. Full Disk Access doesn't cover it, and
    macOS has no prompt for it.
  - Activity Monitor reads these figures through a root helper. `/bin/ps` reads them because
    it is installed setuid root on the sealed system volume.
  - The product owner chose to use `ps` rather than leave these processes unlisted or install
    a privileged helper. A helper would conflict with ADR-004 (no root daemon).
- **Decision:**
  - On macOS only (the module and its call are compiled for macOS only), when `sysinfo`
    reports a process with no memory value, the sampler runs `/bin/ps` once per sample and
    takes those processes' resident memory, cumulative CPU time and UID from it. Other
    targets keep their own readings.
  - **Boundary:**
    - Absolute path; fixed arguments (`-ax -o pid=,uid=,rss=,time=`); no shell.
    - An empty environment except `LC_ALL=C`; no input; stderr discarded.
    - A 1.5 s time limit, after which `ps` is killed and the sample is used without it.
    - At most 1 MiB and 20,000 lines of output.
  - **Parsing:** output is parsed as untrusted text, and a malformed line is skipped, never
    guessed. Only processes `sysinfo` already listed are filled in, matched by PID within the
    same sample.
  - **Read-only:** nothing is written, stored or logged, and `ps` gets no input from the
    frontend.
- **Consequences:**
  - The list covers every process. On the development Mac it went from about 200 unreadable
    to 0.
  - **Cost:** Kleari now runs an external program. It is Apple's own, fixed and
    argument-only, but it is a new kind of dependency. A future macOS could change `ps`'s
    output format or privileges; the parser then skips the changed lines and the page falls
    back to counting those processes as having no memory value.
  - **PID reuse:** a PID could be reused between `sysinfo`'s read and `ps`'s, about a
    millisecond apart. A process that briefly showed another's figures would affect only
    what is displayed for one sample, never an action. Quit uses its own identity checks
    (ADR-009).
  - Thread counts for these processes still need privileges, so they show "Not available".
  - Every other module keeps the no-external-programs rule. Any further use needs its own
    decision.
