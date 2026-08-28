//! Reading the mixer out of a settings or scene document.
//!
//! One pass over every element, dispatching on the tag name and, where tags
//! are reused, on which attributes an element carries. That second test is
//! the awkward part of this format and the reason a DOM is used rather than
//! a stream: `<Strip index='1'>` appears once for routing, again for the
//! compressor, again for a parametric EQ cell, and only its attributes say
//! which one it is.

use crate::blocks::{
    C5Band, Compressor, DeviceOptions, EqCell, Gate, Pitch,
};
use crate::read::{flag, flag_f32, index_of, mode_index, suffix};
use crate::{Bus, EQ_ON, Edition, Error, Imported, MONO, MUTE, SOLO, Strip};
use crate::{BUS_ATTRS, BUS_EQ, BUS_MONO, BUS_MUTE, BUS_SEL};

/// Read the mixer out of a settings or scene document.
pub(crate) fn read_settings(root: &roxmltree::Node<'_, '_>) -> Result<Imported, Error> {
    read_settings_from(root, None)
}

/// Read the mixer, and when the source text is to hand, keep the A/B memory
/// sections as slices of it so they can be written back untouched.
pub(crate) fn read_settings_from(
    root: &roxmltree::Node<'_, '_>,
    source: Option<&str>,
) -> Result<Imported, Error> {
    let mut imported = Imported {
        strips: vec![Strip::default(); 8],
        buses: vec![Bus::default(); 8],
        input_devices: vec![None; 5],
        output_devices: vec![None; 5],
        compressors: vec![Compressor::default(); 8],
        gates: vec![Gate::default(); 8],
        pitches: vec![Pitch::default(); 8],
        ..Imported::default()
    };
    // Anything we understood at all counts. A partial file - one Strip
    // carrying only its EQ, which is what a saved panel preset looks like -
    // is a real document and must not be refused for lacking routing rows.
    let mut found_anything = false;
    // The highest one-based index any element mentions, which is what says
    // which edition wrote the file.
    let mut highest_strip = 0;
    let mut highest_bus = 0;

    for node in root.descendants() {
        let tag = node.tag_name().name();
        // A memory section is copied whole and then skipped, along with
        // everything inside it.
        if tag.ends_with("Mem") && !in_memory(&node) {
            if let Some(text) = source.and_then(|text| slice_of(text, &node)) {
                imported.memories.push(text);
            }
            continue;
        }
        if let Some(index) = index_of(&node) {
            match tag {
                "Strip" | "StripComp" | "StripGate" | "StripPitch" => {
                    highest_strip = highest_strip.max(index + 1);
                }
                "Bus" => highest_bus = highest_bus.max(index + 1),
                _ => {}
            }
        }
        if element(&node, tag, &mut imported) {
            found_anything = true;
        }
    }

    if !found_anything {
        return Err(Error::NotVoicemeeter);
    }
    // The edition is not written anywhere; the highest slot the file
    // addresses is the statement. Counting slots that *carry* something
    // instead would call a Potato file with one strip set up a Standard one.
    imported.edition = Edition::of(highest_strip, highest_bus);
    Ok(imported)
}



