//! The Mouse settings, kept in the history database (`setting` table) like the preferences:
//! one validated record. A stored value the Mouse page doesn't offer replaces the whole record
//! with the defaults, never partly used. An older record (`mouse.v1`, whose combinations were
//! one button or one set of keys, with the wheel) is read and carried over.

use serde::{Deserialize, Serialize};

use crate::persistence::{Database, Unavailable};

const KEY: &str = "mouse.v2";
const KEY_V1: &str = "mouse.v1";
/// At most this many buttons in one combination.
pub const MAX_COMBO_BUTTONS: usize = 2;
/// Wheel speeds offered, in percent of macOS's own.
pub const SPEEDS: [u16; 6] = [50, 75, 100, 150, 200, 300];
/// The buttons that can be changed: macOS numbers them from 0 (left); 2 is the middle button
/// (the third), 3 and 4 the usual side buttons. Left and right click are never changed.
pub const BUTTONS: std::ops::RangeInclusive<u8> = 2..=7;
/// Left and right click, as macOS numbers them. Either can be part of a combination, with one
/// of `BUTTONS`; never on its own and never both.
pub const LEFT: u8 = 0;
pub const RIGHT: u8 = 1;
const MAX_COMBOS: usize = 8;

/// The keys a shortcut can use, with their macOS virtual key codes (the key's position on an
/// ANSI keyboard, so on another layout a letter is the key in that position).
pub const KEYS: [(&str, u16); 61] = [
    ("a", 0),
    ("s", 1),
    ("d", 2),
    ("f", 3),
    ("h", 4),
    ("g", 5),
    ("z", 6),
    ("x", 7),
    ("c", 8),
    ("v", 9),
    ("b", 11),
    ("q", 12),
    ("w", 13),
    ("e", 14),
    ("r", 15),
    ("y", 16),
    ("t", 17),
    ("1", 18),
    ("2", 19),
    ("3", 20),
    ("4", 21),
    ("6", 22),
    ("5", 23),
    ("=", 24),
    ("9", 25),
    ("7", 26),
    ("-", 27),
    ("8", 28),
    ("0", 29),
    ("]", 30),
    ("o", 31),
    ("u", 32),
    ("[", 33),
    ("i", 34),
    ("p", 35),
    ("return", 36),
    ("l", 37),
    ("j", 38),
    ("k", 40),
    ("n", 45),
    ("m", 46),
    ("tab", 48),
    ("space", 49),
    ("delete", 51),
    ("escape", 53),
    ("f1", 122),
    ("f2", 120),
    ("f3", 99),
    ("f4", 118),
    ("f5", 96),
    ("f6", 97),
    ("f7", 98),
    ("f8", 100),
    ("f9", 101),
    ("f10", 109),
    ("f11", 103),
    ("f12", 111),
    ("left", 123),
    ("right", 124),
    ("down", 125),
    ("up", 126),
];

/// The macOS key code of a key name `KEYS` offers.
pub fn key_code(key: &str) -> Option<u16> {
    KEYS.iter()
        .find(|(name, _)| *name == key)
        .map(|(_, code)| *code)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Modifiers {
    pub command: bool,
    pub shift: bool,
    pub option: bool,
    pub control: bool,
}

impl Modifiers {
    pub const fn any(self) -> bool {
        self.command || self.shift || self.option || self.control
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Shortcut {
    /// A name from `KEYS`.
    pub key: String,
    pub modifiers: Modifiers,
}

/// What a button does instead of its own click.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ButtonAction {
    /// ⌘[ (Back in browsers and Finder).
    Back,
    /// ⌘].
    Forward,
    /// ⌃↑, macOS's Mission Control shortcut.
    MissionControl,
    /// ⌃↓, macOS's App Exposé shortcut.
    AppWindows,
    /// ⌃←, the previous Space.
    PreviousSpace,
    /// ⌃→, the next Space.
    NextSpace,
    MiddleClick,
    /// The button does nothing.
    Disabled,
    Shortcut {
        shortcut: Shortcut,
    },
    /// The media keys on a Mac keyboard.
    PlayPause,
    NextTrack,
    PreviousTrack,
    VolumeUp,
    VolumeDown,
    Mute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ButtonMapping {
    pub button: u8,
    pub action: ButtonAction,
}

/// What the wheel does while a combination is held. Up is the first named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScrollAction {
    /// Previous / next Space (⌃← / ⌃→).
    SwitchSpaces,
    /// Zoom in / out (⌘= / ⌘-).
    Zoom,
    /// Previous / next tab (⌃⇧⇥ / ⌃⇥).
    SwitchTabs,
    /// The wheel scrolls sideways.
    HorizontalScroll,
}

/// What a combination does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ComboAction {
    /// While the buttons and keys are held, each wheel step does this.
    Wheel { wheel: ScrollAction },
    /// Pressed together (the last button pressed completes it), this happens once.
    Press { action: ButtonAction },
}

/// A combination: up to two of buttons 3–8 (as macOS numbers them, 2–7), or left or right click
/// with one of them, in increasing order, and any modifier keys, held together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Combo {
    pub buttons: Vec<u8>,
    pub modifiers: Modifiers,
    pub does: ComboAction,
}

