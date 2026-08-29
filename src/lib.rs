//! Reading Voicemeeter's XML settings files.
//!
//! Split out of the mixer because it is pure data: no audio, no GUI, and no
//! reason for either to be in the way of the other. A real settings file
//! carries far more than the mixer currently reads - hundreds of EQ cells, a
//! compressor and gate per strip, three FX sections, MIDI mappings - so this
//! is also the part with the most left to grow.
//!
//! A Windows Voicemeeter user can drop their `VoicemeeterPotato*.xml` in and
//! keep their labels, fader levels, mutes and the whole routing matrix. What
//! cannot carry over is device assignment: the file names Windows devices
//! (WDM/KS/ASIO endpoints) that have no counterpart here, so those are parsed
//! but surfaced separately for the user to reassign by hand.
//!
//! The format is quirky in one way that matters: several elements share the
//! same tag and index but carry different attributes — `<Strip index='1'>`
//! appears once for routing, again for the compressor, again for the gate.
//! Elements are therefore selected by *which attributes they carry*, not by
//! position.

use std::fmt;

pub mod blocks;
pub mod macros;
pub mod midi;
mod read;
pub mod scene;
mod settings;
pub mod vban;
mod write;

use blocks::{C5, Compressor, Delay, DeviceOptions, EqCell, ExternalPatch, Gate, Pitch, Reverb};

/// Which of Voicemeeter's documents a file is.
///
/// They all share one element vocabulary but sit under different roots, and
/// a user pointing at "my Voicemeeter settings" may well mean any of them.
/// Sniffing the root is what lets one entry point do the right thing instead
/// of making the caller know which reader to reach for.
#[derive(Debug, Clone, PartialEq)]
pub enum Document {
    /// The mixer itself, in any of the three editions.
    Settings(Box<Imported>),
    Vban(vban::Config),
    Midi(midi::Map),
    MacroButtons(macros::Buttons),
    /// A recalled scene, which is a settings document wearing a preset name.
    Scene(Box<scene::Scene>),
    /// A voice-modeler pitch preset.
    PitchPreset(Box<scene::PitchPreset>),
}

impl Document {
    /// Read whichever document this is.
    ///
    /// # Errors
    ///
    /// Fails if the text is not XML at all, or if its root is not one this
    /// crate recognises. Everything *inside* a recognised document is read
    /// forgivingly: an element this build has never heard of is skipped, not
    /// refused, because settings files come from other versions.
    pub fn parse(xml: &str) -> Result<Self, Error> {
        let xml = repair(xml);
        let doc = roxmltree::Document::parse(&xml).map_err(Error::Xml)?;
        let root = doc.root_element();
        Ok(match root.tag_name().name() {
            "VBAudioVoicemeeterSettings" | ROOT_PIPEMETER => {
                let mut imported = settings::read_settings_from(&root, Some(&xml))?;
                imported.dialect = Dialect::of(root.tag_name().name());
                Self::Settings(Box::new(imported))
            }
            "VBAudioVoicemeeterVBANConfig" => Self::Vban(vban::Config::read(&root)),
            "VBAudioVoicemeeterMIDIMapping" => Self::Midi(midi::Map::read(&root)),
            "VBAudioVoicemeeterMacroButtonMap" => Self::MacroButtons(macros::Buttons::read(&root)),
            "VBAudioVoicemeeterPresetScene" => Self::Scene(Box::new(scene::Scene::read(&root)?)),
            "VBAudioVoicemeeterPresetPitch" => {
                Self::PitchPreset(Box::new(scene::PitchPreset::read(&root)))
            }
            other => return Err(Error::UnknownDocument(other.to_owned())),
        })
    }

    /// Read a file, choosing the reader from its contents rather than its
    /// name — the names vary by edition, by day, and by who saved it.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be read, or if [`Self::parse`] refuses it.
    pub fn load(path: &std::path::Path) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path).map_err(Error::Io)?;
        Self::parse(&text)
    }

    /// What to call this kind of document when telling the user what they
    /// picked.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Settings(_) => "mixer settings",
            Self::Vban(_) => "VBAN configuration",
            Self::Midi(_) => "MIDI mapping",
            Self::MacroButtons(_) => "MacroButtons",
            Self::Scene(_) => "scene",
            Self::PitchPreset(_) => "voice-modeler preset",
        }
    }

    /// The mixer settings, if that is what this document holds.
    #[must_use]
    pub fn settings(&self) -> Option<&Imported> {
        match self {
            Self::Settings(imported) => Some(imported),
            Self::Scene(scene) => Some(&scene.settings),
            _ => None,
        }
    }
}

