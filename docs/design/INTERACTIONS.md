# Interaction specifications

## Navigation and sidebar

Use the persistent main-window sidebar for top-level destinations, with text and icon,
visible selected state and keyboard reachability. Navigation changes context without
starting a scan or action. Preserve filters only while their scope remains valid. When
space is constrained, collapse deliberately with an accessible toggle and retain a visible
way to identify the current destination.

## Menu-bar behavior

Clicking the menu-bar item opens one compact popover anchored to it; Escape or clicking
outside dismisses it and focus returns to the anchor. An action inside must not close the
popover before its outcome is known. The popover surfaces current state, not fabricated
background activity. It must not expose a destructive shortcut.

## Hover, focus and selection

Hover is supplemental feedback only. Keyboard focus is always visible and independent of
hover. Row hover, keyboard focus and persistent selection use distinct treatments. Selecting
a row only changes selection/details; it never executes an operation. Controls remain
discoverable to keyboard users without hover.

## Search, filtering, sorting and selection

Search updates a labeled result set, supports clearing, and does not silently change scan
scope. Filters show active count and can be reset; an empty result distinguishes no matches
from no data. Sorting announces the active column and direction. Multi-selection exposes a
selection count and clears when results are invalidated. Only backend-eligible items can be
included in an operation preview; the UI does not infer eligibility from category or risk.

## Keyboard shortcuts

Use standard macOS conventions where they do not collide with system behavior:

| Shortcut | Proposed action |
|---|---|
| `⌘,` | Open Settings |
| `⌘F` | Focus the current screen's search field when present |
| `⌘R` | Refresh current read-only view when supported |
| `Escape` | Clear transient selection or dismiss the topmost dismissible overlay |
| `⌘1` … `⌘9` | Optional direct navigation only if destinations and discoverability are stable |

Do not assign a keyboard shortcut to permanent deletion. Any shortcut for an operation must
still enter the same explicit preview and approval flow as pointer interaction.

## Contextual menus

Menus open from the focused/selected item, list actions with verbs, and preserve standard
keyboard navigation. Opening a menu or highlighting an item is never approval. Destructive
actions are not one-step menu commands. If a context action begins a review, it opens a
preview that states the full effect and requires separate confirmation.

## Dialogs and sheets

Use a dialog for a focused decision and a sheet for a contextual, multi-step review where
the parent relationship matters. Keep the background visible but inert. Initial focus goes
to the title/least risky useful control; destructive buttons are not focused by default.
Escape/cancel dismisses before approval; after an operation begins, cancellation uses the
operation's supported semantics and reports partial completion. Return focus to the
initiating control after dismissal.

## Progress, cancellation and failure

Show what is being read or changed, current scope and whether progress is determinate. Make
cancellation available only when the operation can safely honor it. On cancellation or
failure, report completed, skipped and unresolved work; never label partial completion as
success. Retain valid results and mark stale data. A retry is a new explicit action and
must be idempotent or guarded against duplicate effects.

## Toasts and notifications

Use in-app status messages for transient confirmation; persistent operation results belong
in activity history. Announce important status to assistive technology. Notifications are
optional and permission-aware; do not include sensitive full paths. Do not use timeouts to
remove a safety-critical error before it is read.

## Appearance and accessibility preferences

Follow system light/dark appearance by default; change semantic tokens without changing
meaning, hierarchy or status. Under reduced motion, replace animated transitions with
immediate state changes or simple opacity changes. Under reduced transparency, use opaque
surfaces and retain clear borders. Focus indicators, hit targets and keyboard behavior
remain consistent in all appearances.

## Scan, approval, quarantine, restore and deletion

1. A scan is read-only, scope-explicit and cancellable where supported. Results show skipped
   locations and freshness.
2. The user reviews specific findings and opens a preview. The preview names effects,
   evidence, risk, uncertainty, policy eligibility and recovery limits.
3. Approval is deliberate and applies to that exact preview. Backend revalidation occurs
   immediately before operation; stale preview, changed identity, revoked permission or
   changed policy fails closed.
4. Quarantine is a separate operation and preferred where feasible. Failure never
   transitions to permanent deletion. Partial status and recovery are visible.
5. Restore checks for destination conflicts and never overwrites. A conflict is reported for
   user resolution.
6. Permanent deletion is a separate, explicit and clearly named flow after identifying the
   exact affected items. It is never triggered by hover, row selection, context-menu opening,
   keyboard focus or a default action.
