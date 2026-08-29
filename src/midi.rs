//! MIDI mapping.
//!
//! A flat list of assignments, each naming a mixer parameter — `InGainFader1`,
//! `MuteBus3` — and the six bytes of the MIDI message that drives it.
//!
//! The parameter is kept as the string the file uses rather than being
//! parsed into an enum. There are hundreds of them, they differ between
//! editions, and an unknown name has to survive a load and a save regardless:
//! turning `InGainFader1` into a variant would mean a file from a newer
//! version silently losing every mapping this build has not heard of.

use crate::read::{flag, flag_f32, mode_index};

/// Indices into [`Item::flags`].
pub const ENCODER: usize = 0;
pub const PUSH_TO_TALK: usize = 1;
pub const OMNI: usize = 2;
pub const DISABLED: usize = 3;

/// A whole MIDI map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Map {
    /// What the map is called in the original's title bar.
    pub name: String,
    pub input_device: String,
    pub output_device: String,
    pub extra_input_device: String,
    pub items: Vec<Item>,
}

/// One assignment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    /// The mixer parameter, as the file names it.
    pub id: String,
    /// The six message bytes, as written — hex text in the file.
    pub bytes: [u8; 6],
    /// Encoder, push-to-talk, omni and disabled, in that order. An array
    /// rather than four named flags, the same shape the mixer's own toggles
    /// use, and it keeps the struct under clippy's bool limit.
    pub flags: [bool; 4],
    /// Feedback sent back to the controller for the lit and unlit states.
    pub feedback: (u8, u8),
    pub feedback_enabled: [bool; 2],
}

impl Map {
    pub(crate) fn read(root: &roxmltree::Node<'_, '_>) -> Self {
        let mut map = Self::default();
        for node in root.descendants() {
            match node.tag_name().name() {
                "MidiMapName" => node.text().unwrap_or_default().clone_into(&mut map.name),
                "MIDIDevIn" => map.input_device = text_or_attr(&node),
                "MIDIDevOut" => map.output_device = text_or_attr(&node),
                "MIDIDevInExtra" => map.extra_input_device = text_or_attr(&node),
                "MidiMapItem" => map.items.push(Item::read(&node)),
                _ => {}
            }
        }
        map
    }

    /// The assignment for a parameter, if it has one.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }
}

impl Item {
    fn read(node: &roxmltree::Node<'_, '_>) -> Self {
        let mut bytes = [0u8; 6];
        for (i, slot) in bytes.iter_mut().enumerate() {
            *slot = node
                .attribute(format!("b{}", i + 1).as_str())
                .and_then(|text| u8::from_str_radix(text.trim(), 16).ok())
                .unwrap_or(0);
        }
        Self {
            id: node.attribute("id").unwrap_or_default().to_owned(),
            bytes,
            flags: [
                flag(node, "coder"),
                flag(node, "ptt"),
                flag(node, "omni"),
                flag(node, "disabled"),
            ],
            feedback: (byte(node, "feedOn"), byte(node, "feedOff")),
            feedback_enabled: [flag(node, "feed1"), flag(node, "feed2")],
        }
    }
}

fn byte(node: &roxmltree::Node<'_, '_>, attr: &str) -> u8 {
    u8::try_from(mode_index(flag_f32(node, attr))).unwrap_or(0)
}

/// Device elements carry their name as text in some versions and as an
/// attribute in others.
fn text_or_attr(node: &roxmltree::Node<'_, '_>) -> String {
    node.text()
        .or_else(|| node.attribute("name"))
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::Document;

    const SAMPLE: &str = r"<?xml version='1.0' encoding='utf-8'?>
<VBAudioVoicemeeterMIDIMapping>
<VoiceMeeterMidiMap>
    <MidiMapName>my map</MidiMapName>
    <MidiMapItem id='InGainFader1' b1='B0' b2='07' b3='00' b4='00' b5='00' b6='00'
        coder='1' ptt='0' feed1='1' feed2='0' omni='0' disabled='0' feedOn='127' feedOff='0'/>
    <MidiMapItem id='MuteBus3' b1='90' b2='2A' b3='7F' b4='00' b5='00' b6='00'
        coder='0' ptt='1' feed1='0' feed2='0' omni='1' disabled='1' feedOn='64' feedOff='1'/>
</VoiceMeeterMidiMap>
</VBAudioVoicemeeterMIDIMapping>";

    #[test]
    fn the_message_bytes_are_read_as_hex() {
        let Document::Midi(map) = Document::parse(SAMPLE).expect("parses") else {
            panic!("should have been recognised as a MIDI map");
        };
        assert_eq!(map.name, "my map");
        assert_eq!(map.items.len(), 2);

        let fader = map.get("InGainFader1").expect("the fader is mapped");
        assert_eq!(fader.bytes[0], 0xB0);
        assert_eq!(fader.bytes[1], 0x07);
        assert!(fader.flags[super::ENCODER]);
        assert_eq!(fader.feedback, (127, 0));
    }

    #[test]
    fn the_flags_belong_to_the_item_they_are_on() {
        let Document::Midi(map) = Document::parse(SAMPLE).expect("parses") else {
            panic!("wrong document kind");
        };
        let mute = map.get("MuteBus3").expect("the mute is mapped");
        assert!(mute.flags[super::PUSH_TO_TALK]);
        assert!(mute.flags[super::OMNI]);
        assert!(mute.flags[super::DISABLED]);
        assert!(!mute.flags[super::ENCODER]);
        assert!(!map.get("InGainFader1").unwrap().flags[super::DISABLED]);
    }

    #[test]
    fn an_unmapped_parameter_is_absent_rather_than_defaulted() {
        let Document::Midi(map) = Document::parse(SAMPLE).expect("parses") else {
            panic!("wrong document kind");
        };
        assert!(map.get("InGainFader8").is_none());
    }
}
