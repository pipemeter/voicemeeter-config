//! The per-strip processing blocks, and the shared effects.
//!
//! These are the elements the reader used to walk straight past. None of
//! them drive anything in the mixer yet, but a settings file that comes in
//! and goes back out should not lose them on the way through, and the
//! windows that will edit them need somewhere to read from.
//!
//! Every field keeps the file's own units — milliseconds, decibels, a ratio,
//! a normalised 0..1 — rather than being converted on the way in. Converting
//! here would mean converting back on the way out, and two conversions is
//! two chances to be wrong about a format nobody documents.

use crate::read::{flag, flag_f32};

/// The compressor behind a strip's Comp. knob.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Compressor {
    pub gain_in: f32,
    /// Milliseconds.
    pub attack: f32,
    pub release: f32,
    pub knee: f32,
    pub ratio: f32,
    /// Decibels.
    pub threshold: f32,
    pub auto_makeup: bool,
    pub gain_out: f32,
}

impl Default for Compressor {
    fn default() -> Self {
        Self {
            gain_in: 0.0,
            attack: 10.0,
            release: 50.0,
            knee: 0.5,
            ratio: 1.0,
            threshold: 0.0,
            auto_makeup: true,
            gain_out: 0.0,
        }
    }
}

impl Compressor {
    pub(crate) fn read(node: &roxmltree::Node<'_, '_>) -> Self {
        Self {
            gain_in: flag_f32(node, "gainin"),
            attack: flag_f32(node, "attack"),
            release: flag_f32(node, "release"),
            knee: flag_f32(node, "knee"),
            ratio: flag_f32(node, "comprate"),
            threshold: flag_f32(node, "threshold"),
            auto_makeup: flag(node, "automakeup"),
            gain_out: flag_f32(node, "gainout"),
        }
    }
}

/// The gate behind a strip's Gate knob.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Gate {
    /// Decibels.
    pub threshold: f32,
    pub damping: f32,
    /// Sidechain bandpass centre, in Hz.
    pub sidechain: f32,
    /// Milliseconds.
    pub attack: f32,
    pub hold: f32,
    pub release: f32,
}

impl Gate {
    pub(crate) fn read(node: &roxmltree::Node<'_, '_>) -> Self {
        Self {
            threshold: flag_f32(node, "thresin"),
            damping: flag_f32(node, "damping"),
            sidechain: flag_f32(node, "bpsidechain"),
            attack: flag_f32(node, "attack"),
            hold: flag_f32(node, "hold"),
            release: flag_f32(node, "release"),
        }
    }
}

/// Pitch and formant shifting — the other half of the Denoiser knob.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pitch {
    pub on: bool,
    /// Percent.
    pub dry_wet: f32,
    pub value: f32,
    /// Low, medium and high formant shift.
    pub formant: [f32; 3],
}

impl Pitch {
    pub(crate) fn read(node: &roxmltree::Node<'_, '_>) -> Self {
        Self {
            on: flag(node, "pitchon"),
            dry_wet: flag_f32(node, "drywet"),
            value: flag_f32(node, "pitchvalue"),
            formant: [
                flag_f32(node, "formantlo"),
                flag_f32(node, "formantmed"),
                flag_f32(node, "formanthigh"),
            ],
        }
    }
}

/// One cell of a parametric equaliser.
///
/// A file carries these by the hundred — one per channel per cell, for every
/// strip and every bus — which is why they are stored flat and addressed by
/// their own coordinates rather than nested.
///
/// The live cells are `<Strip>` and `<Bus>` elements carrying `cell`, inside
/// `<VoiceMeeterStripEQ>` and `<VoiceMeeterBUSEQ>`. The `StripEQmem` and
/// `BusEQmem` elements that look like the obvious candidates are the A/B
/// compare memories, and on a real file most of them are empty.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EqCell {
    /// Which strip or bus this cell belongs to. One-based, as the file
    /// writes them, and so are the two below.
    pub owner: u32,
    pub channel: u32,
    pub cell: u32,
    pub on: bool,
    /// Filter shape, as the file numbers them.
    pub kind: u32,
    pub gain_db: f32,
    pub freq: f32,
    pub q: f32,
}