impl Combo {
    /// Valid buttons, in increasing order, none repeated. Left or right click only with one of
    /// buttons 3–8 (so it comes first), never alone and never both.
    fn buttons_valid(&self) -> bool {
        let click = |button: &u8| *button == LEFT || *button == RIGHT;
        let clicks = self.buttons.iter().filter(|button| click(button)).count();
        self.buttons.len() <= MAX_COMBO_BUTTONS
            && self
                .buttons
                .iter()
                .all(|button| click(button) || BUTTONS.contains(button))
            && self.buttons.windows(2).all(|pair| pair[0] < pair[1])
            && (clicks == 0 || (clicks == 1 && self.buttons.len() == MAX_COMBO_BUTTONS))
    }

    /// A wheel combination needs a button or a key held; a press needs two buttons, or one
    /// with keys (one button alone is a button setting, not a combination).
    fn valid(&self) -> bool {
        self.buttons_valid()
            && match &self.does {
                ComboAction::Wheel { .. } => !self.buttons.is_empty() || self.modifiers.any(),
                ComboAction::Press { action } => {
                    (self.buttons.len() == MAX_COMBO_BUTTONS
                        || (self.buttons.len() == 1 && self.modifiers.any()))
                        && action_valid(action)
                }
            }
    }

    /// What makes two combinations the same: the buttons, the keys and wheel or press.
    fn key(&self) -> (&[u8], Modifiers, bool) {
        (
            &self.buttons,
            self.modifiers,
            matches!(self.does, ComboAction::Wheel { .. }),
        )
    }
}

fn action_valid(action: &ButtonAction) -> bool {
    match action {
        ButtonAction::Shortcut { shortcut } => key_code(&shortcut.key).is_some(),
        _ => true,
    }
}

/// Which clicks the event tap must see (`MouseSettings::clicks`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Clicks {
    pub left: bool,
    pub right: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MouseSettings {
    /// Everything below applies only while this is on (and macOS allows it).
    pub enabled: bool,
    /// The mouse wheel scrolls the other way from the trackpad's (natural) direction.
    pub reverse_vertical: bool,
    pub reverse_horizontal: bool,
    /// Percent of macOS's wheel speed; one of `SPEEDS`.
    pub speed: u16,
    /// Each wheel step glides instead of jumping.
    pub smooth: bool,
    pub buttons: Vec<ButtonMapping>,
    pub combos: Vec<Combo>,
}

impl Default for MouseSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            reverse_vertical: true,
            reverse_horizontal: false,
            speed: 100,
            smooth: false,
            buttons: Vec::new(),
            combos: Vec::new(),
        }
    }
}

impl MouseSettings {
    /// Only what the Mouse page offers: known speeds, keys and buttons, each button mapped at
    /// most once, and each combination valid and used once.
    pub fn valid(&self) -> bool {
        let mut seen_buttons = Vec::new();
        let buttons = self.buttons.len() <= BUTTONS.count()
            && self.buttons.iter().all(|mapping| {
                let fresh = !seen_buttons.contains(&mapping.button);
                seen_buttons.push(mapping.button);
                fresh && BUTTONS.contains(&mapping.button) && action_valid(&mapping.action)
            });
        let combos = self.combos.len() <= MAX_COMBOS
            && self.combos.iter().enumerate().all(|(index, combo)| {
                combo.valid()
                    && self.combos[..index]
                        .iter()
                        .all(|earlier| earlier.key() != combo.key())
            });
        SPEEDS.contains(&self.speed) && buttons && combos
    }

