//! VBAN network configuration.
//!
//! Its own document rather than part of the mixer settings, because the
//! network setup outlives any one mix.
//!
//! The element names in this file keep the original's spelling, which is the
//! only place it survives anywhere in the program. They are not ours to
//! choose: they are what is written in the file, and a document with
//! different tags is one Voicemeeter cannot read and we cannot read back.
//!
//! Eight incoming streams and eight outgoing, each naming a host, a port and
//! a quality. Nothing here is interpreted: an incoming stream's `in` is the
//! strip it lands on as the file numbers it, not as this program numbers
//! anything, and translating on the way in would mean translating back.

use crate::read::{flag, flag_f32, index_of, mode_index};

/// A whole VBAN configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub username: String,
    /// The colour the original tints the VBAN button with, as written.
    pub colour: String,
    pub incoming: Vec<Stream>,
    pub outgoing: Vec<Stream>,
}

/// One stream, in either direction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stream {
    /// One-based slot, as the file writes it.
    pub index: u32,
    pub enabled: bool,
    pub name: String,
    /// Empty on an incoming stream that accepts any address.
    pub address: String,
    pub port: u16,
    /// Which strip or bus the stream is attached to, as the file numbers it.
    pub channel: u32,
    /// Network quality, 0 (optimal) to 4 (very slow).
    pub quality: u32,
}

impl Config {
    pub(crate) fn read(root: &roxmltree::Node<'_, '_>) -> Self {
        let mut config = Self::default();
        for node in root.descendants() {
            match node.tag_name().name() {
                "VBAN" => {
                    config.enabled = flag(&node, "status");
                    config.username = attr(&node, "username");
                    config.colour = attr(&node, "color");
                }
                "VBANStreamIn" => config.incoming.push(Stream::read(&node, "in")),
                "VBANStreamOut" => config.outgoing.push(Stream::read(&node, "out")),
                _ => {}
            }
        }
        config
    }
}

impl Stream {
    fn read(node: &roxmltree::Node<'_, '_>, channel_attr: &str) -> Self {
        Self {
            index: u32::try_from(index_of(node).unwrap_or(0) + 1).unwrap_or(1),
            enabled: flag(node, "status"),
            name: attr(node, "name"),
            address: attr(node, "ip"),
            // Ports above 65535 cannot be real, so a bad one reads as the
            // VBAN default rather than as a wrapped number.
            port: u16::try_from(mode_index(flag_f32(node, "port"))).unwrap_or(6980),
            channel: mode_index(flag_f32(node, channel_attr)),
            quality: mode_index(flag_f32(node, "NQ")),
        }
    }
}

fn attr(node: &roxmltree::Node<'_, '_>, name: &str) -> String {
    node.attribute(name).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use crate::Document;

    const SAMPLE: &str = r"<?xml version='1.0' encoding='utf-8'?>
<VBAudioVoicemeeterVBANConfig>
<VBANConfiguration>
    <VBAN status='1' username='blu' color='00FBDB26' />
    <VBANStreamIn index='1' in='5' status='0' name='blu-pc' ip='192.168.2.51' port='6980' NQ='0' />
    <VBANStreamIn index='2' in='0' status='1' name='Stream2' ip='' port='6980' NQ='1' />
    <VBANStreamOut index='1' out='0' status='1' name='BusA1' ip='192.168.2.50' port='6980' NQ='2' />
</VBANConfiguration>
</VBAudioVoicemeeterVBANConfig>";

    #[test]
    fn a_vban_document_is_recognised_and_read() {
        let Document::Vban(config) = Document::parse(SAMPLE).expect("parses") else {
            panic!("should have been recognised as a VBAN configuration");
        };
        assert!(config.enabled);
        assert_eq!(config.username, "blu");
        assert_eq!(config.incoming.len(), 2);
        assert_eq!(config.outgoing.len(), 1);

        let first = &config.incoming[0];
        assert_eq!(first.name, "blu-pc");
        assert_eq!(first.address, "192.168.2.51");
        assert_eq!(first.port, 6980);
        // status='0' means the stream is configured but not running.
        assert!(!first.enabled);
        assert!(config.incoming[1].enabled);
    }

    #[test]
    fn an_outgoing_stream_takes_its_channel_from_the_out_attribute() {
        let Document::Vban(config) = Document::parse(SAMPLE).expect("parses") else {
            panic!("wrong document kind");
        };
        assert_eq!(config.incoming[0].channel, 5);
        assert_eq!(config.outgoing[0].channel, 0);
    }
}

#[cfg(test)]
mod round_trip {
    use crate::Document;

    fn sample() -> super::Config {
        super::Config {
            enabled: true,
            username: "Blu-PC-Win11".to_owned(),
            colour: "0x40C0FF".to_owned(),
            incoming: vec![super::Stream {
                index: 1,
                enabled: true,
                name: "blu-pc".to_owned(),
                address: "192.168.2.51".to_owned(),
                port: 6980,
                channel: 5,
                quality: 0,
            }],
            outgoing: vec![super::Stream {
                index: 1,
                enabled: false,
                name: "Stream1".to_owned(),
                address: String::new(),
                port: 6981,
                channel: 0,
                quality: 3,
            }],
        }
    }

    #[test]
    fn a_written_configuration_reads_back_the_same() {
        // The window edits this and nothing else writes it, so a lossy
        // writer would quietly drop the user's network setup on save.
        let before = sample();
        let xml = Document::Vban(before.clone()).render();
        let Document::Vban(after) = Document::parse(&xml).expect("parses") else {
            panic!("a VBAN document did not read back as one");
        };
        assert_eq!(after, before);
    }

    #[test]
    fn the_two_directions_keep_their_own_channel_attribute() {
        // `in` and `out` are the only thing that differs between the tags,
        // and swapping them would silently move every stream.
        let xml = Document::Vban(sample()).render();
        assert!(xml.contains("<VBANStreamIn"), "{xml}");
        assert!(xml.contains("<VBANStreamOut"), "{xml}");
        assert!(xml.contains("in=\"5\""), "{xml}");
        assert!(xml.contains("out=\"0\""), "{xml}");
    }

    #[test]
    fn an_empty_configuration_still_makes_a_valid_document() {
        let xml = Document::Vban(super::Config::default()).render();
        let Document::Vban(after) = Document::parse(&xml).expect("parses") else {
            panic!("not a VBAN document");
        };
        assert_eq!(after, super::Config::default());
    }
}