/// The root element `PipeMeter` writes.
pub const ROOT_PIPEMETER: &str = "PipemeterSettings";
/// The root element Voicemeeter writes.
pub const ROOT_VOICEMEETER: &str = "VBAudioVoicemeeterSettings";

/// Whose settings file this is.
///
/// The two are the same format: `PipeMeter`'s is Voicemeeter's with a
/// different root and a handful of extra elements. Knowing which one came in
/// matters only for what goes back out, and for telling the user what they
/// just opened.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dialect {
    #[default]
    Voicemeeter,
    PipeMeter,
}

impl Dialect {
    #[must_use]
    fn of(root: &str) -> Self {
        if root == ROOT_PIPEMETER {
            Self::PipeMeter
        } else {
            Self::Voicemeeter
        }
    }

    /// The root element to write for this dialect.
    #[must_use]
    pub fn root(self) -> &'static str {
        match self {
            Self::Voicemeeter => ROOT_VOICEMEETER,
            Self::PipeMeter => ROOT_PIPEMETER,
        }
    }
}

/// Which edition of Voicemeeter a settings file came from.
///
/// The editions differ in size, and the file says so only by how many strips
/// and buses it carries. Knowing which is what lets an import from a smaller
/// edition fill the strips it has rather than being read as a Potato file
/// with five empty ones.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Edition {
    /// Two hardware strips and one virtual, two buses.
    Standard,
    /// Three hardware strips and two virtual, five buses.
    Banana,
    /// Five hardware strips and three virtual, eight buses. The default
    /// because it is the largest: reading a file as bigger than it is loses
    /// nothing, reading it as smaller would drop strips.
    #[default]
    Potato,
}

impl Edition {
    /// The edition a file with this many strips and buses came from.
    ///
    /// Anything unrecognised is treated as Potato, the largest: reading a
    /// file as bigger than it is loses nothing, whereas reading it as
    /// smaller would drop strips.
    #[must_use]
    pub fn of(strips: usize, buses: usize) -> Self {
        match (strips, buses) {
            (0..=3, 0..=2) => Self::Standard,
            (0..=5, 0..=5) => Self::Banana,
            _ => Self::Potato,
        }
    }

    /// How many strips and buses this edition has.
    #[must_use]
    pub fn size(self) -> (usize, usize) {
        match self {
            Self::Standard => (3, 2),
            Self::Banana => (5, 5),
            Self::Potato => (8, 8),
        }
    }
}

/// Everything we could read out of a settings file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Imported {
    /// Eight strips: five hardware, then three virtual.
    pub strips: Vec<Strip>,
    /// Eight buses: A1-A5, then B1-B3.
    pub buses: Vec<Bus>,
    /// Windows device names, in file order, for the five inputs and five
    /// outputs. Informational only — they cannot be resolved on Linux.
    pub input_devices: Vec<Option<String>>,
    pub output_devices: Vec<Option<String>>,
    /// One compressor, gate and pitch block per strip.
    pub compressors: Vec<Compressor>,
    pub gates: Vec<Gate>,
    pub pitches: Vec<Pitch>,
    /// Parametric EQ cells, flat. There are hundreds; each carries its own
    /// channel and cell number, so nesting them by strip would only make
    /// them harder to look up.
    pub strip_eq: Vec<EqCell>,
    pub bus_eq: Vec<EqCell>,
    pub reverb: Reverb,
    pub delay: Delay,
    pub c5: C5,
    pub external_patch: ExternalPatch,
    pub device_options: DeviceOptions,
    /// Which edition wrote the file, inferred from how much of it there is.
    pub edition: Edition,
    /// Whether this came from Voicemeeter or from `PipeMeter`.
    pub dialect: Dialect,
    /// The elements only our own dialect carries.
    pub extras: Extras,
    /// The A/B compare memories, kept as the text they arrived as.
    ///
    /// Deliberately not modelled. They are copies of sections this program
    /// does not edit — thirty-eight of them in a real file, most blank — and
    /// a type for each would be a lot of code whose only job is to hand back
    /// what it was given. Keeping the source text is both shorter and more
    /// faithful: nothing can be lost in a translation that never happens.
    ///
    /// Without this a load followed by a save silently dropped every one.
    pub memories: Vec<String>,
}

