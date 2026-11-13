//! What the Mouse module does with each mouse event, decided without touching macOS so it can
//! be tested: the event tap (`tap.rs`) turns events into `Input`s and carries out the `Output`.
//!
//! Only a wheel that scrolls in steps (a mouse) is changed. Continuous scrolling — a trackpad,
//! or Apple's Magic Mouse — passes untouched, so the trackpad keeps its own direction.

use super::config::{key_code, ButtonAction, Modifiers, MouseSettings, ScrollAction, LEFT, RIGHT};

/// Points one wheel step scrolls at 100 % when gliding; macOS's own step is about this.
pub const STEP_POINTS: f64 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    /// A wheel event: lines up (+) / down (−) and left (+) / right (−), and the modifiers held.
    Scroll {
        continuous: bool,
        vertical: i64,
        horizontal: i64,
        modifiers: Modifiers,
    },
    /// A button pressed, with the modifier keys held at that moment.
    Down {
        button: u8,
        modifiers: Modifiers,
    },
    Up(u8),
    Dragged(u8),
}

/// The media keys on a Mac keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Media {
    PlayPause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Mute,
}

/// A key press Kleari sends: a macOS key code with its modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keys {
    pub code: u16,
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    Keys(Keys),
    /// Press and release a media key.
    Media(Media),
    /// Press and release this button where the pointer is (1 is right click).
    Click(u8),
    /// Press (`true`) or release ⌘ on its own, for the app switcher.
    Command(bool),
    /// Scroll by points: up (+) / down (−), left (+) / right (−).
    Scroll {
        vertical: f64,
        horizontal: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Leave the event as it is.
    Pass,
    /// Drop the event.
    Suppress,
    /// Change the wheel event in place: reverse either direction and scale its steps.
    Adjust {
        reverse_vertical: bool,
        reverse_horizontal: bool,
        percent: u16,
    },
    /// Drop the event and glide by these points instead.
    Glide { vertical: f64, horizontal: f64 },
    /// Drop the event and do these instead.
    Act(Vec<Effect>),
}

const fn keys(code: u16, command: bool, shift: bool, option: bool, control: bool) -> Effect {
    Effect::Keys(Keys {
        code,
        modifiers: Modifiers {
            command,
            shift,
            option,
            control,
        },
    })
}

const LEFT_BRACKET: u16 = 33;
const RIGHT_BRACKET: u16 = 30;
const EQUAL: u16 = 24;
const MINUS: u16 = 27;
const TAB: u16 = 48;
const GRAVE: u16 = 50;
const LEFT_ARROW: u16 = 123;
const RIGHT_ARROW: u16 = 124;
const DOWN_ARROW: u16 = 125;
const UP_ARROW: u16 = 126;

/// What a mapped button does, as effects. Unknown shortcut keys (never saved) do nothing.
pub fn effects(action: &ButtonAction) -> Vec<Effect> {
    match action {
        ButtonAction::Back => vec![keys(LEFT_BRACKET, true, false, false, false)],
        ButtonAction::Forward => vec![keys(RIGHT_BRACKET, true, false, false, false)],
        ButtonAction::MissionControl => vec![keys(UP_ARROW, false, false, false, true)],
        ButtonAction::AppWindows => vec![keys(DOWN_ARROW, false, false, false, true)],
        ButtonAction::PreviousSpace => vec![keys(LEFT_ARROW, false, false, false, true)],
        ButtonAction::NextSpace => vec![keys(RIGHT_ARROW, false, false, false, true)],
        ButtonAction::MiddleClick => vec![Effect::Click(2)],
        ButtonAction::Disabled => Vec::new(),
        ButtonAction::Shortcut { shortcut } => key_code(&shortcut.key)
            .map(|code| {
                vec![Effect::Keys(Keys {
                    code,
                    modifiers: shortcut.modifiers,
                })]
            })
            .unwrap_or_default(),
        ButtonAction::PlayPause => vec![Effect::Media(Media::PlayPause)],
        ButtonAction::NextTrack => vec![Effect::Media(Media::Next)],
        ButtonAction::PreviousTrack => vec![Effect::Media(Media::Previous)],
        ButtonAction::VolumeUp => vec![Effect::Media(Media::VolumeUp)],
        ButtonAction::VolumeDown => vec![Effect::Media(Media::VolumeDown)],
        ButtonAction::Mute => vec![Effect::Media(Media::Mute)],
        ButtonAction::LastApp => vec![keys(TAB, true, false, false, false)],
        ButtonAction::NextWindow => vec![keys(GRAVE, true, false, false, false)],
    }
}

