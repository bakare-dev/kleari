# ADR-016: Mouse module

- **Status:** Accepted, by the product owner's decision. It widens
  Kleari's scope beyond maintenance to input settings, at the owner's request.
- **Context:** macOS has one scroll direction for the trackpad and a mouse, so someone who
  wants natural scrolling on the trackpad gets it on a mouse wheel too; it also offers little
  for extra mouse buttons. The product owner asked for a module that fixes both: a mouse wheel
  that scrolls the classic way, wheel speed and smooth scrolling, extra buttons mapped to
  actions or keyboard shortcuts, and a button or modifier held to make the wheel do something
  else.
- **Decision:**
  - **Event tap** (`mouse/tap.rs`, CoreGraphics `CGEventTapCreate`, session tap, head insert,
    active). It asks only for scroll-wheel and extra-button (`otherMouse` down, up, dragged)
    events, and left- or right-click events each only while a combination uses that click (the
    tap is replaced when that changes). A replacement, and turning the settings
    off, happen only while no press is held or held back (tried again every 200 ms, at most
    10 s, then regardless), so no press is split from its release: checked under the engine's
    lock, which the tap takes for each event, and the old tap then passes every event untouched
    until it is gone. While a replacement waits, the old tap keeps its settings and the clicks
    it sees; the new settings take effect with the new tap, never half in the old one. While
    turning off (or Accessibility being withdrawn) waits, the tap still sees its event types
    but changes nothing: new presses pass untouched at once, and a press held back or replaced
    before still has its drag and release dropped. Each try reads the latest settings under
    the tap's lock, so a retry never applies settings a later change replaced.
    It never sees keystrokes or pointer movement.
    Nothing it sees is stored, logged or sent. It runs on its own thread with its own run loop, checked
    four times a second for a stop, and for Accessibility having been withdrawn (then the
    settings are put in effect again, which drains and stops it, asked again at each wake until
    that gets under way; the access answer acted on is the one the page is shown); a tap macOS turns off for being slow is turned back on.
  - **Permission.** macOS allows an active tap only once the user turns Kleari on under
    Privacy & Security › Accessibility (`CGPreflightPostEventAccess`). The Mouse page explains
    what is seen and why before asking (`CGRequestPostEventAccess`, macOS's own prompt the first
    time), offers the System Settings pane, and notices by itself (every 2 s while waiting, and
    when the window comes forward) once it is allowed. Without it nothing is changed. While the
    page is open it also reads again whenever the window comes forward, so access withdrawn in
    System Settings shows on return.
  - **Only a mouse wheel.** An event macOS marks continuous (a trackpad, or Apple's Magic
    Mouse) passes untouched, so the trackpad keeps its direction. A stepped wheel is reversed and
    scaled in place (its line, fixed-point and point deltas), or, with smooth scrolling, dropped
    and replaced by a glide of pixel scroll events (about 120 a second, easing out).
  - **Buttons** 3 to 8 (macOS's 2–7) can do: Back (⌘[), Forward (⌘]), Mission Control (⌃↑),
    App windows (⌃↓), previous or next Space (⌃← / ⌃→), a middle click, nothing, or a keyboard
    shortcut recorded on the page (61 keys; positional key codes, so on a non-ANSI layout a
    letter is the key in that position), or a media key: Play/Pause, Next or Previous track,
    Volume up or down, Mute (posted as the system-defined events a Mac keyboard's media keys
    send). Left and right click aren't set up as buttons (they can be part of a combination,
    below). A mapped button acts on press; its drag and
    release are dropped.
  - **Combinations** (up to 8): up to two of buttons 3–8 (or left or right click
    with one of them, below) and any modifier keys,
    held together. Each either works **while scrolling** (each wheel step switches Spaces,
    zooms (⌘= / ⌘-), switches tabs (⌃⇥ / ⌃⇧⇥) or scrolls sideways) or **when pressed** (the
    last button pressed completes it, with the keys held at that moment, and it does any button
    action once). A wheel combination needs a button or a key; a press needs two buttons, or
    one with keys (one button alone is a button setting). The same buttons and keys can have
    one of each kind. Matching is exact: ⌥⇧ is not ⌥. A button in any combination waits for its
    release: if no combination was completed and the wheel wasn't turned while it was held, it
    then does its own click or setting. The page explains why an incomplete combination isn't
    saved yet, and flags one using a button the finder has never seen (remembered in the page's
    own storage).
  - **Left or right click in a combination** (by the product owner's decision,
    reversing "never"): either can be one of a combination's two buttons, always with one of
    buttons 3–8, never alone and never both. Left click always reaches the app (press, drag
    and release), since holding it back would break dragging, selecting and double-clicks;
    held, it counts towards a combination, and the other button pressed completes a press
    combination. Right click, while part of a combination, is held back: its drag is dropped,
    and on release, if no combination was completed and the wheel wasn't turned, a right click
    is posted there, with the modifier keys held at release (Caps Lock and Fn included) and a
    click count of one (so its
    menu opens on release instead of press, and ⌘-right-click stays one). A button 3–8 held first
    and then left or right clicked doesn't complete a press combination. Neither can be set up
    as a button of its own, and neither is flagged as unseen.
  - **Events Kleari posts** (keys, clicks, glides, media keys) carry a tag in the event's
    source-user-data field and are let through by the tap. A failure while deciding passes the
    event through unchanged (`catch_unwind`), so the mouse never stops working.
  - **Button finder** (`mouse/probe.rs`), so the user can tell which button is which before
    setting anything up. An in-app `NSEvent` local monitor, installed at launch, sees mouse
    presses, releases and scrolls inside Kleari's own windows only; it needs no permission and
    never sees keystrokes. It reports to the page (`mouse-probe` event) only while the Mouse
    page is open (`listen_mouse_probe`): the button as macOS numbers it, the wheel's physical
    direction (natural scrolling undone) and whether it scrolls in steps or smoothly. A drawn
    mouse on the page lights the matching part; clicking a part goes to its setting. Only which
    buttons it has seen is kept, in the page's own storage, to flag settings a mouse can't
    trigger.
  - **Testing in the finder's box.** While the pointer is over the box, the page says so
    (`capture_mouse_probe`, renewed every second; it lapses on its own 2 s after the last
    renewal, and ends when the pointer leaves, the window loses focus or the page closes).
    Meanwhile the middle and side buttons, the wheel and right click only light up the
    drawing: the monitor drops them before Kleari's window acts on them (no Back/Forward, no
    scrolling, no menu). Left click still works, to choose a part. The event tap passes every
    event through untouched, so the drawing shows the real buttons even with the settings on;
    they apply again as soon as testing ends. Presses and releases stay paired across testing:
    a button released during testing ends its own pairing (`Engine::released`); a button
    pressed before testing and released after it stays paired, so its release is still
    dropped; and a button pressed during testing is recorded only where it was
    dropped, by the in-app monitor (`probe::dropped_press`), so a press another app received
    is never recorded; that button's drag and release after testing are then dropped too, by
    the tap wherever the pointer is, or by the monitor in Kleari's window when the tap isn't
    running. A new press of the button clears the record. Dropping such a release also
    ends the button's pairing in the engine (`Engine::released`, without its usual release
    action), in case the tap had handled the press just before testing began. No app gets a release without its
    press because of testing. Testing
    is process-wide, not tied to where the pointer is: if leaving the box went unnoticed, the
    settings pause everywhere for at most the 2 s it takes to lapse.
  - **Decisions are pure** (`mouse/engine.rs`) and unit-tested; `tap.rs` only converts events.
  - **Settings** are one validated record (`mouse.v2` in the `setting` table; an older
    `mouse.v1` record, whose combinations were one button or one set of keys, is read and
    carried over until the next save writes `mouse.v2`), off until the user turns them on. A stored value
    the page doesn't offer replaces the whole record with the defaults. They apply at launch
    and on each save, while Kleari runs. The page saves one change at a time, each applied
    to what is saved when its turn comes (a combination named by its content, not its row),
    and shows the saved settings with the changes still waiting; a change that fails drops out
    with the reason and is never carried into a later one (`mouseSaves.ts`). Reads (each visit,
    the Accessibility checks) take their turn with the saves, so none returns from before a
    later save. Combination
    changes (edits, removals, additions) wait in the app (`useMouseCombos`, so they still save
    after leaving the page), kept by the combination they change, and save one at a time, each
    worked out from the combinations as saved once the one before has finished: an edit when it
    is complete and repeats no other combination. A change made while one saves moves back onto
    the combination if that save fails; a failure while elsewhere is shown on return.
  - **Status** at the top of the page: On now / Off / Waiting for Accessibility / macOS didn't
    start them. With the settings on, it reminds that they work only while Kleari runs:
    closing the window quits Kleari when Show in menu bar is off, and after a restart they
    stay off unless Open at login is on (with a way to Settings). It also lists buttons the
    settings use that the finder has never seen.
  - **Commands:** `get_mouse`, `set_mouse`, `request_mouse_access`, `listen_mouse_probe`,
    `capture_mouse_probe` (default capability only);
    the Accessibility pane joins the fixed System Settings panes.
- **Consequences:**
  - Kleari asks for Accessibility, a broad permission: it could post keyboard events. It
    posts only the fixed shortcuts above or the one the user recorded, in response to the
    user's own mouse input. Accessibility is granted to Kleari's code signature; a
    self-signed release keeps it across updates, an ad hoc (development) build may not.
  - Every wheel and extra-button event passes through Kleari while the module is on; a
    stalled tap would delay them until macOS turns it off.
  - Turning off the settings, or a change of which clicks the tap sees, stops the running tap
    (once no press is held, at most 10 s) and waits, at most 2 s, for its thread to remove it.
    A change while a button is held therefore takes effect when it is let go. A press whose
    release never comes (none should) holds a change back for the 10 s at most. Two taps never run at once: if the old one hasn't finished by then, no
    new one starts, the page says the settings aren't running, and the old one is asked again
    on the next change or read.
  - macOS's own shortcuts for Mission Control and Spaces must be enabled for those actions.
  - Keyboard shortcuts captured on the page are those the window receives; one macOS or the
    app menu takes first (such as ⌘Q) can't be recorded.
  - Not built: per-app settings, cursor speed, and a Magic Mouse direction of its own.