/// Settings that are `PipeMeter`'s alone.
///
/// Kept in the same struct as everything else rather than beside it: our
/// dialect is Voicemeeter's plus these, and separating them would mean two
/// things to pass around wherever one document is meant.
///
/// Voicemeeter's own reader skips elements it does not know, exactly as this
/// one does, so a file carrying them still loads there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extras {
    /// The word beside the wordmark.
    pub edition: String,
    /// Which effect the second internal FX slot holds, `Delay` or `C5`.
    pub internal_fx2: String,
    /// The menu's checkable options, in the mixer's own order.
    pub options: [bool; 8],
    /// The settings file last opened by hand, reopened at startup when the
    /// menu says so. Empty means the ordinary session.
    pub startup_settings: String,
    /// Where the recorder writes its takes. Empty means the default beside
    /// the settings.
    pub recording_dir: String,
    /// Each internal FX slot's state: the preset name it is set to, empty
    /// for off, then its knob positions. Written as one string per slot so
    /// a slot that gains a knob later does not invalidate the file.
    pub internal_fx_state: [String; 2],
    /// Which pre-fader inputs the recorder page has armed, in strip order.
    pub armed_inputs: [bool; 8],
    /// The External FX Return grid: two rows of eight, one per bus, held
    /// as text so a row that gains a bus later does not invalidate a file.
    pub external_returns: [String; 2],
}

/// One strip's transferable settings.
///
/// Covers every `<Strip>` attribute that has somewhere to go in the mixer.
/// The ones left on the floor are listed in `DESIGN.md`; they belong to
/// features that do not exist here yet, so parsing them would only produce
/// values nothing reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Strip {
    pub label: String,
    /// Channel layout the strip is set to, as the file numbers it.
    pub layout: u32,
    /// Whether the strip is one of the virtual ones.
    pub virtual_in: bool,
    /// The `M.C.` button — mono/centre, on virtual strips.
    pub mc: bool,
    /// Which preset program each of the three AUDIBILITY knobs is on.
    pub programs: [u32; 3],
    /// Fader ceiling, in dB.
    pub limit_db: f32,
    /// The eight fader layers. A scene recall swaps between them, which is
    /// what the eight scene buttons in the banner are for.
    pub layers: [f32; 8],
    /// Whether this strip's first button is the karaoke one.
    pub karaoke: bool,
    pub gain_db: f32,
    /// Mute, mono, solo and EQ-on. An array rather than four named flags,
    /// matching how [`Bus`] stores its own and keeping the struct under
    /// clippy's bool limit. Index with the constants below.
    pub flags: [bool; 4],
    /// Routing, indexed A1-A5 then B1-B3 to match the mixer's own order.
    pub buses: [bool; 8],
    /// Comp. and Gate knobs, 0.0..=10.0 as Voicemeeter stores them.
    pub comp: f32,
    pub gate: f32,
    pub denoiser: f32,
    /// pan handle per face — colour, modulation, 3D position, in that
    /// order — each -1.0..=1.0 on both axes in the file.
    ///
    /// Three, not one: the faces control different things and each keeps its
    /// own handle. Collapsing them into a single position, as this used to,
    /// silently discarded two thirds of what the file said.
    pub panels: [(f32, f32); 3],
    /// Which of those three faces the pad is showing, 0-2.
    ///
    /// Ours rather than Voicemeeter's: the original does not record it, so
    /// this is written into our own dialect and read back only from a file
    /// that has it. Without it every strip came back on the first face and
    /// a mixer set up across all three had to be reset by hand each launch.
    pub panel_face: usize,
    /// The three EQ gains on a virtual strip, in dB.
    pub eq_gain: [f32; 3],
    /// Reverb, Delay, Send 1, Send 2 knobs.
    pub sends: [f32; 4],
    /// The Post toggles flanking them, same order.
    pub post: [bool; 4],
}

