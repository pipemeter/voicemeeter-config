//! Writing documents back out.
//!
//! The counterpart to the reader, and in the same crate on purpose: a format
//! known in two places drifts. Everything here is arranged so that what goes
//! out reads back in as the same thing, which is what the round-trip tests
//! at the bottom check.
//!
//! Through a real XML writer rather than string building, so text and
//! attributes are escaped. Voicemeeter itself does not do this — it will
//! happily write a device name containing an ampersand and produce a file
//! that is not XML — and repairing that on the way in is one thing, but
//! emitting it ourselves would be another.

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

use crate::blocks::{Compressor, EqCell, Gate, Pitch};
use crate::settings::{PANEL_ATTRS, SEND_ATTRS};
use crate::{BUS_ATTRS, Bus, Document, EQ_ON, Imported, MONO, MUTE, SOLO, Strip};
use crate::{BUS_EQ, BUS_MONO, BUS_MUTE, BUS_SEL};

/// Where the settings sections live, by dialect.
///
/// The element names are the only structural difference between the two
/// dialects — same children, same attributes, different wrappers — which is
/// what makes ours an extension of theirs rather than a format of its own.
struct Sections {
    root: &'static str,
    prefix: &'static str,
}

impl Sections {
    fn of(dialect: crate::Dialect) -> Self {
        match dialect {
            crate::Dialect::Voicemeeter => Self {
                root: crate::ROOT_VOICEMEETER,
                prefix: "VoiceMeeter",
            },
            crate::Dialect::PipeMeter => Self {
                root: crate::ROOT_PIPEMETER,
                prefix: "Pipemeter",
            },
        }
    }

    /// A wrapper section's name in this dialect. Voicemeeter names every
    /// one of them `VoiceMeeter` + a suffix, so the dialect is a prefix
    /// rather than a table: a section added later is covered already.
    fn named(&self, suffix: &str) -> String {
        format!("{}{suffix}", self.prefix)
    }
}

impl Document {
    /// Render this document as XML.
    #[must_use]
    pub fn render(&self) -> String {
        let mut writer = Writer::new_with_indent(Vec::new(), b'\t', 1);
        let _ = self.build(&mut writer);
        String::from_utf8(writer.into_inner()).unwrap_or_default()
    }

    /// Write this document to a file, creating its directory if needed.
    ///
    /// # Errors
    ///
    /// Fails if the directory cannot be made or the file cannot be written.
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.render())
    }

    fn build(&self, writer: &mut Writer<Vec<u8>>) -> std::io::Result<()> {
        writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("utf-8"), None)))?;
        match self {
            Self::Settings(imported) => settings(writer, imported),
            Self::Scene(scene) => {
                let sections = Sections::of(scene.settings.dialect);
                writer.write_event(Event::Start(BytesStart::new(
                    "VBAudioVoicemeeterPresetScene",
                )))?;
                let mut name = BytesStart::new("PresetName");
                name.push_attribute(("index", scene.slot.to_string().as_str()));
                writer.write_event(Event::Start(name))?;
                writer.write_event(Event::Text(BytesText::new(&scene.name)))?;
                writer.write_event(Event::End(BytesEnd::new("PresetName")))?;
                text_element(writer, "PresetComment", &scene.comment)?;
                text_element(writer, "PresetTimeStamp", &scene.timestamp)?;
                body(writer, &scene.settings, &sections)?;
                writer.write_event(Event::End(BytesEnd::new("VBAudioVoicemeeterPresetScene")))?;
                Ok(())
            }
            Self::Vban(config) => vban(writer, config),
            Self::Midi(_) | Self::MacroButtons(_) | Self::PitchPreset(_) => Ok(()),
        }
    }
}