/// What one wheel step does while a combination is held. `up` is the wheel turned up (or
/// left, for a sideways-only step).
fn combo_effect(action: ScrollAction, up: bool, lines: i64) -> Effect {
    match (action, up) {
        (ScrollAction::SwitchSpaces, true) => keys(LEFT_ARROW, false, false, false, true),
        (ScrollAction::SwitchSpaces, false) => keys(RIGHT_ARROW, false, false, false, true),
        (ScrollAction::Zoom, true) => keys(EQUAL, true, false, false, false),
        (ScrollAction::Zoom, false) => keys(MINUS, true, false, false, false),
        (ScrollAction::SwitchTabs, true) => keys(TAB, false, true, false, true),
        (ScrollAction::SwitchTabs, false) => keys(TAB, false, false, false, true),
        (ScrollAction::SwitchApps, true) => keys(TAB, true, true, false, false),
        (ScrollAction::SwitchApps, false) => keys(TAB, true, false, false, false),
        (ScrollAction::SwitchWindows, true) => keys(GRAVE, true, true, false, false),
        (ScrollAction::SwitchWindows, false) => keys(GRAVE, true, false, false, false),
        (ScrollAction::HorizontalScroll, _) => Effect::Scroll {
            vertical: 0.0,
            horizontal: lines as f64 * STEP_POINTS,
        },
    }
}

/// One frame of a glide: about a quarter of what is left, in whole points, at least one point
/// while a point or more remains. What it returns is subtracted from what is left.
pub fn glide_step(remaining: f64) -> f64 {
    if remaining.abs() < 1.0 {
        return 0.0;
    }
    let step = (remaining * 0.25).round();
    if step == 0.0 {
        remaining.signum()
    } else {
        step
    }
}

/// What is held between events. One per event tap.
#[derive(Debug, Default)]
pub struct Engine {
    /// Buttons that are part of a combination and held now, in increasing order. Their own
    /// click waits for their release.
    held: Vec<u8>,
    /// A combination happened while they were held (or the wheel turned), so releasing them
    /// doesn't click. Reset once none is held.
    used: bool,
    /// Buttons whose press was replaced; their drag and release are dropped too.
    swallowed: Vec<u8>,
    /// ⌘ is held for the app switcher; released when a held button is let go.
    switching: bool,
}

impl Engine {
    /// `button` was released while its events went past untouched (buttons tested in the Mouse
    /// page's finder). Its press, if it was held or swallowed, is matched by this release, so a
    /// later press starts afresh. Every other button keeps its pairing: one pressed before
    /// testing and released after it is still swallowed as usual. (A press made during testing is
    /// paired by the button finder, which knows whether Kleari's window dropped it: see
    /// `probe::dropped_press`.)
    pub fn released(&mut self, button: u8) {
        self.let_go(button);
        self.swallowed.retain(|swallowed| *swallowed != button);
    }

    /// No press is held or held back, so none waits for a release: the tap can be replaced
    /// without splitting a press from its release.
    pub fn idle(&self) -> bool {
        self.held.is_empty() && self.swallowed.is_empty() && !self.switching
    }

    /// `button` is no longer held; once none is, the next hold starts unused.
    fn let_go(&mut self, button: u8) {
        self.held.retain(|held| *held != button);
        if self.held.is_empty() {
            self.used = false;
        }
    }

    pub fn handle(&mut self, input: Input, settings: &MouseSettings) -> Output {
        if !settings.enabled {
            return self.off(input);
        }
        match input {
            Input::Scroll {
                continuous,
                vertical,
                horizontal,
                modifiers,
            } => self.scroll(continuous, vertical, horizontal, modifiers, settings),
            Input::Down { button, modifiers } => self.down(button, modifiers, settings),
            Input::Up(button) => self.up(button, settings),
            Input::Dragged(button) => {
                // Left click always reaches the app, so its drag does too.
                if button != LEFT
                    && (self.held.contains(&button) || self.swallowed.contains(&button))
                {
                    Output::Suppress
                } else {
                    Output::Pass
                }
            }
        }
    }

    /// Turned off: everything passes untouched, except the drag and release of a press held
    /// back or replaced before, which are dropped (without its action), so no app gets a release
    /// without its press. Left click's always reached the app, so its release does too.
    fn off(&mut self, input: Input) -> Output {
        match input {
            Input::Up(button) => {
                let pending = self.held.contains(&button) || self.swallowed.contains(&button);
                self.released(button);
                if pending && button != LEFT {
                    Output::Suppress
                } else {
                    Output::Pass
                }
            }
            Input::Dragged(button)
                if button != LEFT
                    && (self.held.contains(&button) || self.swallowed.contains(&button)) =>
            {
                Output::Suppress
            }
            _ => Output::Pass,
        }
    }