/// One bus's transferable settings.
///
/// The toggles are an array rather than named flags, matching how the mixer's
/// the mixer stores them and keeping clippy's bool limit
/// happy. Index with the `SEL`/`MONO`/`EQ`/`MUTE` constants from `master`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bus {
    pub label: String,
    /// Whether the bus is one of the virtual ones.
    pub virtual_out: bool,
    /// The cross-feed toggle.
    pub cross: bool,
    /// Mix mode, as the file numbers them.
    pub mode: u32,
    pub gain_db: f32,
    pub toggles: [bool; 4],
    /// FX returns: reverb, delay, FX1, FX2.
    pub returns: [f32; 4],
    /// Whether this bus is the monitored one.
    pub monitor: bool,
}

/// Indices into [`Strip::panels`].
pub const PANEL_COLOUR: usize = 0;
pub const PANEL_MODULATION: usize = 1;
pub const PANEL_3D: usize = 2;

/// Indices into [`Strip::flags`].
pub const MUTE: usize = 0;
pub const MONO: usize = 1;

/// Indices into [`Bus::toggles`]. Numbered separately from the strip flags
/// above because the two carry different things in a different order, and a
/// single shared set would silently mean the wrong one somewhere.
pub const BUS_SEL: usize = 0;
pub const BUS_MONO: usize = 1;
pub const BUS_EQ: usize = 2;
pub const BUS_MUTE: usize = 3;
pub const SOLO: usize = 2;
pub const EQ_ON: usize = 3;

/// Why an import failed.
#[derive(Debug)]
pub enum Error {
    Xml(roxmltree::Error),
    Io(std::io::Error),
    /// Parsed, but nothing recognisable was in it.
    NotVoicemeeter,
    /// A root element this crate does not know.
    UnknownDocument(String),
    /// A valid document, but not the kind the caller asked for.
    WrongDocument(Document),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(e) => write!(f, "not valid XML: {e}"),
            Self::Io(e) => write!(f, "could not be read: {e}"),
            Self::NotVoicemeeter => {
                write!(f, "no Voicemeeter strips or buses found in this file")
            }
            Self::UnknownDocument(root) => {
                write!(f, "not a Voicemeeter document: root element <{root}>")
            }
            Self::WrongDocument(found) => {
                write!(f, "this is a {} file, not mixer settings", found.kind())
            }
        }
    }
}

impl std::error::Error for Error {}

/// The `<Strip>` attributes naming each bus assignment, in mixer order.
/// Voicemeeter numbers the first of each group without a suffix.
const BUS_ATTRS: [&str; 8] = [
    "busa", "busa2", "busa3", "busa4", "busa5", "busb", "busb2", "busb3",
];

/// Repair the one malformation Voicemeeter actually writes.
///
/// Real settings files open `<VoiceMeeterParameters>` twice in a row and
/// close it once, which is not well-formed XML and which every strict parser
/// rejects. Rather than reach for a lenient parser and lose all the other
/// checks, the duplicate opening tag is dropped first.
///
/// Only *consecutive* duplicates are removed, so a genuinely nested document
/// is left alone.
fn repair(xml: &str) -> std::borrow::Cow<'_, str> {
    let mut stack: Vec<&str> = Vec::new();
    let mut previous_open: Option<&str> = None;
    let mut out = String::new();
    let mut repaired = false;

    for line in xml.lines() {
        let trimmed = line.trim();
        let name = opening_tag(trimmed);

        if let Some(name) = name {
            if previous_open == Some(trimmed) {
                repaired = true;
                continue;
            }
            if stack.last() == Some(&name) {
                repaired = true;
                stack.pop();
                out.push_str("</");
                out.push_str(name);
                out.push_str(">\n");
                previous_open = None;
                continue;
            }
            stack.push(name);
        } else if let Some(name) = closing_tag(trimmed)
            && stack.last() == Some(&name)
        {
            stack.pop();
        }

        previous_open = name.map(|_| trimmed);
        out.push_str(line);
        out.push('\n');
    }

    let escaped = escape_bare_ampersands(&out);
    if repaired || escaped.len() != out.len() {
        std::borrow::Cow::Owned(escaped)
    } else {
        std::borrow::Cow::Borrowed(xml)
    }
}

