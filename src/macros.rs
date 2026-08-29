//! `MacroButtons`.
//!
//! A separate program on Windows, with its own window and its own file. The
//! buttons hold scripts — `Strip[0].mute = 1` and so on — that run when the
//! button is pressed, released, or on load.
//!
//! The scripts are kept as text. They are a small language of their own, and
//! parsing them belongs wherever they are executed rather than here: a file
//! has to survive a load and a save whether or not this build understands
//! every statement in it.

use crate::read::{flag, flag_f32, index_of, mode_index};

/// A whole button map.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Buttons {
    /// Where the window sits, and how big it is.
    pub window: Window,
    pub buttons: Vec<Button>,
}

/// The `MacroButtons` window's own geometry, as the file records it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Window {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// One button.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Button {
    /// One-based, as the file writes it.
    pub index: u32,
    /// Push, toggle, or one of the other shapes, as the file numbers them.
    pub kind: u32,
    pub colour: u32,
    pub name: String,
    pub subname: String,
    /// The three scripts: on load, on press, on release.
    pub on_init: String,
    pub on_press: String,
    pub on_release: String,
    pub shortcut: Shortcut,
    pub trigger: Trigger,
    /// The six MIDI bytes that press this button, if any.
    pub midi: [u8; 6],
}

/// A keyboard shortcut, as the file records one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Shortcut {
    pub key: u32,
    /// Ctrl, Shift and Alt.
    pub modifiers: [bool; 3],
    /// Whether the shortcut fires even when another window has focus.
    pub global: bool,
    pub exclusive: bool,
}

/// Level-triggered activation: the button presses itself when audio on a
/// channel crosses a threshold.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Trigger {
    pub enabled: bool,
    pub channel: u32,
    /// Decibels, as tenths of a dB in the file.
    pub level_in: f32,
    pub level_out: f32,
    /// Milliseconds.
    pub hold: u32,
    pub after_mute: bool,
}

impl Buttons {
    pub(crate) fn read(root: &roxmltree::Node<'_, '_>) -> Self {
        let mut map = Self::default();
        for node in root.descendants() {
            match node.tag_name().name() {
                "MacroButtonConfiguration" => {
                    map.window = Window {
                        x: signed(&node, "x0"),
                        y: signed(&node, "y0"),
                        width: mode_index(flag_f32(&node, "dx")),
                        height: mode_index(flag_f32(&node, "dy")),
                    };
                }
                "MacroButton" => map.buttons.push(Button::read(&node)),
                _ => {}
            }
        }
        map
    }
}

impl Button {
    fn read(node: &roxmltree::Node<'_, '_>) -> Self {
        let mut button = Self {
            index: u32::try_from(index_of(node).unwrap_or(0) + 1).unwrap_or(1),
            kind: mode_index(flag_f32(node, "type")),
            colour: mode_index(flag_f32(node, "color")),
            shortcut: Shortcut {
                key: mode_index(flag_f32(node, "key")),
                modifiers: [flag(node, "ctrl"), flag(node, "shift"), flag(node, "alt")],
                global: flag(node, "anyway"),
                exclusive: flag(node, "exclusive"),
            },
            ..Self::default()
        };
        button.trigger.enabled = flag(node, "trigger");

        for child in node.children() {
            let text = child.text().unwrap_or_default().to_owned();
            match child.tag_name().name() {
                "MB_Name" => button.name = text,
                "MB_Subname" => button.subname = text,
                "MB_InitRequest" => button.on_init = text,
                "MB_OnRequest" => button.on_press = text,
                "MB_OffRequest" => button.on_release = text,
                "MB_MIDI" => {
                    for (i, slot) in button.midi.iter_mut().enumerate() {
                        *slot = child
                            .attribute(format!("b{}", i + 1).as_str())
                            .and_then(|t| u8::from_str_radix(t.trim(), 16).ok())
                            .unwrap_or(0);
                    }
                }
                "MB_TRIGGER" => {
                    button.trigger.channel = mode_index(flag_f32(&child, "tchannel"));
                    button.trigger.level_in = flag_f32(&child, "tin");
                    button.trigger.level_out = flag_f32(&child, "tout");
                    button.trigger.hold = mode_index(flag_f32(&child, "tmsHold"));
                    button.trigger.after_mute = flag(&child, "tafterMute");
                }
                _ => {}
            }
        }
        button
    }

    /// Whether the button does anything at all.
    ///
    /// A stock file carries eighty of them and almost all are blank, so this
    /// is what tells the interesting ones from the padding.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.is_empty()
            && self.on_init.is_empty()
            && self.on_press.is_empty()
            && self.on_release.is_empty()
    }
}

fn signed(node: &roxmltree::Node<'_, '_>, attr: &str) -> i32 {
    node.attribute(attr)
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use crate::Document;

    const SAMPLE: &str = r"<?xml version='1.0' encoding='utf-8'?>
<VBAudioVoicemeeterMacroButtonMap>
<MacroButtonConfiguration x0='10' y0='-20' dx='650' dy='180' >
    <MacroButton index='1' type='2' color='3' key='65' ctrl='1' shift='0' alt='0'
        anyway='1' exclusive='0' trigger='1' xinput='0' >
        <MB_MIDI b1='90' b2='2A' b3='00' b4='00' b5='00' b6='00' />
        <MB_TRIGGER tchannel='4' tin='-20.0' tout='-30.0' tmsHold='1000' tafterMute='1' />
        <MB_Name>Push To Talk</MB_Name>
        <MB_OnRequest>Strip[0].mute = 0;</MB_OnRequest>
        <MB_OffRequest>Strip[0].mute = 1;</MB_OffRequest>
    </MacroButton>
    <MacroButton index='2' type='0' color='0' key='0' ctrl='0' shift='0' alt='0' >
        <MB_Name></MB_Name>
    </MacroButton>
</MacroButtonConfiguration>
</VBAudioVoicemeeterMacroButtonMap>";

    #[test]
    fn a_button_map_is_recognised_and_read() {
        let Document::MacroButtons(map) = Document::parse(SAMPLE).expect("parses") else {
            panic!("should have been recognised as a button map");
        };
        assert_eq!(map.window.x, 10);
        assert_eq!(map.window.y, -20);
        assert_eq!(map.buttons.len(), 2);

        let first = &map.buttons[0];
        assert_eq!(first.name, "Push To Talk");
        assert_eq!(first.on_press, "Strip[0].mute = 0;");
        assert_eq!(first.on_release, "Strip[0].mute = 1;");
        assert_eq!(first.midi[0], 0x90);
        assert!(first.shortcut.modifiers[0]);
        assert!(first.shortcut.global);
    }

    #[test]
    fn the_blank_padding_buttons_are_recognisable() {
        let Document::MacroButtons(map) = Document::parse(SAMPLE).expect("parses") else {
            panic!("wrong document kind");
        };
        assert!(!map.buttons[0].is_empty());
        assert!(map.buttons[1].is_empty());
    }

    #[test]
    fn a_trigger_keeps_its_levels() {
        let Document::MacroButtons(map) = Document::parse(SAMPLE).expect("parses") else {
            panic!("wrong document kind");
        };
        let trigger = map.buttons[0].trigger;
        assert!(trigger.enabled);
        assert_eq!(trigger.channel, 4);
        assert!((trigger.level_in + 20.0).abs() < 0.01);
        assert!((trigger.level_out + 30.0).abs() < 0.01);
        assert_eq!(trigger.hold, 1000);
        assert!(trigger.after_mute);
    }
}