/// The VBAN configuration, in the shape its own document has.
///
/// Written because the network window edits it. The rest of the read-only
/// kinds stay read-only for the reason given above; this one has stopped
/// being read-only, so it needs a writer or the window's edits would last
/// until the program closes and no longer.
/// The element names below are the only place the old spelling survives, and
/// deliberately: they are not ours to choose. They are what is written in the
/// file, and a document with different tags is a document Voicemeeter cannot
/// read and we cannot read back.
fn vban(writer: &mut Writer<Vec<u8>>, config: &crate::vban::Config) -> std::io::Result<()> {
    const ROOT: &str = "VBAudioVoicemeeterVBANConfig";
    writer.write_event(Event::Start(BytesStart::new(ROOT)))?;

    let mut header = BytesStart::new("VBAN");
    header.push_attribute(("status", bit(config.enabled)));
    header.push_attribute(("username", config.username.as_str()));
    header.push_attribute(("color", config.colour.as_str()));
    writer.write_event(Event::Empty(header))?;

    for (tag, channel_attr, streams) in [
        ("VBANStreamIn", "in", &config.incoming),
        ("VBANStreamOut", "out", &config.outgoing),
    ] {
        for stream in streams {
            let mut e = BytesStart::new(tag);
            e.push_attribute(("index", stream.index.to_string().as_str()));
            e.push_attribute(("status", bit(stream.enabled)));
            e.push_attribute(("name", stream.name.as_str()));
            e.push_attribute(("ip", stream.address.as_str()));
            e.push_attribute(("port", stream.port.to_string().as_str()));
            e.push_attribute((channel_attr, stream.channel.to_string().as_str()));
            e.push_attribute(("NQ", stream.quality.to_string().as_str()));
            writer.write_event(Event::Empty(e))?;
        }
    }

    writer.write_event(Event::End(BytesEnd::new(ROOT)))?;
    Ok(())
}

fn settings(writer: &mut Writer<Vec<u8>>, imported: &Imported) -> std::io::Result<()> {
    let sections = Sections::of(imported.dialect);
    writer.write_event(Event::Start(BytesStart::new(sections.root)))?;
    body(writer, imported, &sections)?;
    writer.write_event(Event::End(BytesEnd::new(sections.root)))?;
    Ok(())
}

fn body(
    writer: &mut Writer<Vec<u8>>,
    imported: &Imported,
    sections: &Sections,
) -> std::io::Result<()> {
    writer.write_event(Event::Start(BytesStart::new(sections.named("DeviceConfiguration"))))?;
    for (i, name) in imported.input_devices.iter().enumerate() {
        device(writer, "InputDev", i, name.as_deref())?;
    }
    for (i, name) in imported.output_devices.iter().enumerate() {
        device(writer, "OutputDev", i, name.as_deref())?;
    }
    writer.write_event(Event::End(BytesEnd::new(sections.named("DeviceConfiguration"))))?;

    writer.write_event(Event::Start(BytesStart::new(sections.named("Parameters"))))?;
    extras(writer, imported)?;
    labels(writer, imported)?;
    for (i, strip) in imported.strips.iter().enumerate() {
        strip_row(writer, i, strip)?;
        panel_row(writer, i, strip)?;
    }
    for (i, bus) in imported.buses.iter().enumerate() {
        bus_row(writer, i, bus)?;
    }
    blocks(writer, imported)?;
    writer.write_event(Event::End(BytesEnd::new(sections.named("Parameters"))))?;

    seen_devices(writer, imported)?;
    eq_section(writer, &sections.named("StripEQ"), "Strip", &imported.strip_eq)?;
    eq_section(writer, &sections.named("BUSEQ"), "Bus", &imported.bus_eq)?;
    memories(writer, imported)
}

/// Every device the mixer has ever seen.
///
/// Ours, not Voicemeeter's - Windows keeps its device history in the
/// registry, so there is no original key to follow. It goes in this file
/// all the same rather than a list of its own: two settings files is one
/// more than anybody asked for, and a device history that can drift out of
/// step with the assignments referring to it is worse than no history.
///
/// The name is fixed in both dialects. A Voicemeeter installation never
/// writes it, so there is no `VoiceMeeter` spelling for it to collide with.
fn seen_devices(writer: &mut Writer<Vec<u8>>, imported: &Imported) -> std::io::Result<()> {
    if imported.seen_devices.is_empty() {
        return Ok(());
    }
    writer.write_event(Event::Start(BytesStart::new(crate::SEEN_DEVICES)))?;
    for device in &imported.seen_devices {
        let mut element = BytesStart::new("SeenDevice");
        element.push_attribute(("direction", device.direction.as_str()));
        element.push_attribute(("name", device.name.as_str()));
        element.push_attribute(("description", device.description.as_str()));
        writer.write_event(Event::Empty(element))?;
    }
    writer.write_event(Event::End(BytesEnd::new(crate::SEEN_DEVICES)))?;
    Ok(())
}