impl EqCell {
    pub(crate) fn read(node: &roxmltree::Node<'_, '_>) -> Self {
        Self {
            owner: crate::read::mode_index(flag_f32(node, "index")),
            channel: crate::read::mode_index(flag_f32(node, "channel")),
            cell: crate::read::mode_index(flag_f32(node, "cell")),
            on: flag(node, "EQon"),
            kind: crate::read::mode_index(flag_f32(node, "EQtype")),
            gain_db: flag_f32(node, "dblevel"),
            freq: flag_f32(node, "freq"),
            q: flag_f32(node, "Q"),
        }
    }
}

/// The shared reverb.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reverb {
    pub muted: bool,
    pub preset: u32,
    /// Shown on the SPECIAL FX panel beside the effect's name.
    pub preset_name: String,
    pub dry: f32,
    pub wet: f32,
    pub predelay: f32,
    pub decay: f32,
    pub early_reflections: f32,
    pub eq_muted: bool,
}

/// The shared multi-tap delay.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Delay {
    pub muted: bool,
    pub balance: (f32, f32),
    pub dry: f32,
    pub wet: f32,
    pub autofit: bool,
    pub phase: bool,
    pub predelay: f32,
    pub time_ratio: f32,
    pub bpm: f32,
    pub feedback: f32,
    pub feedback_kind: u32,
}

/// The five-band compressor the panel abbreviates to `C5`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct C5 {
    pub muted: bool,
    pub auto_makeup: bool,
    pub bands: u32,
    pub master_gain: f32,
    /// Crossover ranks between the bands.
    pub crossovers: [f32; 4],
    pub band: Vec<C5Band>,
}

/// One band of the five-band compressor.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct C5Band {
    pub index: u32,
    /// Mute, solo, chain, compressor-on and limiter-on, in that order. An
    /// array rather than five named flags, the same shape the strips and
    /// buses use for theirs.
    pub flags: [bool; 5],
    pub gain: f32,
    pub attack: f32,
    pub release: f32,
    pub knee: f32,
    pub ratio: f32,
    pub threshold: f32,
    pub gain_out: f32,
    pub limit: f32,
    pub distortion: f32,
}

/// Indices into [`C5Band::flags`].
pub const BAND_MUTE: usize = 0;
pub const BAND_SOLO: usize = 1;
pub const BAND_CHAIN: usize = 2;
pub const BAND_COMP: usize = 3;
pub const BAND_LIMITER: usize = 4;

impl Reverb {
    pub(crate) fn read_general(&mut self, node: &roxmltree::Node<'_, '_>) {
        self.muted = flag(node, "mute");
        self.preset = crate::read::mode_index(flag_f32(node, "nuPreset"));
        node.attribute("presetName")
            .unwrap_or_default()
            .clone_into(&mut self.preset_name);
        self.dry = flag_f32(node, "dry");
        self.wet = flag_f32(node, "wet");
    }

    pub(crate) fn read_param(&mut self, node: &roxmltree::Node<'_, '_>) {
        self.predelay = flag_f32(node, "predelay");
        self.decay = flag_f32(node, "decay");
        self.early_reflections = flag_f32(node, "EarlyRefAmount");
        self.eq_muted = flag(node, "EQmute");
    }
}

impl Delay {
    pub(crate) fn read_general(&mut self, node: &roxmltree::Node<'_, '_>) {
        self.muted = flag(node, "mute");
        self.balance = (
            flag_f32(node, "balance_left"),
            flag_f32(node, "balance_right"),
        );
        self.dry = flag_f32(node, "dry");
        self.wet = flag_f32(node, "wet");
        self.autofit = flag(node, "autofit");
        self.phase = flag(node, "phase");
    }

    pub(crate) fn read_delay(&mut self, node: &roxmltree::Node<'_, '_>) {
        self.predelay = flag_f32(node, "predelay");
        self.time_ratio = flag_f32(node, "timeratio");
        self.bpm = flag_f32(node, "currentBPM");
        self.feedback = flag_f32(node, "feedback");
        self.feedback_kind = crate::read::mode_index(flag_f32(node, "feedbacktype"));
    }
}

impl C5 {
    pub(crate) fn read_general(&mut self, node: &roxmltree::Node<'_, '_>) {
        self.muted = flag(node, "mute");
        self.auto_makeup = flag(node, "automakeup");
        self.bands = crate::read::mode_index(flag_f32(node, "nbBand"));
        self.master_gain = flag_f32(node, "MasterGain");
        for (i, attr) in ["freqRnk1", "freqRnk2", "freqRnk3", "freqRnk4"]
            .into_iter()
            .enumerate()
        {
            self.crossovers[i] = flag_f32(node, attr);
        }
    }
}