    fn scroll(
        &mut self,
        continuous: bool,
        vertical: i64,
        horizontal: i64,
        modifiers: Modifiers,
        settings: &MouseSettings,
    ) -> Output {
        if continuous {
            return Output::Pass;
        }
        // Turning the wheel while holding buttons means they weren't meant as clicks.
        if !self.held.is_empty() {
            self.used = true;
        }
        let combo = if self.held.is_empty() && !modifiers.any() {
            None
        } else {
            settings.wheel_combo(&self.held, modifiers)
        };
        if let Some(action) = combo {
            let (lines, up) = if vertical == 0 {
                (horizontal, horizontal > 0)
            } else {
                (vertical, vertical > 0)
            };
            if lines == 0 {
                return Output::Suppress;
            }
            if action == ScrollAction::SwitchApps && !self.switching {
                // ⌘ stays down until the buttons are let go, so the switcher stays open.
                self.switching = true;
                return Output::Act(vec![Effect::Command(true), combo_effect(action, up, lines)]);
            }
            return Output::Act(vec![combo_effect(action, up, lines)]);
        }
        let vertical_sign = if settings.reverse_vertical { -1.0 } else { 1.0 };
        let horizontal_sign = if settings.reverse_horizontal {
            -1.0
        } else {
            1.0
        };
        if settings.smooth {
            let scale = STEP_POINTS * f64::from(settings.speed) / 100.0;
            return Output::Glide {
                vertical: vertical as f64 * scale * vertical_sign,
                horizontal: horizontal as f64 * scale * horizontal_sign,
            };
        }
        if !settings.reverse_vertical && !settings.reverse_horizontal && settings.speed == 100 {
            return Output::Pass;
        }
        Output::Adjust {
            reverse_vertical: settings.reverse_vertical,
            reverse_horizontal: settings.reverse_horizontal,
            percent: settings.speed,
        }
    }

    fn down(&mut self, button: u8, modifiers: Modifiers, settings: &MouseSettings) -> Output {
        if settings.in_combo(button) {
            if let Err(at) = self.held.binary_search(&button) {
                self.held.insert(at, button);
            }
            // Left or right click held counts towards a combination, which one of buttons 3–8
            // then completes. Left click always reaches the app (holding it back would break
            // dragging, selecting and double-clicks); right click waits for its release, so its
            // menu doesn't open while a combination is used.
            if button == LEFT {
                return Output::Pass;
            }
            if button == RIGHT {
                return Output::Suppress;
            }
            // The last button pressed completes a press combination, with the keys held now.
            if let Some(action) = settings.press_combo(&self.held, modifiers) {
                self.used = true;
                return Output::Act(effects(action));
            }
            return Output::Suppress;
        }
        match settings.mapping(button) {
            Some(action) => {
                if !self.swallowed.contains(&button) {
                    self.swallowed.push(button);
                }
                Output::Act(effects(action))
            }
            None => Output::Pass,
        }
    }