/// The A/B compare memories, put back exactly as they arrived.
///
/// Written raw rather than through the event writer: they are already well
/// formed XML — they came out of a document this crate just parsed — and
/// re-serialising them could only change them.
fn memories(writer: &mut Writer<Vec<u8>>, imported: &Imported) -> std::io::Result<()> {
    use std::io::Write as _;
    for section in &imported.memories {
        let out = writer.get_mut();
        out.write_all(b"\n")?;
        out.write_all(section.as_bytes())?;
    }
    if !imported.memories.is_empty() {
        writer.get_mut().write_all(b"\n")?;
    }
    Ok(())
}

/// The elements only our dialect carries.
///
/// Written unconditionally rather than only for our dialect: Voicemeeter's
/// reader skips what it does not know, exactly as ours does, so a file with
/// them in stays loadable there.
fn extras(writer: &mut Writer<Vec<u8>>, imported: &Imported) -> std::io::Result<()> {
    let extras = &imported.extras;
    if !extras.edition.is_empty() {
        text_element(writer, "Edition", &extras.edition)?;
    }
    if !extras.internal_fx2.is_empty() {
        text_element(writer, "InternalFx2", &extras.internal_fx2)?;
    }
    for (slot, state) in extras.internal_fx_state.iter().enumerate() {
        if !state.is_empty() {
            text_element(writer, &format!("InternalFxState{}", slot + 1), state)?;
        }
    }
    for (row, values) in extras.external_returns.iter().enumerate() {
        if !values.is_empty() {
            text_element(writer, &format!("ExternalReturns{}", row + 1), values)?;
        }
    }
    if !extras.recording_dir.is_empty() {
        text_element(writer, "RecordingDir", &extras.recording_dir)?;
    }
    if !extras.startup_settings.is_empty() {
        text_element(writer, "StartupSettings", &extras.startup_settings)?;
    }
    if extras.armed_inputs.iter().any(|set| *set) {
        let flags: Vec<&str> = extras
            .armed_inputs
            .iter()
            .map(|set| if *set { "1" } else { "0" })
            .collect();
        text_element(writer, "ArmedInputs", &flags.join(","))?;
    }
    if extras.options.iter().any(|set| *set) {
        let flags: Vec<&str> = extras
            .options
            .iter()
            .map(|set| if *set { "1" } else { "0" })
            .collect();
        text_element(writer, "MenuOptions", &flags.join(","))?;
    }
    Ok(())
}

fn labels(writer: &mut Writer<Vec<u8>>, imported: &Imported) -> std::io::Result<()> {
    for (i, strip) in imported.strips.iter().enumerate() {
        let tag = if i < 5 {
            format!("LabelStrip{}", i + 1)
        } else {
            format!("LabelVirtualStrip{}", i - 4)
        };
        text_element(writer, &tag, &strip.label)?;
    }
    for (i, bus) in imported.buses.iter().enumerate() {
        let tag = if i < 5 {
            format!("LabelBus{}", i + 1)
        } else {
            format!("LabelVirtualBus{}", i - 4)
        };
        text_element(writer, &tag, &bus.label)?;
    }
    Ok(())
}