    // These serve the engine, which only the macOS event tap drives.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn mapping(&self, button: u8) -> Option<&ButtonAction> {
        self.buttons
            .iter()
            .find(|mapping| mapping.button == button)
            .map(|mapping| &mapping.action)
    }

    /// Which of left and right click a combination uses, so the event tap must see them; each
    /// is seen only while needed.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn clicks(&self) -> Clicks {
        Clicks {
            left: self.in_combo(LEFT),
            right: self.in_combo(RIGHT),
        }
    }

    /// Whether `button` is part of any combination (its own click then waits for release).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn in_combo(&self, button: u8) -> bool {
        self.combos
            .iter()
            .any(|combo| combo.buttons.contains(&button))
    }

    /// The wheel combination for exactly these buttons (increasing order) and keys.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn wheel_combo(&self, buttons: &[u8], modifiers: Modifiers) -> Option<ScrollAction> {
        self.combos.iter().find_map(|combo| match combo.does {
            ComboAction::Wheel { wheel }
                if combo.buttons == buttons && combo.modifiers == modifiers =>
            {
                Some(wheel)
            }
            _ => None,
        })
    }

    /// The press combination for exactly these buttons (increasing order) and keys.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn press_combo(&self, buttons: &[u8], modifiers: Modifiers) -> Option<&ButtonAction> {
        self.combos.iter().find_map(|combo| match &combo.does {
            ComboAction::Press { action }
                if combo.buttons == buttons && combo.modifiers == modifiers =>
            {
                Some(action)
            }
            _ => None,
        })
    }
}

/// The older shape (`mouse.v1`), read once and carried over.
mod v1 {
    use serde::Deserialize;

    use super::{ButtonMapping, Combo, ComboAction, Modifiers, MouseSettings, ScrollAction};

    #[derive(Deserialize)]
    #[serde(
        tag = "kind",
        rename_all = "camelCase",
        rename_all_fields = "camelCase"
    )]
    enum Trigger {
        Button { button: u8 },
        Modifiers { modifiers: Modifiers },
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct ScrollCombo {
        trigger: Trigger,
        action: ScrollAction,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    pub struct Settings {
        enabled: bool,
        reverse_vertical: bool,
        reverse_horizontal: bool,
        speed: u16,
        smooth: bool,
        buttons: Vec<ButtonMapping>,
        combos: Vec<ScrollCombo>,
    }

    impl From<Settings> for MouseSettings {
        fn from(old: Settings) -> Self {
            Self {
                enabled: old.enabled,
                reverse_vertical: old.reverse_vertical,
                reverse_horizontal: old.reverse_horizontal,
                speed: old.speed,
                smooth: old.smooth,
                buttons: old.buttons,
                combos: old
                    .combos
                    .into_iter()
                    .map(|combo| {
                        let (buttons, modifiers) = match combo.trigger {
                            Trigger::Button { button } => (vec![button], Modifiers::default()),
                            Trigger::Modifiers { modifiers } => (Vec::new(), modifiers),
                        };
                        Combo {
                            buttons,
                            modifiers,
                            does: ComboAction::Wheel {
                                wheel: combo.action,
                            },
                        }
                    })
                    .collect(),
            }
        }
    }
}

/// What was read: the settings in effect, and whether the stored ones had to be replaced
/// (`saved`, `default`, `invalid` or `unavailable`), as for the preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Loaded {
    pub settings: MouseSettings,
    pub source: &'static str,
}