    fn up(&mut self, button: u8, settings: &MouseSettings) -> Output {
        if self.switching && self.held.contains(&button) {
            // Letting go of the combination lets go of ⌘: macOS switches to the app chosen.
            self.switching = false;
            self.let_go(button);
            return Output::Act(vec![Effect::Command(false)]);
        }
        if self.held.contains(&button) {
            let used = self.used;
            self.let_go(button);
            // Its press reached the app, so its release must too.
            if button == LEFT {
                return Output::Pass;
            }
            if used {
                return Output::Suppress;
            }
            // Not part of anything: the button does what it otherwise would.
            return Output::Act(match settings.mapping(button) {
                Some(action) => effects(action),
                None => vec![Effect::Click(button)],
            });
        }
        if let Some(index) = self
            .swallowed
            .iter()
            .position(|swallowed| *swallowed == button)
        {
            self.swallowed.remove(index);
            return Output::Suppress;
        }
        Output::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mouse::config::{ButtonMapping, Clicks, Combo, ComboAction, Shortcut};

    fn on() -> MouseSettings {
        MouseSettings {
            enabled: true,
            ..MouseSettings::default()
        }
    }

    fn down(button: u8) -> Input {
        Input::Down {
            button,
            modifiers: Modifiers::default(),
        }
    }

    /// A wheel combination of these buttons and keys.
    fn held(buttons: &[u8], modifiers: Modifiers, wheel: ScrollAction) -> Combo {
        Combo {
            buttons: buttons.to_vec(),
            modifiers,
            does: ComboAction::Wheel { wheel },
        }
    }

    /// A press combination of these buttons and keys.
    fn pressed(buttons: &[u8], modifiers: Modifiers, action: ButtonAction) -> Combo {
        Combo {
            buttons: buttons.to_vec(),
            modifiers,
            does: ComboAction::Press { action },
        }
    }

    const NONE: Modifiers = Modifiers {
        command: false,
        shift: false,
        option: false,
        control: false,
    };

    fn wheel(vertical: i64) -> Input {
        Input::Scroll {
            continuous: false,
            vertical,
            horizontal: 0,
            modifiers: Modifiers::default(),
        }
    }

    const OPTION: Modifiers = Modifiers {
        command: false,
        shift: false,
        option: true,
        control: false,
    };

    #[test]
    fn nothing_changes_while_off() {
        let mut engine = Engine::default();
        let off = MouseSettings::default();
        assert_eq!(engine.handle(wheel(1), &off), Output::Pass);
        assert_eq!(engine.handle(down(3), &off), Output::Pass);
    }

    #[test]
    fn the_mouse_wheel_reverses_and_the_trackpad_doesnt() {
        let mut engine = Engine::default();
        let settings = on();
        assert_eq!(
            engine.handle(wheel(1), &settings),
            Output::Adjust {
                reverse_vertical: true,
                reverse_horizontal: false,
                percent: 100
            }
        );
        let trackpad = Input::Scroll {
            continuous: true,
            vertical: 1,
            horizontal: 0,
            modifiers: Modifiers::default(),
        };
        assert_eq!(engine.handle(trackpad, &settings), Output::Pass);
        // Neither reversed nor faster: left alone.
        let plain = MouseSettings {
            reverse_vertical: false,
            ..on()
        };
        assert_eq!(engine.handle(wheel(1), &plain), Output::Pass);
    }

    #[test]
    fn smooth_scrolling_glides_by_the_speed() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            smooth: true,
            speed: 200,
            ..on()
        };
        assert_eq!(
            engine.handle(wheel(1), &settings),
            Output::Glide {
                vertical: -2.0 * STEP_POINTS,
                horizontal: 0.0
            }
        );
    }

    #[test]
    fn a_mapped_button_acts_on_press_and_its_release_is_dropped() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            buttons: vec![
                ButtonMapping {
                    button: 3,
                    action: ButtonAction::Back,
                },
                ButtonMapping {
                    button: 4,
                    action: ButtonAction::Shortcut {
                        shortcut: Shortcut {
                            key: "t".to_owned(),
                            modifiers: Modifiers {
                                command: true,
                                shift: true,
                                ..Modifiers::default()
                            },
                        },
                    },
                },
            ],
            ..on()
        };
        assert_eq!(
            engine.handle(down(3), &settings),
            Output::Act(vec![keys(LEFT_BRACKET, true, false, false, false)])
        );
        assert_eq!(
            engine.handle(Input::Dragged(3), &settings),
            Output::Suppress
        );
        assert_eq!(engine.handle(Input::Up(3), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(down(4), &settings),
            Output::Act(vec![keys(17, true, true, false, false)])
        );
        // An unmapped button passes, press and release.
        assert_eq!(engine.handle(down(5), &settings), Output::Pass);
        assert_eq!(engine.handle(Input::Up(5), &settings), Output::Pass);
    }

    #[test]
    fn a_dropped_release_ends_a_hold_begun_just_before_testing() {
        // The tap handled the press (held for a combination) just before testing began; the
        // window then dropped the press, and the tap drops its release after testing and calls
        // `released`. The wheel must not stay in the combination.
        let mut engine = Engine::default();
        let settings = MouseSettings {
            combos: vec![held(&[4], NONE, ScrollAction::SwitchSpaces)],
            ..on()
        };
        assert_eq!(engine.handle(down(4), &settings), Output::Suppress);
        engine.released(4);
        assert!(matches!(
            engine.handle(wheel(1), &settings),
            Output::Adjust { .. }
        ));
    }

    #[test]
    fn a_release_during_testing_ends_only_that_buttons_pairing() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            buttons: vec![ButtonMapping {
                button: 3,
                action: ButtonAction::Back,
            }],
            combos: vec![held(&[4], NONE, ScrollAction::Zoom)],
            ..on()
        };
        // Both pressed before testing: 3's press acted, 4 is held for its combination.
        engine.handle(down(3), &settings);
        engine.handle(down(4), &settings);
        // 4 is released during testing (its release went past untouched).
        engine.released(4);
        // The wheel scrolls as usual again: 4 is no longer held.
        assert!(matches!(
            engine.handle(wheel(1), &settings),
            Output::Adjust { .. }
        ));
        // 3, pressed before testing and released after it, is still swallowed: no stray release.
        assert_eq!(engine.handle(Input::Up(3), &settings), Output::Suppress);
        // And a fresh press and release of 4 behaves as usual.
        assert_eq!(engine.handle(down(4), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(Input::Up(4), &settings),
            Output::Act(vec![Effect::Click(4)])
        );
    }

    #[test]
    fn a_held_button_turns_the_wheel_into_its_combination() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            combos: vec![held(&[4], NONE, ScrollAction::SwitchSpaces)],
            ..on()
        };
        assert_eq!(engine.handle(down(4), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(wheel(1), &settings),
            Output::Act(vec![keys(LEFT_ARROW, false, false, false, true)])
        );
        assert_eq!(
            engine.handle(wheel(-1), &settings),
            Output::Act(vec![keys(RIGHT_ARROW, false, false, false, true)])
        );
        // Used with the wheel: releasing it doesn't click.
        assert_eq!(engine.handle(Input::Up(4), &settings), Output::Suppress);
        // Pressed and released alone, it clicks as usual.
        assert_eq!(engine.handle(down(4), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(Input::Up(4), &settings),
            Output::Act(vec![Effect::Click(4)])
        );
    }

    #[test]
    fn the_app_switcher_holds_command_until_the_buttons_are_let_go() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            combos: vec![held(&[4], NONE, ScrollAction::SwitchApps)],
            ..on()
        };
        assert!(settings.valid());
        assert_eq!(engine.handle(down(4), &settings), Output::Suppress);
        // The first step holds ⌘ and moves on; the next only moves.
        assert_eq!(
            engine.handle(wheel(-1), &settings),
            Output::Act(vec![
                Effect::Command(true),
                combo_effect(ScrollAction::SwitchApps, false, -1)
            ])
        );
        assert_eq!(
            engine.handle(wheel(1), &settings),
            Output::Act(vec![combo_effect(ScrollAction::SwitchApps, true, 1)])
        );
        assert!(!engine.idle(), "not idle while ⌘ is held");
        // Letting go switches to the app chosen, and nothing clicks.
        assert_eq!(
            engine.handle(Input::Up(4), &settings),
            Output::Act(vec![Effect::Command(false)])
        );
        assert!(engine.idle());
        // Windows step one at a time with ⌘`.
        let windows = MouseSettings {
            combos: vec![held(&[4], NONE, ScrollAction::SwitchWindows)],
            ..on()
        };
        assert_eq!(engine.handle(down(4), &windows), Output::Suppress);
        assert_eq!(
            engine.handle(wheel(-1), &windows),
            Output::Act(vec![keys(GRAVE, true, false, false, false)])
        );
    }

    #[test]
    fn the_app_switcher_needs_a_button_that_isnt_left_click() {
        let keys_only = MouseSettings {
            combos: vec![held(&[], OPTION, ScrollAction::SwitchApps)],
            ..on()
        };
        assert!(!keys_only.valid());
        let with_left = MouseSettings {
            combos: vec![held(&[LEFT, 4], NONE, ScrollAction::SwitchApps)],
            ..on()
        };
        assert!(!with_left.valid());
        assert_eq!(
            effects(&ButtonAction::LastApp),
            vec![keys(TAB, true, false, false, false)]
        );
        assert_eq!(
            effects(&ButtonAction::NextWindow),
            vec![keys(GRAVE, true, false, false, false)]
        );
    }

    #[test]
    fn a_held_button_with_its_own_action_does_it_when_released_alone() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            buttons: vec![ButtonMapping {
                button: 3,
                action: ButtonAction::MissionControl,
            }],
            combos: vec![held(&[3], NONE, ScrollAction::Zoom)],
            ..on()
        };
        assert_eq!(engine.handle(down(3), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(Input::Up(3), &settings),
            Output::Act(vec![keys(UP_ARROW, false, false, false, true)])
        );
    }

    #[test]
    fn modifier_combinations_need_exactly_those_modifiers() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            combos: vec![held(&[], OPTION, ScrollAction::HorizontalScroll)],
            ..on()
        };
        let with = |modifiers| Input::Scroll {
            continuous: false,
            vertical: -2,
            horizontal: 0,
            modifiers,
        };
        assert_eq!(
            engine.handle(with(OPTION), &settings),
            Output::Act(vec![Effect::Scroll {
                vertical: 0.0,
                horizontal: -2.0 * STEP_POINTS
            }])
        );
        // ⌥⇧ isn't ⌥: the wheel does its usual thing.
        let more = Modifiers {
            shift: true,
            ..OPTION
        };
        assert!(matches!(
            engine.handle(with(more), &settings),
            Output::Adjust { .. }
        ));
        // A trackpad with ⌥ held is still left alone.
        let pad = Input::Scroll {
            continuous: true,
            vertical: -2,
            horizontal: 0,
            modifiers: OPTION,
        };
        assert_eq!(engine.handle(pad, &settings), Output::Pass);
    }

    #[test]
    fn two_buttons_held_make_the_wheel_do_their_combination() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            combos: vec![
                held(&[3], NONE, ScrollAction::SwitchTabs),
                held(&[3, 4], NONE, ScrollAction::Zoom),
            ],
            ..on()
        };
        // Pressed in either order, 4 then 3, both held: the two-button combination.
        assert_eq!(engine.handle(down(4), &settings), Output::Suppress);
        assert_eq!(engine.handle(down(3), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(wheel(1), &settings),
            Output::Act(vec![keys(EQUAL, true, false, false, false)])
        );
        // Let go of 4: only 3 is held, so its own combination.
        assert_eq!(engine.handle(Input::Up(4), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(wheel(-1), &settings),
            Output::Act(vec![keys(TAB, false, false, false, true)])
        );
        assert_eq!(engine.handle(Input::Up(3), &settings), Output::Suppress);
    }

    #[test]
    fn two_buttons_pressed_together_do_their_action_once() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            combos: vec![pressed(&[3, 4], NONE, ButtonAction::PlayPause)],
            ..on()
        };
        assert_eq!(engine.handle(down(3), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(down(4), &settings),
            Output::Act(vec![Effect::Media(Media::PlayPause)])
        );
        // Neither clicks on release.
        assert_eq!(engine.handle(Input::Up(3), &settings), Output::Suppress);
        assert_eq!(engine.handle(Input::Up(4), &settings), Output::Suppress);
        // Pressed alone, each still clicks (on release).
        assert_eq!(engine.handle(down(3), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(Input::Up(3), &settings),
            Output::Act(vec![Effect::Click(3)])
        );
    }

    #[test]
    fn a_button_with_keys_does_its_action_without_the_wheel() {
        let mut engine = Engine::default();
        let command = Modifiers {
            command: true,
            ..NONE
        };
        let settings = MouseSettings {
            combos: vec![pressed(
                &[2],
                command,
                ButtonAction::Shortcut {
                    shortcut: Shortcut {
                        key: "w".to_owned(),
                        modifiers: command,
                    },
                },
            )],
            ..on()
        };
        // ⌘ + middle click.
        let with_command = Input::Down {
            button: 2,
            modifiers: command,
        };
        assert_eq!(
            engine.handle(with_command, &settings),
            Output::Act(vec![keys(13, true, false, false, false)])
        );
        assert_eq!(engine.handle(Input::Up(2), &settings), Output::Suppress);
        // Without ⌘ it is a middle click again (on release).
        assert_eq!(engine.handle(down(2), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(Input::Up(2), &settings),
            Output::Act(vec![Effect::Click(2)])
        );
    }

    #[test]
    fn a_button_and_keys_held_make_the_wheel_do_their_combination() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            combos: vec![held(&[4], OPTION, ScrollAction::SwitchSpaces)],
            ..on()
        };
        assert_eq!(engine.handle(down(4), &settings), Output::Suppress);
        let step = |modifiers| Input::Scroll {
            continuous: false,
            vertical: 1,
            horizontal: 0,
            modifiers,
        };
        assert_eq!(
            engine.handle(step(OPTION), &settings),
            Output::Act(vec![keys(LEFT_ARROW, false, false, false, true)])
        );
        // Button 4 without ⌥: no combination, so the wheel scrolls as usual.
        assert!(matches!(
            engine.handle(step(NONE), &settings),
            Output::Adjust { .. }
        ));
        // The wheel was used while 4 was held, so its release doesn't click.
        assert_eq!(engine.handle(Input::Up(4), &settings), Output::Suppress);
    }

    #[test]
    fn media_actions_send_media_keys() {
        assert_eq!(
            effects(&ButtonAction::VolumeUp),
            vec![Effect::Media(Media::VolumeUp)]
        );
        assert_eq!(
            effects(&ButtonAction::Mute),
            vec![Effect::Media(Media::Mute)]
        );
        assert_eq!(
            effects(&ButtonAction::PreviousTrack),
            vec![Effect::Media(Media::Previous)]
        );
    }

    #[test]
    fn a_glide_eases_out_and_ends_exactly() {
        let mut remaining = -80.0;
        let mut frames = 0;
        let mut total = 0.0;
        while glide_step(remaining) != 0.0 {
            let step = glide_step(remaining);
            assert!(step < 0.0 && step.abs() <= remaining.abs());
            remaining -= step;
            total += step;
            frames += 1;
        }
        assert_eq!(total, -80.0);
        assert!(frames > 4 && frames < 40, "{frames} frames");
        assert_eq!(glide_step(0.4), 0.0);
        assert_eq!(glide_step(2.0), 1.0);
    }

    #[test]
    fn combination_steps_keep_their_direction() {
        assert_eq!(
            combo_effect(ScrollAction::Zoom, true, 1),
            keys(EQUAL, true, false, false, false)
        );
        assert_eq!(
            combo_effect(ScrollAction::Zoom, false, -1),
            keys(MINUS, true, false, false, false)
        );
        assert_eq!(
            combo_effect(ScrollAction::SwitchTabs, true, 1),
            keys(TAB, false, true, false, true)
        );
        assert_eq!(
            combo_effect(ScrollAction::SwitchTabs, false, -1),
            keys(TAB, false, false, false, true)
        );
    }

    #[test]
    fn left_click_reaches_the_app_and_counts_towards_a_combination() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            combos: vec![
                pressed(&[LEFT, 3], NONE, ButtonAction::Back),
                held(&[LEFT, 4], NONE, ScrollAction::Zoom),
            ],
            ..on()
        };
        // Hold left (it goes through), press button 4: the combination, once.
        assert_eq!(engine.handle(down(LEFT), &settings), Output::Pass);
        assert_eq!(engine.handle(Input::Dragged(LEFT), &settings), Output::Pass);
        assert_eq!(
            engine.handle(down(3), &settings),
            Output::Act(effects(&ButtonAction::Back))
        );
        assert_eq!(engine.handle(Input::Up(3), &settings), Output::Suppress);
        // Left's release always goes through, used or not.
        assert_eq!(engine.handle(Input::Up(LEFT), &settings), Output::Pass);
        // Hold left and button 5, turn the wheel: zoom.
        assert_eq!(engine.handle(down(LEFT), &settings), Output::Pass);
        assert_eq!(engine.handle(down(4), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(wheel(1), &settings),
            Output::Act(vec![combo_effect(ScrollAction::Zoom, true, 1)])
        );
        assert_eq!(engine.handle(Input::Up(LEFT), &settings), Output::Pass);
        assert_eq!(engine.handle(Input::Up(4), &settings), Output::Suppress);
        // Left alone is a plain click, and the wheel scrolls as usual while left is held.
        assert_eq!(engine.handle(down(LEFT), &settings), Output::Pass);
        assert_eq!(
            engine.handle(wheel(1), &settings),
            Output::Adjust {
                reverse_vertical: true,
                reverse_horizontal: false,
                percent: 100
            }
        );
        assert_eq!(engine.handle(Input::Up(LEFT), &settings), Output::Pass);
        // Button 4 alone still clicks on release.
        assert_eq!(engine.handle(down(3), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(Input::Up(3), &settings),
            Output::Act(vec![Effect::Click(3)])
        );
    }

    #[test]
    fn right_click_waits_for_its_release_in_a_combination() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            combos: vec![
                pressed(&[RIGHT, 3], NONE, ButtonAction::Mute),
                held(&[RIGHT, 2], NONE, ScrollAction::SwitchTabs),
            ],
            ..on()
        };
        // Hold right (held back), press button 4: the combination; right's release is dropped,
        // so no menu opens.
        assert_eq!(engine.handle(down(RIGHT), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(Input::Dragged(RIGHT), &settings),
            Output::Suppress
        );
        assert_eq!(
            engine.handle(down(3), &settings),
            Output::Act(effects(&ButtonAction::Mute))
        );
        assert_eq!(engine.handle(Input::Up(3), &settings), Output::Suppress);
        assert_eq!(engine.handle(Input::Up(RIGHT), &settings), Output::Suppress);
        // Hold right and the middle button, turn the wheel: switch tabs.
        assert_eq!(engine.handle(down(RIGHT), &settings), Output::Suppress);
        assert_eq!(engine.handle(down(2), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(wheel(-1), &settings),
            Output::Act(vec![combo_effect(ScrollAction::SwitchTabs, false, -1)])
        );
        assert_eq!(engine.handle(Input::Up(2), &settings), Output::Suppress);
        assert_eq!(engine.handle(Input::Up(RIGHT), &settings), Output::Suppress);
        // A plain right click: its click on release, so its menu opens then.
        assert_eq!(engine.handle(down(RIGHT), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(Input::Up(RIGHT), &settings),
            Output::Act(vec![Effect::Click(RIGHT)])
        );
        // A button pressed first, then right: right doesn't complete a press combination.
        assert_eq!(engine.handle(down(3), &settings), Output::Suppress);
        assert_eq!(engine.handle(down(RIGHT), &settings), Output::Suppress);
        assert_eq!(
            engine.handle(Input::Up(RIGHT), &settings),
            Output::Act(vec![Effect::Click(RIGHT)])
        );
    }

    #[test]
    fn turned_off_mid_press_still_pairs_the_release() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            buttons: vec![ButtonMapping {
                button: 4,
                action: ButtonAction::Forward,
            }],
            combos: vec![
                pressed(&[RIGHT, 3], NONE, ButtonAction::Mute),
                pressed(&[LEFT, 2], NONE, ButtonAction::Back),
            ],
            ..on()
        };
        // Button 5 replaced, right held back, left held (it went through).
        engine.handle(down(4), &settings);
        engine.handle(down(RIGHT), &settings);
        assert_eq!(engine.handle(down(LEFT), &settings), Output::Pass);
        let off = MouseSettings {
            enabled: false,
            ..settings
        };
        // A new press goes straight through.
        assert_eq!(engine.handle(down(3), &off), Output::Pass);
        assert_eq!(engine.handle(Input::Up(3), &off), Output::Pass);
        // The held-back ones' drags and releases are dropped, without their action; left's
        // release goes through.
        assert_eq!(engine.handle(Input::Dragged(RIGHT), &off), Output::Suppress);
        assert_eq!(engine.handle(Input::Up(4), &off), Output::Suppress);
        assert_eq!(engine.handle(Input::Up(RIGHT), &off), Output::Suppress);
        assert_eq!(engine.handle(Input::Up(LEFT), &off), Output::Pass);
        assert!(engine.idle());
        // Then nothing is touched.
        assert_eq!(engine.handle(Input::Up(4), &off), Output::Pass);
    }

    #[test]
    fn idle_only_once_every_held_or_swallowed_press_is_released() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            buttons: vec![ButtonMapping {
                button: 4,
                action: ButtonAction::Forward,
            }],
            combos: vec![pressed(&[RIGHT, 3], NONE, ButtonAction::Mute)],
            ..on()
        };
        assert!(engine.idle());
        // Right held back: not idle until its release.
        engine.handle(down(RIGHT), &settings);
        assert!(!engine.idle());
        engine.handle(Input::Up(RIGHT), &settings);
        assert!(engine.idle());
        // A mapped button's press swallowed: not idle until its release.
        engine.handle(down(4), &settings);
        assert!(!engine.idle());
        engine.handle(Input::Up(4), &settings);
        assert!(engine.idle());
        // Left in a combination passes, but is held until released.
        let left = MouseSettings {
            combos: vec![pressed(&[LEFT, 3], NONE, ButtonAction::Back)],
            ..on()
        };
        engine.handle(down(LEFT), &left);
        assert!(!engine.idle());
        engine.handle(Input::Up(LEFT), &left);
        assert!(engine.idle());
    }

    #[test]
    fn left_and_right_click_pass_untouched_unless_a_combination_uses_them() {
        let mut engine = Engine::default();
        let settings = MouseSettings {
            combos: vec![held(&[3], NONE, ScrollAction::Zoom)],
            ..on()
        };
        for button in [LEFT, RIGHT] {
            assert_eq!(engine.handle(down(button), &settings), Output::Pass);
            assert_eq!(
                engine.handle(Input::Dragged(button), &settings),
                Output::Pass
            );
            assert_eq!(engine.handle(Input::Up(button), &settings), Output::Pass);
        }
        assert_eq!(settings.clicks(), Clicks::default());
        // Only the click a combination uses is seen.
        assert_eq!(
            MouseSettings {
                combos: vec![held(&[RIGHT, 3], NONE, ScrollAction::Zoom)],
                ..on()
            }
            .clicks(),
            Clicks {
                left: false,
                right: true
            }
        );
    }
}