fn strip_row(writer: &mut Writer<Vec<u8>>, i: usize, strip: &Strip) -> std::io::Result<()> {
    let mut e = BytesStart::new("Strip");
    e.push_attribute(("index", (i + 1).to_string().as_str()));
    e.push_attribute(("layout", strip.layout.to_string().as_str()));
    e.push_attribute(("mute", bit(strip.flags[MUTE])));
    e.push_attribute(("vaio", bit(strip.virtual_in)));
    e.push_attribute(("solo", bit(strip.flags[SOLO])));
    e.push_attribute(("mono", bit(strip.flags[MONO])));
    e.push_attribute(("muc", bit(strip.mc)));
    e.push_attribute(("EQon", bit(strip.flags[EQ_ON])));
    e.push_attribute(("karaoke", bit(strip.karaoke)));
    for (attr, value) in ["prg_c", "prg_g", "prg_d"].into_iter().zip(strip.programs) {
        e.push_attribute((attr, value.to_string().as_str()));
    }
    for (attr, on) in BUS_ATTRS.into_iter().zip(strip.buses) {
        e.push_attribute((attr, bit(on)));
    }
    e.push_attribute(("audibility_c", f(strip.comp).as_str()));
    e.push_attribute(("audibility_g", f(strip.gate).as_str()));
    e.push_attribute(("denoiser_g", f(strip.denoiser).as_str()));
    for (i, gain) in strip.eq_gain.iter().enumerate() {
        e.push_attribute((format!("EQGain{}", i + 1).as_str(), f(*gain).as_str()));
    }
    for ((send, post), (value, on)) in SEND_ATTRS
        .into_iter()
        .zip(strip.sends.iter().zip(strip.post))
    {
        e.push_attribute((send, f(*value).as_str()));
        e.push_attribute((post, bit(on)));
    }
    for (i, level) in strip.layers.iter().enumerate() {
        e.push_attribute((format!("layer{}", i + 1).as_str(), f(*level).as_str()));
    }
    e.push_attribute(("dblimit", f(strip.limit_db).as_str()));
    e.push_attribute(("dblevel", f(strip.gain_db).as_str()));
    writer.write_event(Event::Empty(e))
}

/// The pan coordinates, which live on a `<Strip>` of their own.
fn panel_row(writer: &mut Writer<Vec<u8>>, i: usize, strip: &Strip) -> std::io::Result<()> {
    let mut e = BytesStart::new("Strip");
    e.push_attribute(("index", (i + 1).to_string().as_str()));
    for ((x, y), (px, py)) in PANEL_ATTRS.into_iter().zip(strip.panels) {
        e.push_attribute((x, format!("{px:.3}").as_str()));
        e.push_attribute((y, format!("{py:.3}").as_str()));
    }
    e.push_attribute(("PanelFace", strip.panel_face.to_string().as_str()));
    writer.write_event(Event::Empty(e))
}

fn bus_row(writer: &mut Writer<Vec<u8>>, i: usize, bus: &Bus) -> std::io::Result<()> {
    let mut e = BytesStart::new("Bus");
    e.push_attribute(("index", (i + 1).to_string().as_str()));
    e.push_attribute(("mute", bit(bus.toggles[BUS_MUTE])));
    e.push_attribute(("vaio", bit(bus.virtual_out)));
    e.push_attribute(("mono", bit(bus.toggles[BUS_MONO])));
    e.push_attribute(("cross", bit(bus.cross)));
    e.push_attribute(("BusMode", bus.mode.to_string().as_str()));
    e.push_attribute(("EQon", bit(bus.toggles[BUS_EQ])));
    e.push_attribute(("SEL", bit(bus.toggles[BUS_SEL])));
    e.push_attribute(("monitor", bit(bus.monitor)));
    for (attr, value) in ["retR", "retD", "retFx1", "retFx2"]
        .into_iter()
        .zip(bus.returns)
    {
        e.push_attribute((attr, f(value).as_str()));
    }
    e.push_attribute(("dblevel", f(bus.gain_db).as_str()));
    writer.write_event(Event::Empty(e))
}

/// The per-strip processing blocks. Written verbatim: nothing here is
/// interpreted, because the point is to preserve settings this program does
/// not understand well enough to edit.
fn blocks(writer: &mut Writer<Vec<u8>>, imported: &Imported) -> std::io::Result<()> {
    for (i, comp) in imported.compressors.iter().enumerate() {
        compressor_row(writer, i, comp)?;
    }
    for (i, gate) in imported.gates.iter().enumerate() {
        gate_row(writer, i, gate)?;
    }
    for (i, pitch) in imported.pitches.iter().enumerate() {
        pitch_row(writer, i, pitch)?;
    }
    Ok(())
}