pub fn load(db: &Database) -> Loaded {
    let rows = match db.settings() {
        Ok(rows) => rows,
        Err(_) => {
            return Loaded {
                settings: MouseSettings::default(),
                source: "unavailable",
            }
        }
    };
    let stored = |wanted: &str| {
        rows.iter()
            .find(|(key, _)| key == wanted)
            .map(|(_, value)| value.as_str())
    };
    // Settings from before combinations could use two buttons and keys: carried over (and
    // saved in the new shape at the next change).
    let parsed = match (stored(KEY), stored(KEY_V1)) {
        (Some(text), _) => serde_json::from_str::<MouseSettings>(text).ok(),
        (None, Some(text)) => serde_json::from_str::<v1::Settings>(text)
            .ok()
            .map(MouseSettings::from),
        (None, None) => {
            return Loaded {
                settings: MouseSettings::default(),
                source: "default",
            }
        }
    };
    match parsed {
        Some(settings) if settings.valid() => Loaded {
            settings,
            source: "saved",
        },
        _ => Loaded {
            settings: MouseSettings::default(),
            source: "invalid",
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveError {
    Invalid,
    Unavailable(Unavailable),
}

pub fn save(db: &Database, settings: &MouseSettings) -> Result<(), SaveError> {
    if !settings.valid() {
        return Err(SaveError::Invalid);
    }
    let text = serde_json::to_string(settings).map_err(|_| SaveError::Invalid)?;
    db.set_setting(KEY, &text).map_err(SaveError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut(key: &str) -> ButtonAction {
        ButtonAction::Shortcut {
            shortcut: Shortcut {
                key: key.to_owned(),
                modifiers: Modifiers {
                    command: true,
                    ..Modifiers::default()
                },
            },
        }
    }

    fn with(change: impl FnOnce(&mut MouseSettings)) -> MouseSettings {
        let mut settings = MouseSettings::default();
        change(&mut settings);
        settings
    }

    #[test]
    fn only_what_the_page_offers_is_valid() {
        assert!(MouseSettings::default().valid());
        assert!(with(|s| s.buttons.push(ButtonMapping {
            button: 3,
            action: shortcut("t")
        }))
        .valid());
        assert!(!with(|s| s.speed = 120).valid());
        // Left and right click are never changed.
        assert!(!with(|s| s.buttons.push(ButtonMapping {
            button: 0,
            action: ButtonAction::Back
        }))
        .valid());
        assert!(!with(|s| s.buttons.push(ButtonMapping {
            button: 1,
            action: ButtonAction::Back
        }))
        .valid());
        assert!(!with(|s| s.buttons.push(ButtonMapping {
            button: 3,
            action: shortcut("ä")
        }))
        .valid());
        assert!(!with(|s| {
            s.buttons.push(ButtonMapping {
                button: 3,
                action: ButtonAction::Back,
            });
            s.buttons.push(ButtonMapping {
                button: 3,
                action: ButtonAction::Forward,
            });
        })
        .valid());
    }

    const CMD: Modifiers = Modifiers {
        command: true,
        shift: false,
        option: false,
        control: false,
    };

    fn wheel(buttons: &[u8], modifiers: Modifiers, wheel: ScrollAction) -> Combo {
        Combo {
            buttons: buttons.to_vec(),
            modifiers,
            does: ComboAction::Wheel { wheel },
        }
    }

    fn press(buttons: &[u8], modifiers: Modifiers, action: ButtonAction) -> Combo {
        Combo {
            buttons: buttons.to_vec(),
            modifiers,
            does: ComboAction::Press { action },
        }
    }

    #[test]
    fn combinations_take_up_to_two_buttons_and_keys() {
        let ok = |combo: Combo| with(|s| s.combos.push(combo)).valid();
        let none = Modifiers::default();
        // Wheel: one or two buttons, keys, or both.
        assert!(ok(wheel(&[3], none, ScrollAction::Zoom)));
        assert!(ok(wheel(&[3, 4], none, ScrollAction::Zoom)));
        assert!(ok(wheel(&[], CMD, ScrollAction::Zoom)));
        assert!(ok(wheel(&[2], CMD, ScrollAction::Zoom)));
        assert!(!ok(wheel(&[], none, ScrollAction::Zoom)));
        // Press: two buttons, or one with keys; a button alone is a button setting.
        assert!(ok(press(&[3, 4], none, ButtonAction::MissionControl)));
        assert!(ok(press(&[2], CMD, ButtonAction::PlayPause)));
        assert!(!ok(press(&[3], none, ButtonAction::Back)));
        assert!(!ok(press(&[], CMD, ButtonAction::Back)));
        // Left or right click with one of buttons 3–8, for the wheel or a press, with or
        // without keys; never alone, never both, never with keys only.
        assert!(ok(wheel(&[RIGHT, 3], none, ScrollAction::Zoom)));
        assert!(ok(press(&[LEFT, 4], none, ButtonAction::Back)));
        assert!(ok(press(&[RIGHT, 2], CMD, ButtonAction::Mute)));
        assert!(!ok(wheel(&[LEFT], none, ScrollAction::Zoom)));
        assert!(!ok(wheel(&[RIGHT], CMD, ScrollAction::Zoom)));
        assert!(!ok(press(&[RIGHT], CMD, ButtonAction::Back)));
        assert!(!ok(wheel(&[LEFT, RIGHT], none, ScrollAction::Zoom)));
        assert!(!ok(wheel(&[3, RIGHT], none, ScrollAction::Zoom)));
        assert!(!ok(wheel(&[8, 3], none, ScrollAction::Zoom)));
        // Left and right click still can't be set up as buttons of their own.
        assert!(!with(|s| s.buttons.push(ButtonMapping {
            button: RIGHT,
            action: ButtonAction::Back
        }))
        .valid());
        // Never three buttons, in increasing order, no repeats.
        assert!(!ok(wheel(&[2, 3, 4], none, ScrollAction::Zoom)));
        assert!(!ok(wheel(&[4, 3], none, ScrollAction::Zoom)));
        assert!(!ok(wheel(&[3, 3], none, ScrollAction::Zoom)));
        assert!(!ok(press(&[3, 4], none, shortcut("ä"))));
        // The same buttons and keys once for the wheel and once for a press: both allowed.
        assert!(with(|s| {
            s.combos.push(wheel(&[3, 4], none, ScrollAction::Zoom));
            s.combos.push(press(&[3, 4], none, ButtonAction::Mute));
        })
        .valid());
        // But not twice for the same one.
        assert!(!with(|s| {
            s.combos.push(wheel(&[3], none, ScrollAction::Zoom));
            s.combos.push(wheel(&[3], none, ScrollAction::SwitchTabs));
        })
        .valid());
    }

    #[test]
    fn settings_from_before_two_button_combinations_carry_over() {
        let db = Database::in_memory();
        // In the older `mouse.v1` shape.
        db.set_setting(
            KEY_V1,
            r#"{"enabled":true,"reverseVertical":true,"reverseHorizontal":true,"speed":100,"smooth":true,"buttons":[],"combos":[{"trigger":{"kind":"button","button":3},"action":"switchTabs"},{"trigger":{"kind":"modifiers","modifiers":{"command":false,"shift":false,"option":true,"control":false}},"action":"horizontalScroll"}]}"#,
        )
        .expect("stored");
        let loaded = load(&db);
        assert_eq!(loaded.source, "saved");
        assert!(loaded.settings.enabled && loaded.settings.smooth);
        let option = Modifiers {
            option: true,
            ..Modifiers::default()
        };
        assert_eq!(
            loaded.settings.combos,
            vec![
                wheel(&[3], Modifiers::default(), ScrollAction::SwitchTabs),
                wheel(&[], option, ScrollAction::HorizontalScroll),
            ]
        );
        // Once saved in the new shape, that is what is read.
        let changed = MouseSettings {
            smooth: false,
            ..loaded.settings
        };
        save(&db, &changed).expect("saved");
        assert_eq!(load(&db).settings, changed);
    }

    #[test]
    fn every_offered_key_has_a_distinct_code() {
        let mut codes: Vec<u16> = KEYS.iter().map(|(_, code)| *code).collect();
        let count = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), count);
        assert_eq!(key_code("up"), Some(126));
        assert_eq!(key_code("["), Some(33));
        assert_eq!(key_code("nope"), None);
    }

    #[test]
    fn saved_settings_round_trip_and_unusable_ones_fall_back_to_the_defaults() {
        let db = Database::in_memory();
        assert_eq!(load(&db).source, "default");
        let chosen = with(|s| {
            s.enabled = true;
            s.smooth = true;
            s.buttons.push(ButtonMapping {
                button: 3,
                action: ButtonAction::Back,
            });
            s.combos.push(wheel(
                &[4],
                Modifiers::default(),
                ScrollAction::SwitchSpaces,
            ));
            s.combos.push(press(&[3, 4], CMD, ButtonAction::VolumeUp));
        });
        save(&db, &chosen).expect("saved");
        assert_eq!(
            load(&db),
            Loaded {
                settings: chosen,
                source: "saved"
            }
        );
        assert_eq!(save(&db, &with(|s| s.speed = 7)), Err(SaveError::Invalid));
        db.set_setting(KEY, "{\"enabled\":true}").expect("edited");
        assert_eq!(load(&db).source, "invalid");
        assert_eq!(load(&db).settings, MouseSettings::default());
    }

    #[test]
    fn the_stored_shape_is_tagged_and_camel_case() {
        let text = serde_json::to_string(&ButtonMapping {
            button: 3,
            action: shortcut("t"),
        })
        .unwrap_or_default();
        assert_eq!(
            text,
            r#"{"button":3,"action":{"kind":"shortcut","shortcut":{"key":"t","modifiers":{"command":true,"shift":false,"option":false,"control":false}}}}"#
        );
        let held = serde_json::to_string(&wheel(&[3, 4], CMD, ScrollAction::HorizontalScroll))
            .unwrap_or_default();
        assert_eq!(
            held,
            r#"{"buttons":[3,4],"modifiers":{"command":true,"shift":false,"option":false,"control":false},"does":{"kind":"wheel","wheel":"horizontalScroll"}}"#
        );
        let pressed =
            serde_json::to_string(&press(&[2], CMD, ButtonAction::PlayPause)).unwrap_or_default();
        assert_eq!(
            pressed,
            r#"{"buttons":[2],"modifiers":{"command":true,"shift":false,"option":false,"control":false},"does":{"kind":"press","action":{"kind":"playPause"}}}"#
        );
    }
}
