# Reusable component inventory

Share components when they have a stable responsibility or repeated behavior. This list is
a design inventory, not a requirement to create every component immediately. Keep feature
composition screen-specific until a real reuse need exists.

| Component | Responsibility and states | Accessibility | Sharing guidance |
|---|---|---|---|
| `AppShell` | Window layout, current destination and appearance; normal/narrow/loading shell | Main/aside/nav landmarks; responsive focus continuity | Shared root composition |
| `Sidebar` | Destination navigation and selected item; expanded/collapsed/active | Link semantics, `aria-current`, keyboard operation and named collapse control | Shared main-window navigation |
| `TopBar` | Page context, title and screen-level controls; stable/loading | Heading hierarchy, labeled controls; no duplicated navigation | Shared frame; screen supplies actions |
| `MetricCard` | One value, unit, source and freshness; available/stale/unavailable | Value not color-only; include unit in reading order | Share for consistent metric presentation |
| `StatusIndicator` | State label with icon/color; success/warning/error/unknown/stale | Text label and optional live announcement | Shared primitive |
| `StorageBreakdown` | Scope and categorized known/unknown capacity; loading/partial/empty/error | Text/table equivalent, legend and units | Storage feature component; avoid generic chart abstraction without reuse |
| `DataTable` | Dense sortable/filterable rows; loading/empty/error/selected/partial | Table or accessible grid semantics; keyboard sorting/selection | Share only with stable row/action contract; columns remain screen-specific |
| `FilterBar` | Active filters/count/clear; idle/active/empty | Labels, accessible names and announced result count | Share when filter semantics match |
| `Select` | A fixed list of choices as the native macOS pop-up, styled like the stepper; enabled/disabled | Native `<select>` with an `aria-label`; keyboard and VoiceOver as macOS provides | Shared: Settings › Scanning (schedule) and the Mouse page |
| `SearchField` | Labeled query, clear and focus; idle/query/loading/no-results | Programmatic label, named clear button, keyboard focus | Shared primitive |
| `EmptyState` | Explain why no content exists and next safe action | Heading and descriptive text; clear action label | Shared pattern; screen owns explanation |
| `ErrorState` | State what failed, what remains valid and safe recovery | Appropriate status announcement; no color-only meaning | Shared layout; screen owns recovery action |
| `LoadingState` | Communicate pending work and scope; indeterminate/determinate | `aria-busy`, progress semantics and text description | Shared primitive |
| `ProgressIndicator` | Operation progress/scope/supported cancel; active/canceling/partial/failed | Correct progress ARIA values; announce meaningful changes | Shared presentation; feature owns cancellation contract |
| `RiskBadge` | Backend-supplied risk, distinct from eligibility; levels/unknown | Text and separate eligibility label | Share only after risk taxonomy is agreed; never authorizes action |
| `ConfirmationDialog` | Explain exact effect and collect approval; review/blocked/confirming/result | Dialog label, focus containment/return, safe focus, explicit buttons | Share mechanics; operation owns safety copy/requirements |
| `MenuBarPopover` | Compact current status and main-window entry; open/closed/unavailable | Anchor, focus return, Escape/outside dismissal, keyboard flow | Separate Tauri surface; share tokens/status where useful |
| `ActivityTimeline` | Chronological outcomes and recovery state; empty/partial/error | Ordered-list/time semantics and readable outcome | Share only where event model is stable |

Use native buttons, links, inputs and semantic sections before wrapping them in a custom
abstraction. A component may remain screen-specific when behavior, data contract or
accessibility needs differ. Do not create placeholders just to make this inventory appear
implemented.