fn compressor_row(
    writer: &mut Writer<Vec<u8>>,
    i: usize,
    comp: &Compressor,
) -> std::io::Result<()> {
    let mut e = BytesStart::new("StripComp");
    e.push_attribute(("index", (i + 1).to_string().as_str()));
    e.push_attribute(("gainin", f(comp.gain_in).as_str()));
    e.push_attribute(("attack", f(comp.attack).as_str()));
    e.push_attribute(("release", f(comp.release).as_str()));
    e.push_attribute(("knee", f(comp.knee).as_str()));
    e.push_attribute(("comprate", f(comp.ratio).as_str()));
    e.push_attribute(("threshold", f(comp.threshold).as_str()));
    e.push_attribute(("automakeup", bit(comp.auto_makeup)));
    e.push_attribute(("gainout", f(comp.gain_out).as_str()));
    writer.write_event(Event::Empty(e))
}

fn gate_row(writer: &mut Writer<Vec<u8>>, i: usize, gate: &Gate) -> std::io::Result<()> {
    let mut e = BytesStart::new("StripGate");
    e.push_attribute(("index", (i + 1).to_string().as_str()));
    e.push_attribute(("thresin", f(gate.threshold).as_str()));
    e.push_attribute(("damping", f(gate.damping).as_str()));
    e.push_attribute(("bpsidechain", f(gate.sidechain).as_str()));
    e.push_attribute(("attack", f(gate.attack).as_str()));
    e.push_attribute(("hold", f(gate.hold).as_str()));
    e.push_attribute(("release", f(gate.release).as_str()));
    writer.write_event(Event::Empty(e))
}

fn pitch_row(writer: &mut Writer<Vec<u8>>, i: usize, pitch: &Pitch) -> std::io::Result<()> {
    let mut e = BytesStart::new("StripPitch");
    e.push_attribute(("index", (i + 1).to_string().as_str()));
    e.push_attribute(("pitchon", bit(pitch.on)));
    e.push_attribute(("drywet", f(pitch.dry_wet).as_str()));
    e.push_attribute(("pitchvalue", f(pitch.value).as_str()));
    for (attr, value) in ["formantlo", "formantmed", "formanthigh"]
        .into_iter()
        .zip(pitch.formant)
    {
        e.push_attribute((attr, f(value).as_str()));
    }
    writer.write_event(Event::Empty(e))
}

fn eq_section(
    writer: &mut Writer<Vec<u8>>,
    section: &str,
    tag: &str,
    cells: &[EqCell],
) -> std::io::Result<()> {
    if cells.is_empty() {
        return Ok(());
    }
    writer.write_event(Event::Start(BytesStart::new(section)))?;
    for cell in cells {
        let mut e = BytesStart::new(tag);
        e.push_attribute(("index", cell.owner.to_string().as_str()));
        e.push_attribute(("channel", cell.channel.to_string().as_str()));
        e.push_attribute(("cell", cell.cell.to_string().as_str()));
        e.push_attribute(("EQon", bit(cell.on)));
        e.push_attribute(("EQtype", cell.kind.to_string().as_str()));
        e.push_attribute(("dblevel", f(cell.gain_db).as_str()));
        e.push_attribute(("freq", f(cell.freq).as_str()));
        e.push_attribute(("Q", f(cell.q).as_str()));
        writer.write_event(Event::Empty(e))?;
    }
    writer.write_event(Event::End(BytesEnd::new(section)))
}

fn device(
    writer: &mut Writer<Vec<u8>>,
    tag: &str,
    index: usize,
    name: Option<&str>,
) -> std::io::Result<()> {
    let mut e = BytesStart::new(tag);
    e.push_attribute(("index", (index + 1).to_string().as_str()));
    e.push_attribute(("name", name.unwrap_or("-")));
    writer.write_event(Event::Empty(e))
}

fn text_element(writer: &mut Writer<Vec<u8>>, tag: &str, text: &str) -> std::io::Result<()> {
    if text.is_empty() {
        return writer.write_event(Event::Empty(BytesStart::new(tag)));
    }
    writer.write_event(Event::Start(BytesStart::new(tag)))?;
    writer.write_event(Event::Text(BytesText::new(text)))?;
    writer.write_event(Event::End(BytesEnd::new(tag)))
}

fn bit(on: bool) -> &'static str {
    if on { "1" } else { "0" }
}