/// Read one element into the document. Returns whether it was recognised.
///
/// Split out because it is a long flat dispatch and reads better on its own
/// than nested three deep inside the walk.
fn element(node: &roxmltree::Node<'_, '_>, tag: &str, imported: &mut Imported) -> bool {
    match tag {
        // The routing element is the one carrying bus assignments; the
        // compressor and gate elements share its tag and index.
        // The panel coordinates live on a different <Strip> element from
        // the routing one, so they get their own arm.
        // EQ cells share the Strip and Bus tags too, and are told apart
        // by carrying a cell number. Only the live ones are taken: the
        // `*mem` elements beside them are the A/B compare memories.
        "Strip" if node.has_attribute("cell") => {
            imported.strip_eq.push(EqCell::read(node));
                    }
        "Bus" if node.has_attribute("cell") => {
            imported.bus_eq.push(EqCell::read(node));
                    }
        "Strip" if node.has_attribute("ColorPanelx") || node.has_attribute("Panel3Dx") => {
            if let Some(slot) = index_of(node).and_then(|i| imported.strips.get_mut(i)) {
                read_panel(node, slot);
                            }
        }
        "Strip" if node.has_attribute("busa") => {
            if let Some(slot) = index_of(node).and_then(|i| imported.strips.get_mut(i)) {
                read_strip(node, slot);
                            }
        }
        // Likewise the bus element carrying BusMode, not the EQ cells.
        "Bus" if node.has_attribute("BusMode") => {
            if let Some(slot) = index_of(node).and_then(|i| imported.buses.get_mut(i)) {
                read_bus(node, slot);
                            }
        }
        // The compressor, gate and pitch blocks share the Strip index
        // but not its attributes, which is how they are told apart.
        "StripComp" => {
                        if let Some(slot) = index_of(node).and_then(|i| imported.compressors.get_mut(i)) {
                *slot = Compressor::read(node);
            }
        }
        "StripGate" => {
            if let Some(slot) = index_of(node).and_then(|i| imported.gates.get_mut(i)) {
                *slot = Gate::read(node);
            }
        }
        "StripPitch" => {
            if let Some(slot) = index_of(node).and_then(|i| imported.pitches.get_mut(i)) {
                *slot = Pitch::read(node);
            }
        }
        // The effects appear once live and again inside the A/B memory
        // sections, where the later copies are blank. Taking the last
        // one seen would wipe the preset name off the panel.
        "ReverbGeneral" if !in_memory(node) => imported.reverb.read_general(node),
        "ReverbParam" if !in_memory(node) => imported.reverb.read_param(node),
        "MultiTapGeneral" if !in_memory(node) => imported.delay.read_general(node),
        "MultiTapDelay" if !in_memory(node) => imported.delay.read_delay(node),
        "C5LimGeneral" if !in_memory(node) => imported.c5.read_general(node),
        "C5LimBand" if !in_memory(node) => imported.c5.band.push(C5Band::read(node)),
        "ExternalFXsend" => imported.external_patch.read_send(node),
        "ExternalFXreturn" => imported.external_patch.read_return(node),
        "OptionDev" => imported.device_options = DeviceOptions::read(node),
        "InputDev" => {
            read_device(node, &mut imported.input_devices);
                    }
        "OutputDev" => {
            read_device(node, &mut imported.output_devices);
                    }
        "Edition" => imported.extras.edition = text_of(node),
        "InternalFx2" => imported.extras.internal_fx2 = text_of(node),
        "InternalFxState1" => imported.extras.internal_fx_state[0] = text_of(node),
        "InternalFxState2" => imported.extras.internal_fx_state[1] = text_of(node),
        "MenuOptions" => read_options(node, &mut imported.extras.options),
        "ArmedInputs" => read_options(node, &mut imported.extras.armed_inputs),
        // A label is the only arm that may not match anything, so it is the
        // one that decides whether this element counted.
        other => return read_label(other, node, imported),
    }
    true
}

/// Send knobs and their Post toggles, in mixer order.
pub(crate) const SEND_ATTRS: [(&str, &str); 4] = [
    ("sendR", "postR"),
    ("sendD", "postD"),
    ("sendFx1", "postFx1"),
    ("sendFx2", "postFx2"),
];

fn read_strip(node: &roxmltree::Node<'_, '_>, strip: &mut Strip) {
    strip.gain_db = flag_f32(node, "dblevel");
    strip.flags[MUTE] = flag(node, "mute");
    strip.flags[MONO] = flag(node, "mono");
    strip.karaoke = flag(node, "karaoke");
    strip.layout = mode_index(flag_f32(node, "layout"));
    strip.virtual_in = flag(node, "vaio");
    strip.mc = flag(node, "muc");
    for (i, attr) in ["prg_c", "prg_g", "prg_d"].into_iter().enumerate() {
        strip.programs[i] = mode_index(flag_f32(node, attr));
    }
    strip.limit_db = flag_f32(node, "dblimit");
    // Layers are one-based in the file and carry the fader level the strip
    // takes when that scene is recalled.
    for (i, slot) in strip.layers.iter_mut().enumerate() {
        *slot = flag_f32(node, &format!("layer{}", i + 1));
    }
    strip.flags[SOLO] = flag(node, "solo");
    for (slot, attr) in strip.buses.iter_mut().zip(BUS_ATTRS) {
        *slot = flag(node, attr);
    }

    strip.comp = flag_f32(node, "audibility_c");
    strip.gate = flag_f32(node, "audibility_g");
    strip.denoiser = flag_f32(node, "denoiser_g");
    strip.flags[EQ_ON] = flag(node, "EQon");
    for (i, attr) in ["EQGain1", "EQGain2", "EQGain3"].into_iter().enumerate() {
        strip.eq_gain[i] = flag_f32(node, attr);
    }
    for (i, (send, post)) in SEND_ATTRS.into_iter().enumerate() {
        strip.sends[i] = flag_f32(node, send);
        strip.post[i] = flag(node, post);
    }
}

/// The DUMBPAN handle, which lives on whichever panel element carries it.
///
/// Voicemeeter writes a separate `<Strip>` per panel mode, each with its own
/// coordinate pair, so this is read from the element that has them rather
/// than from the routing one.
fn read_panel(node: &roxmltree::Node<'_, '_>, strip: &mut Strip) {
    // Each face writes its own pair, and an element may carry more than one,
    // so every pair present is taken rather than the first.
    // Ours, and only in our own files. Clamped rather than trusted: an index
    // past the end would panic every draw.
    if node.has_attribute("PanelFace") {
        strip.panel_face = (mode_index(flag_f32(node, "PanelFace")) as usize).min(2);
    }
    for (slot, (x, y)) in PANEL_ATTRS.into_iter().enumerate() {
        if node.has_attribute(x) {
            strip.panels[slot] = (flag_f32(node, x), flag_f32(node, y));
        }
    }
}

