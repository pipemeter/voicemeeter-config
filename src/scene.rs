//! Saved scenes and voice-modeler presets.
//!
//! A scene is a whole settings document with a name and a timestamp wrapped
//! round it, which is why it holds an [`Imported`] rather than repeating its
//! fields. A pitch preset is much smaller: one strip's pitch block.

use crate::blocks::Pitch;
use crate::read::index_of;
use crate::settings::read_settings;
use crate::{Error, Imported};

/// A saved scene: the mixer as it was, under a name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub name: String,
    pub comment: String,
    /// As written by the original: `2026/04/29 - 01h53:28`. Kept as text
    /// rather than parsed, since nothing here needs to do arithmetic on it
    /// and a date format is a poor thing to guess at.
    pub timestamp: String,
    /// One-based slot the preset occupies.
    pub slot: u32,
    pub settings: Imported,
}

impl Scene {
    pub(crate) fn read(root: &roxmltree::Node<'_, '_>) -> Result<Self, Error> {
        let mut scene = Self {
            settings: read_settings(root)?,
            ..Self::default()
        };
        for node in root.descendants() {
            let text = node.text().unwrap_or_default().trim().to_owned();
            match node.tag_name().name() {
                "PresetName" => {
                    scene.name = text;
                    scene.slot = u32::try_from(index_of(&node).unwrap_or(0) + 1).unwrap_or(1);
                }
                "PresetComment" => scene.comment = text,
                "PresetTimeStamp" => scene.timestamp = text,
                _ => {}
            }
        }
        Ok(scene)
    }
}

/// A voice-modeler preset: one pitch block under a name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PitchPreset {
    pub name: String,
    pub timestamp: String,
    pub slot: u32,
    pub pitch: Pitch,
}

impl PitchPreset {
    pub(crate) fn read(root: &roxmltree::Node<'_, '_>) -> Self {
        let mut preset = Self::default();
        for node in root.descendants() {
            let text = node.text().unwrap_or_default().trim().to_owned();
            match node.tag_name().name() {
                "PresetName" => {
                    preset.name = text;
                    preset.slot = u32::try_from(index_of(&node).unwrap_or(0) + 1).unwrap_or(1);
                }
                "PresetTimeStamp" => preset.timestamp = text,
                "StripPitch" => preset.pitch = Pitch::read(&node),
                _ => {}
            }
        }
        preset
    }
}

#[cfg(test)]
mod tests {
    use crate::Document;

    #[test]
    fn a_pitch_preset_keeps_its_name_and_its_block() {
        let xml = r"<?xml version='1.0' encoding='utf-8'?>
<VBAudioVoicemeeterPresetPitch>
    <PresetName index='1' fdevice='0' >P-3</PresetName>
    <PresetTimeStamp>2025/08/24 - 15h35:40</PresetTimeStamp>
    <StripPitch index='0' pitchon='1' drywet='100.00' pitchvalue='-3.00'
        formantlo='1.00' formantmed='2.00' formanthigh='3.00' />
</VBAudioVoicemeeterPresetPitch>";
        let Document::PitchPreset(preset) = Document::parse(xml).expect("parses") else {
            panic!("should have been recognised as a pitch preset");
        };
        assert_eq!(preset.name, "P-3");
        assert_eq!(preset.timestamp, "2025/08/24 - 15h35:40");
        assert!(preset.pitch.on);
        assert!((preset.pitch.value + 3.0).abs() < 0.01);
        assert!((preset.pitch.formant[2] - 3.0).abs() < 0.01);
    }
}