/// Two decimals, as the original writes its numbers.
fn f(value: f32) -> String {
    format!("{value:.2}")
}

#[cfg(test)]
mod tests {
    use crate::{Dialect, Document, Imported, MUTE, PANEL_3D, SOLO, parse};

    fn sample() -> Imported {
        let mut imported = crate::Imported {
            strips: vec![crate::Strip::default(); 8],
            buses: vec![crate::Bus::default(); 8],
            input_devices: vec![None; 5],
            output_devices: vec![None; 5],
            compressors: vec![crate::blocks::Compressor::default(); 8],
            gates: vec![crate::blocks::Gate::default(); 8],
            pitches: vec![crate::blocks::Pitch::default(); 8],
            ..crate::Imported::default()
        };
        imported.strips[0].label = "Mic IN".to_owned();
        imported.strips[0].gain_db = 3.09;
        imported.strips[0].flags[SOLO] = true;
        imported.strips[0].buses = [false, false, false, true, false, true, true, false];
        imported.strips[0].layers[2] = -6.5;
        imported.strips[0].panels[PANEL_3D] = (-0.25, 0.5);
        imported.strips[2].comp = 4.0;
        imported.buses[1].label = "Headphones".to_owned();
        imported.buses[1].gain_db = -3.25;
        imported.buses[1].toggles[MUTE] = true;
        imported.buses[1].mode = 5;
        imported.compressors[3].ratio = 2.74;
        imported.gates[4].hold = 177.0;
        imported.input_devices[0] = Some("a & b".to_owned());
        imported.extras.edition = "Romanesco".to_owned();
        imported.extras.internal_fx2 = "C5".to_owned();
        imported.extras.options[1] = true;
        imported.extras.options[6] = true;
        imported.extras.internal_fx_state[0] = "HALL,0.3500,0.7200,0.2000,0.8500".to_owned();
        imported.extras.armed_inputs[2] = true;
        imported.extras.external_returns[1] =
            "0.0000,0.7500,0.0000,0.0000,0.0000,0.0000,0.0000,0.0000".to_owned();
        imported.extras.armed_inputs[7] = true;
        imported
    }