/// The attribute pair each DUMBPAN face uses, in [`Strip::panels`] order.
pub(crate) const PANEL_ATTRS: [(&str, &str); 3] = [
    ("ColorPanelx", "ColorPanely"),
    ("ModPanelx", "ModPanely"),
    ("Panel3Dx", "Panel3Dy"),
];

fn text_of(node: &roxmltree::Node<'_, '_>) -> String {
    node.text().unwrap_or_default().trim().to_owned()
}

/// The menu options row: six flags, comma separated.
///
/// All six or none. A partial row would half-apply, which is worse than
/// falling back to the defaults.
fn read_options<const N: usize>(node: &roxmltree::Node<'_, '_>, into: &mut [bool; N]) {
    let raw = text_of(node);
    let fields: Vec<&str> = raw.split(',').collect();
    if fields.len() != into.len() {
        return;
    }
    for (slot, field) in into.iter_mut().zip(fields) {
        *slot = field.trim() == "1";
    }
}

fn read_bus(node: &roxmltree::Node<'_, '_>, bus: &mut Bus) {
    bus.gain_db = flag_f32(node, "dblevel");
    // Kept as the file's own number; the mixer turns it into a mode. A
    // negative or absurd value falls back to zero rather than wrapping.
    bus.mode = mode_index(flag_f32(node, "BusMode"));
    bus.virtual_out = flag(node, "vaio");
    bus.cross = flag(node, "cross");
    bus.monitor = flag(node, "monitor");
    for (i, attr) in ["retR", "retD", "retFx1", "retFx2"].into_iter().enumerate() {
        bus.returns[i] = flag_f32(node, attr);
    }
    bus.toggles[BUS_SEL] = flag(node, "SEL");
    bus.toggles[BUS_MONO] = flag(node, "mono");
    bus.toggles[BUS_EQ] = flag(node, "EQon");
    bus.toggles[BUS_MUTE] = flag(node, "mute");
}

/// `<InputDev index='1' name="Microphone (WG2)" />`. A lone dash means the
/// slot is empty, which is how Voicemeeter writes "no device".
/// Whether an element sits inside one of the A/B compare memory sections
/// rather than in the live settings.
/// The source text an element occupies, if its range can be trusted.
fn slice_of(source: &str, node: &roxmltree::Node<'_, '_>) -> Option<String> {
    let range = node.range();
    source.get(range).map(str::to_owned)
}

fn in_memory(node: &roxmltree::Node<'_, '_>) -> bool {
    // Skipping self matters: roxmltree counts a node among its own
    // ancestors, so without this a memory section is "inside a memory
    // section" and the capture below skips the very thing it is for.
    node.ancestors()
        .skip(1)
        .any(|a| a.tag_name().name().ends_with("Mem"))
}

fn read_device(node: &roxmltree::Node<'_, '_>, slots: &mut [Option<String>]) {
    let Some(slot) = index_of(node).and_then(|i| slots.get_mut(i)) else {
        return;
    };
    *slot = match node.attribute("name") {
        Some("-") | None => None,
        Some(name) if name.trim().is_empty() => None,
        Some(name) => Some(name.to_owned()),
    };
}

/// Labels live in their own elements, named by position rather than indexed:
/// `LabelStrip1`, `LabelVirtualStrip1`, `LabelBus1`, `LabelVirtualBus1`.
fn read_label(tag: &str, node: &roxmltree::Node<'_, '_>, into: &mut Imported) -> bool {
    let Some(text) = node.text().map(str::trim).filter(|t| !t.is_empty()) else {
        return false;
    };

    // Hardware strips occupy 0-4 and virtual strips 5-7; the same split
    // applies to the A and B buses.
    let target = if let Some(n) = suffix(tag, "LabelVirtualStrip") {
        into.strips.get_mut(4 + n)
    } else if let Some(n) = suffix(tag, "LabelStrip") {
        into.strips.get_mut(n - 1)
    } else if let Some(n) = suffix(tag, "LabelVirtualBus") {
        return set_bus_label(into, 4 + n, text);
    } else if let Some(n) = suffix(tag, "LabelBus") {
        return set_bus_label(into, n - 1, text);
    } else {
        None
    };

    if let Some(strip) = target {
        text.clone_into(&mut strip.label);
        return true;
    }
    false
}

fn set_bus_label(into: &mut Imported, index: usize, text: &str) -> bool {
    if let Some(bus) = into.buses.get_mut(index) {
        text.clone_into(&mut bus.label);
        return true;
    }
    false
}