/// The element name, if this line is a plain opening tag.
///
/// Self-closing tags, closing tags and declarations are all excluded: only a
/// tag that leaves an element open can be the half of a pair that goes
/// missing.
fn opening_tag(trimmed: &str) -> Option<&str> {
    if !trimmed.starts_with('<')
        || !trimmed.ends_with('>')
        || trimmed.starts_with("</")
        || trimmed.ends_with("/>")
        || trimmed.starts_with("<?")
        || trimmed.starts_with("<!")
    {
        return None;
    }
    Some(element_name(&trimmed[1..]))
}

fn closing_tag(trimmed: &str) -> Option<&str> {
    trimmed.strip_prefix("</").map(element_name)
}

/// The name at the start of a tag body, up to the first space or bracket.
fn element_name(body: &str) -> &str {
    let end = body
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(body.len());
    &body[..end]
}

/// Escape ampersands that are not already the start of an entity.
///
/// Voicemeeter writes device names into attributes without escaping them, so
/// a card called "Y&H Game Live" produces a file that is not XML at all and
/// that no conforming parser will touch. The name is the user's, not ours,
/// and refusing their settings over their sound card's branding would be
/// absurd — so the ampersand is repaired on the way in.
///
/// Only bare ones: anything already written as `&amp;`, `&lt;` or a numeric
/// reference is left exactly as it is, or loading a correct file twice would
/// double-escape it.
fn escape_bare_ampersands(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;

    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if is_entity(tail) {
            out.push('&');
        } else {
            out.push_str("&amp;");
        }
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// Whether text starting at an ampersand is a well-formed entity reference.
fn is_entity(tail: &str) -> bool {
    let Some(end) = tail.find(';') else {
        return false;
    };
    let body = &tail[1..end];
    if body.is_empty() {
        return false;
    }
    if let Some(digits) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
        return !digits.is_empty() && digits.chars().all(|c| c.is_ascii_hexdigit());
    }
    if let Some(digits) = body.strip_prefix('#') {
        return !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit());
    }
    body.starts_with(|c: char| c.is_ascii_alphabetic())
        && body.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Parse a Voicemeeter settings file.