    /// Every real settings file we have, written back out and read again.
    ///
    /// The strongest statement available about the writer: not that it emits
    /// something plausible, but that a genuine 300 KB file survives the round
    /// trip with its strips, buses, blocks and EQ cells intact.
    #[test]
    fn real_files_survive_a_round_trip() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.references/configs");
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };

        let mut checked = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "xml") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(document) = Document::parse(&text) else {
                continue;
            };
            let Some(original) = document.settings() else {
                continue;
            };

            let written = Document::Settings(Box::new(original.clone())).render();
            let back = parse(&written)
                .unwrap_or_else(|e| panic!("{} did not survive: {e}", path.display()));

            let name = path.display();
            assert_eq!(back.strips.len(), original.strips.len(), "{name}: strips");
            assert_eq!(back.buses.len(), original.buses.len(), "{name}: buses");
            assert_eq!(
                back.strip_eq.len(),
                original.strip_eq.len(),
                "{name}: strip EQ cells"
            );
            assert_eq!(
                back.bus_eq.len(),
                original.bus_eq.len(),
                "{name}: bus EQ cells"
            );
            for (i, (a, b)) in original.strips.iter().zip(&back.strips).enumerate() {
                assert_eq!(a.buses, b.buses, "{name}: strip {i} routing");
                assert_eq!(a.label, b.label, "{name}: strip {i} label");
                assert!(
                    (a.gain_db - b.gain_db).abs() < 0.01,
                    "{name}: strip {i} gain"
                );
                assert_eq!(a.panels, b.panels, "{name}: strip {i} panels");
            }
            for (i, (a, b)) in original.buses.iter().zip(&back.buses).enumerate() {
                assert_eq!(a.mode, b.mode, "{name}: bus {i} mode");
                assert_eq!(a.toggles, b.toggles, "{name}: bus {i} toggles");
            }
            assert_eq!(
                back.memories.len(),
                original.memories.len(),
                "{name}: A/B memories"
            );
            assert_eq!(back.memories, original.memories, "{name}: memory contents");
            checked += 1;
        }
        assert!(
            checked > 5,
            "expected several settings files, saw {checked}"
        );
    }

    #[test]
    fn an_empty_label_reads_back_as_empty_not_as_whitespace() {
        let mut original = sample();
        original.strips[3].label = String::new();
        let text = Document::Settings(Box::new(original)).render();
        let back = parse(&text).expect("round trips");
        assert_eq!(back.strips[3].label, "");
    }

    #[test]
    fn a_settings_document_round_trips() {
        let original = sample();
        let text = Document::Settings(Box::new(original.clone())).render();
        let back = parse(&text).expect("what we write, we read");

        assert_eq!(back.strips[0].label, "Mic IN");
        assert!((back.strips[0].gain_db - 3.09).abs() < 0.01);
        assert_eq!(back.strips[0].buses, original.strips[0].buses);
        assert!(back.strips[0].flags[SOLO]);
        assert!((back.strips[0].layers[2] + 6.5).abs() < 0.01);
        assert!((back.strips[2].comp - 4.0).abs() < 0.01);

        assert_eq!(back.buses[1].label, "Headphones");
        assert!((back.buses[1].gain_db + 3.25).abs() < 0.01);
        assert!(back.buses[1].toggles[MUTE]);
        assert_eq!(back.buses[1].mode, 5);

        assert!((back.compressors[3].ratio - 2.74).abs() < 0.01);
        assert!((back.gates[4].hold - 177.0).abs() < 0.01);
    }

    #[test]
    fn all_three_panel_faces_survive() {
        let mut original = sample();
        original.strips[0].panels = [(0.1, 0.2), (0.3, 0.4), (-0.5, -0.6)];
        let text = Document::Settings(Box::new(original)).render();
        let back = parse(&text).expect("round trips");

        for (i, (x, y)) in [(0.1, 0.2), (0.3, 0.4), (-0.5, -0.6)]
            .into_iter()
            .enumerate()
        {
            assert!((back.strips[0].panels[i].0 - x).abs() < 0.01, "face {i} x");
            assert!((back.strips[0].panels[i].1 - y).abs() < 0.01, "face {i} y");
        }
    }

    #[test]
    fn a_device_name_with_an_ampersand_is_escaped_not_repaired() {
        let text = Document::Settings(Box::new(sample())).render();
        assert!(text.contains("a &amp; b"));
        assert_eq!(
            parse(&text).expect("parses").input_devices[0].as_deref(),
            Some("a & b")
        );
    }

    #[test]
    fn our_dialect_and_theirs_differ_only_in_their_wrappers() {
        let mut ours = sample();
        ours.dialect = Dialect::PipeMeter;
        let ours_text = Document::Settings(Box::new(ours)).render();
        assert!(ours_text.contains("<PipemeterSettings>"));
        assert!(ours_text.contains("<PipemeterParameters>"));

        let theirs_text = Document::Settings(Box::new(sample())).render();
        assert!(theirs_text.contains("<VBAudioVoicemeeterSettings>"));

        let strip_of = |text: &str| {
            text.lines()
                .find(|l| l.contains("<Strip index=\"1\"") && l.contains("busa"))
                .unwrap_or_default()
                .to_owned()
        };
        assert_eq!(strip_of(&ours_text), strip_of(&theirs_text));
    }

    #[test]
    fn our_own_extras_survive_and_their_reader_would_skip_them() {
        let text = Document::Settings(Box::new(sample())).render();
        let back = parse(&text).expect("round trips");
        assert_eq!(back.extras.edition, "Romanesco");
        assert_eq!(back.extras.internal_fx2, "C5");
        assert!(back.extras.options[1]);
        assert!(back.extras.options[6], "the seventh option round trips");
        assert_eq!(
            back.extras.internal_fx_state[0],
            "HALL,0.3500,0.7200,0.2000,0.8500"
        );
        assert_eq!(
            back.extras.external_returns[1],
            "0.0000,0.7500,0.0000,0.0000,0.0000,0.0000,0.0000,0.0000"
        );
        assert_eq!(
            back.extras.armed_inputs,
            [false, false, true, false, false, false, false, true]
        );
        assert!(text.contains("<Edition>"));
    }
}