impl C5Band {
    pub(crate) fn read(node: &roxmltree::Node<'_, '_>) -> Self {
        Self {
            index: crate::read::mode_index(flag_f32(node, "index")),
            flags: [
                flag(node, "mute"),
                flag(node, "solo"),
                flag(node, "chain"),
                flag(node, "compOn"),
                flag(node, "limOn"),
            ],
            gain: flag_f32(node, "gain"),
            attack: flag_f32(node, "attack"),
            release: flag_f32(node, "release"),
            knee: flag_f32(node, "knee"),
            ratio: flag_f32(node, "ratio"),
            threshold: flag_f32(node, "threshold"),
            gain_out: flag_f32(node, "gainOut"),
            limit: flag_f32(node, "limit"),
            distortion: flag_f32(node, "distorsion"),
        }
    }
}

/// Which channels the two external FX sends and returns are patched to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExternalPatch {
    /// Send 1 and send 2, each a pair of channels.
    pub send: [[u32; 2]; 2],
    pub ret: [[u32; 2]; 2],
}

impl ExternalPatch {
    pub(crate) fn read_send(&mut self, node: &roxmltree::Node<'_, '_>) {
        self.send = [
            [num(node, "send1ch1"), num(node, "send1ch2")],
            [num(node, "send2ch1"), num(node, "send2ch2")],
        ];
    }

    pub(crate) fn read_return(&mut self, node: &roxmltree::Node<'_, '_>) {
        self.ret = [
            [num(node, "ret1ch1"), num(node, "ret1ch2")],
            [num(node, "ret2ch1"), num(node, "ret2ch2")],
        ];
    }
}

/// Buffer sizes per driver, and the monitoring delays.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DeviceOptions {
    pub mme: u32,
    pub wdm: u32,
    pub ks: u32,
    pub asio: u32,
    pub asio_rate: u32,
    /// Monitoring synchro delay per hardware out, in milliseconds.
    pub monitor_delay: [f32; 5],
}

impl DeviceOptions {
    pub(crate) fn read(node: &roxmltree::Node<'_, '_>) -> Self {
        let mut options = Self {
            mme: num(node, "mme"),
            wdm: num(node, "wdm"),
            ks: num(node, "ks"),
            asio: num(node, "asio"),
            asio_rate: num(node, "srasio"),
            monitor_delay: [0.0; 5],
        };
        for (i, attr) in ["msA1", "msA2", "msA3", "msA4", "msA5"]
            .into_iter()
            .enumerate()
        {
            options.monitor_delay[i] = flag_f32(node, attr);
        }
        options
    }
}

fn num(node: &roxmltree::Node<'_, '_>, attr: &str) -> u32 {
    crate::read::mode_index(flag_f32(node, attr))
}

#[cfg(test)]
mod tests {
    use super::{Compressor, Gate};

    fn node<'a>(doc: &'a roxmltree::Document<'a>) -> roxmltree::Node<'a, 'a> {
        doc.root_element()
    }

    #[test]
    fn the_compressor_reads_the_names_the_file_uses() {
        let xml = "<StripComp index='1' gainin='1.5' attack='10.0' release='50.0' \
                   knee='0.50' comprate='2.740' threshold='-40.00' automakeup='1' \
                   gainout='3.0' />";
        let doc = roxmltree::Document::parse(xml).unwrap();
        let comp = Compressor::read(&node(&doc));
        assert!((comp.ratio - 2.74).abs() < 1e-4);
        assert!((comp.threshold + 40.0).abs() < 1e-4);
        assert!(comp.auto_makeup);
        assert!((comp.gain_out - 3.0).abs() < 1e-4);
    }

    #[test]
    fn a_missing_attribute_leaves_its_default() {
        let doc = roxmltree::Document::parse("<StripGate index='1' hold='177.0' />").unwrap();
        let gate = Gate::read(&node(&doc));
        assert!((gate.hold - 177.0).abs() < 1e-4);
        assert!(gate.threshold.abs() < f32::EPSILON);
    }
}