///
/// # Errors
///
/// Fails only if the document cannot be parsed as XML at all. Anything the
/// reader does not recognise is skipped rather than refused: settings files
/// come from other versions and other editions, and half a mixer restored is
/// better than none.
pub fn parse(xml: &str) -> Result<Imported, Error> {
    match Document::parse(xml)? {
        Document::Settings(imported) => Ok(*imported),
        Document::Scene(scene) => Ok(scene.settings),
        other => Err(Error::WrongDocument(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::{Dialect, Edition, Error, PANEL_COLOUR, ROOT_PIPEMETER, ROOT_VOICEMEETER, parse};

    /// Trimmed from a real `VoicemeeterPotato` settings file: one hardware
    /// strip, one virtual strip, two buses, labels and devices. Includes the
    /// duplicate `Strip`/`Bus` elements that must not be mistaken for the
    /// routing ones.
    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<VBAudioVoicemeeterSettings>
<VoiceMeeterDeviceConfiguration>
    <InputDev index='1' type='4' name="Microphone (WG2)" />
    <InputDev index='2' name="-" />
    <OutputDev index='1' name="-" />
    <OutputDev index='2' type='4' name="Headphones (WG2)" />
</VoiceMeeterDeviceConfiguration>
<VoiceMeeterParameters>
    <LabelStrip1>Mic IN</LabelStrip1>
    <LabelStrip2></LabelStrip2>
    <LabelVirtualStrip1>Main OUT</LabelVirtualStrip1>
    <LabelBus2>Headphones</LabelBus2>
    <LabelVirtualBus1>Mic IN</LabelVirtualBus1>
    <Strip index='1' layout='0' mute='0' solo='0' mono='1' busa='0' busa2='0'
        busa3='0' busa4='1' busa5='0' busb='1' busb2='1' busb3='0' dblevel='3.09'
        audibility_c='1.9' audibility_g='0.8' denoiser_g='0.5' EQon='1'
        EQGain1='-12.0' EQGain2='3.5' EQGain3='0.0'
        sendR='1.5' postR='1' sendD='0.0' postD='0'
        sendFx1='2.5' postFx1='0' sendFx2='0.0' postFx2='1' />
    <StripComp index='1' gainin='0.00' threshold='-40.00' />
    <Strip index='1' paneltype='1' ColorPanelx='-0.178' ColorPanely='0.250' />
    <Strip index='6' mute='1' solo='0' mono='0' busa='1' busa2='0' busa3='0'
        busa4='0' busa5='0' busb='0' busb2='0' busb3='0' dblevel='-6.50' />
    <Bus index='2' mute='0' mono='1' cross='0' BusMode='2' EQon='1' SEL='0'
        monitor='1' retR='4.0' retD='0.0' retFx1='1.25' retFx2='0.0' dblevel='-3.25' />
    <Bus index='2' channel='1' cell='1' gain='0.00' freq='50.00' />
</VoiceMeeterParameters>
</VBAudioVoicemeeterSettings>"#;

    #[test]
    fn reads_routing_from_the_right_duplicate_element() {
        let out = parse(SAMPLE).expect("parses");
        let strip = &out.strips[0];
        assert_eq!(
            strip.buses,
            [false, false, false, true, false, true, true, false]
        );
        assert!((strip.gain_db - 3.09).abs() < f32::EPSILON);
        assert!(strip.flags[super::MONO]);
        assert!(!strip.flags[super::MUTE]);
    }

    #[test]
    fn virtual_strips_follow_the_hardware_ones() {
        let out = parse(SAMPLE).expect("parses");
        assert_eq!(out.strips[5].label, "Main OUT");
        assert!(out.strips[5].flags[super::MUTE]);
        assert!((out.strips[5].gain_db - -6.50).abs() < f32::EPSILON);
    }

    #[test]
    fn labels_land_in_the_right_slots() {
        let out = parse(SAMPLE).expect("parses");
        assert_eq!(out.strips[0].label, "Mic IN");
        assert_eq!(out.strips[1].label, "");
        assert_eq!(out.buses[1].label, "Headphones");
        assert_eq!(out.buses[5].label, "Mic IN");
    }

    #[test]
    fn bus_flags_come_from_the_params_element_not_the_eq_cells() {
        let out = parse(SAMPLE).expect("parses");
        let bus = &out.buses[1];
        assert!(bus.toggles[super::BUS_EQ]);
        assert!(bus.toggles[super::BUS_MONO]);
        assert!((bus.gain_db - -3.25).abs() < f32::EPSILON);
    }

    #[test]
    fn a_dash_means_no_device() {
        let out = parse(SAMPLE).expect("parses");
        assert_eq!(out.input_devices[0].as_deref(), Some("Microphone (WG2)"));
        assert_eq!(out.input_devices[1], None);
        assert_eq!(out.output_devices[0], None);
        assert_eq!(out.output_devices[1].as_deref(), Some("Headphones (WG2)"));
    }

    #[test]
    fn strip_effects_and_sends_are_read() {
        let out = parse(SAMPLE).expect("parses");
        let strip = &out.strips[0];
        assert!((strip.comp - 1.9).abs() < 0.001);
        assert!((strip.gate - 0.8).abs() < 0.001);
        assert!((strip.denoiser - 0.5).abs() < 0.001);
        assert!(strip.flags[super::EQ_ON]);
        assert!((strip.eq_gain[0] - -12.0).abs() < 0.001);
        assert!((strip.eq_gain[1] - 3.5).abs() < 0.001);
        assert!((strip.sends[0] - 1.5).abs() < 0.001);
        assert!((strip.sends[2] - 2.5).abs() < 0.001);
        assert_eq!(strip.post, [true, false, false, true]);
    }

    #[test]
    fn the_panel_handle_comes_from_its_own_element() {
        let out = parse(SAMPLE).expect("parses");
        let colour = out.strips[0].panels[PANEL_COLOUR];
        assert!((colour.0 - -0.178).abs() < 0.001);
        assert!((colour.1 - 0.250).abs() < 0.001);
        assert_eq!(
            out.strips[0].buses,
            [false, false, false, true, false, true, true, false]
        );
    }

    #[test]
    fn a_nonsense_bus_mode_falls_back_to_the_first() {
        assert_eq!(crate::read::mode_index(2.0), 2);
        assert_eq!(crate::read::mode_index(-1.0), 0);
        assert_eq!(crate::read::mode_index(f32::NAN), 0);
    }

    #[test]
    fn bus_returns_and_monitor_are_read() {
        let out = parse(SAMPLE).expect("parses");
        let bus = &out.buses[1];
        assert!(bus.monitor);
        assert!((bus.returns[0] - 4.0).abs() < 0.001);
        assert!((bus.returns[2] - 1.25).abs() < 0.001);
    }

    #[test]
    fn the_edition_comes_from_the_highest_slot_addressed() {
        let sparse = format!(
            "<{ROOT_VOICEMEETER}><VoiceMeeterParameters>\
             <Strip index='8' busa='1' dblevel='0.0' /></VoiceMeeterParameters>\
             </{ROOT_VOICEMEETER}>"
        );
        assert_eq!(parse(&sparse).expect("parses").edition, Edition::Potato);

        let small = format!(
            "<{ROOT_VOICEMEETER}><VoiceMeeterParameters>\
             <Strip index='1' busa='1' dblevel='0.0' />\
             <Bus index='1' BusMode='0' dblevel='0.0' /></VoiceMeeterParameters>\
             </{ROOT_VOICEMEETER}>"
        );
        assert_eq!(parse(&small).expect("parses").edition, Edition::Standard);
    }

    #[test]
    fn every_edition_knows_its_own_size() {
        assert_eq!(Edition::Standard.size(), (3, 2));
        assert_eq!(Edition::Banana.size(), (5, 5));
        assert_eq!(Edition::Potato.size(), (8, 8));
    }

    #[test]
    fn unrelated_xml_is_rejected_by_its_root() {
        let err = parse("<html><body>not a mixer</body></html>").unwrap_err();
        assert!(matches!(err, Error::UnknownDocument(root) if root == "html"));
    }

    #[test]
    fn a_recognised_document_of_the_wrong_kind_says_so() {
        let vban = "<VBAudioVoicemeeterVBANConfig><VBANConfiguration/>\
                    </VBAudioVoicemeeterVBANConfig>";
        let err = parse(vban).unwrap_err();
        assert!(matches!(err, Error::WrongDocument(_)));
        assert!(err.to_string().contains("VBAN"));
    }

    #[test]
    fn a_pipemeter_file_is_read_as_the_same_format() {
        let ours = format!(
            "<{ROOT_PIPEMETER}><PipemeterParameters>\
             <Strip index='1' busa='1' dblevel='1.0' /></PipemeterParameters></{ROOT_PIPEMETER}>"
        );
        let parsed = parse(&ours).expect("our own dialect is the same format");
        assert_eq!(parsed.dialect, Dialect::PipeMeter);
        assert!(parsed.strips[0].buses[0]);

        let theirs = ours.replace(ROOT_PIPEMETER, ROOT_VOICEMEETER);
        assert_eq!(
            parse(&theirs).expect("and so is theirs").dialect,
            Dialect::Voicemeeter
        );
    }

    /// Every reference file we have, of every kind. The point is coverage
    /// of the *shapes* a real installation produces: three editions, scenes,
    /// presets, VBAN, MIDI and `MacroButtons`, plus the odd hand-edited
    /// fragment. Any that fails to parse is a gap in the reader.
    #[test]
    fn every_reference_document_is_recognised() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.references/configs");
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };

        let mut seen = 0;
        let mut walk = vec![std::path::PathBuf::from(root)];
        while let Some(dir) = walk.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|n| n == "docs") {
                        continue;
                    }
                    walk.push(path);
                } else if path.extension().is_some_and(|e| e == "xml") {
                    let Ok(text) = std::fs::read_to_string(&path) else {
                        continue;
                    };
                    let parsed = super::Document::parse(&text);
                    let Ok(document) = parsed else {
                        panic!("{} did not parse: {:?}", path.display(), parsed.err());
                    };
                    seen += 1;
                    drop(document);
                }
            }
        }
        drop(entries);
        assert!(seen > 10, "expected a pile of reference files, saw {seen}");
    }

    /// A real 300 KB settings file, dumped from Voicemeeter Potato on
    /// Windows. Parsing it is the only way to know the reader survives the
    /// shape of a genuine file rather than of the fixtures above.
    #[test]
    fn a_real_settings_file_reads_end_to_end() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.references/configs/windows/voicemeeter.xml"
        );
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let parsed = parse(&text).expect("a real file should parse");

        assert_eq!(parsed.strips.len(), 8);
        assert_eq!(parsed.buses.len(), 8);
        assert!(parsed.strip_eq.len() > 100, "strip EQ cells were dropped");
        assert!(parsed.bus_eq.len() > 100, "bus EQ cells were dropped");
        assert_eq!(parsed.c5.band.len(), 5);
        assert!(!parsed.reverb.preset_name.is_empty());
        assert!(parsed.device_options.mme > 0);
        assert_eq!(parsed.memories.len(), 38, "A/B memories were not captured");

        println!(
            "strip EQ cells {}, bus EQ cells {}, C5 bands {}, reverb preset {:?}",
            parsed.strip_eq.len(),
            parsed.bus_eq.len(),
            parsed.c5.band.len(),
            parsed.reverb.preset_name,
        );
        assert!(parsed.compressors.iter().any(|c| c.ratio > 1.0));
        assert!(parsed.gates.iter().any(|g| g.hold > 0.0));
        assert!(
            parsed
                .strips
                .iter()
                .any(|s| s.layers.iter().any(|l| *l != 0.0))
        );
    }

    #[test]
    fn the_duplicated_parameters_tag_is_repaired() {
        let doubled = SAMPLE.replace(
            "<VoiceMeeterParameters>",
            "<VoiceMeeterParameters>\n<VoiceMeeterParameters>",
        );
        let out = parse(&doubled).expect("repaired and parsed");
        assert_eq!(out.strips[0].label, "Mic IN");
    }

    #[test]
    fn genuine_nesting_is_left_alone() {
        let nested = format!(
            "<{ROOT_VOICEMEETER}>\n<b>\n<Strip index='1' busa='1' dblevel='1.0' />\n</b>\n\
             </{ROOT_VOICEMEETER}>"
        );
        let out = parse(&nested).expect("parses");
        assert!(out.strips[0].buses[0]);
    }

    #[test]
    fn an_unescaped_ampersand_in_a_device_name_is_repaired() {
        let xml = format!(
            "<{ROOT_VOICEMEETER}><VoiceMeeterDeviceConfiguration>\
             <InputDev index='1' name=\"Digital Audio Interface (Y&H Game Live)\" />\
             </VoiceMeeterDeviceConfiguration><VoiceMeeterParameters>\
             <Strip index='1' busa='1' dblevel='0.0' /></VoiceMeeterParameters>\
             </{ROOT_VOICEMEETER}>"
        );
        let parsed = parse(&xml).expect("a real file with a real card name");
        assert_eq!(
            parsed.input_devices[0].as_deref(),
            Some("Digital Audio Interface (Y&H Game Live)")
        );
    }

    #[test]
    fn entities_that_are_already_correct_are_left_alone() {
        let xml = format!(
            "<{ROOT_VOICEMEETER}><VoiceMeeterDeviceConfiguration>\
             <InputDev index='1' name=\"a &amp; b &#65; c\" />\
             </VoiceMeeterDeviceConfiguration><VoiceMeeterParameters>\
             <Strip index='1' busa='1' dblevel='0.0' /></VoiceMeeterParameters>\
             </{ROOT_VOICEMEETER}>"
        );
        assert_eq!(
            parse(&xml).expect("parses").input_devices[0].as_deref(),
            Some("a & b A c")
        );
    }

    #[test]
    fn malformed_xml_is_rejected() {
        assert!(matches!(parse("<unclosed"), Err(Error::Xml(_))));
    }
}
