//! Reading attributes off an element, forgivingly.
//!
//! Everything here treats a missing or malformed attribute as its default
//! rather than as an error. Settings files come from other versions and
//! other editions of the program, and half a mixer restored is worth more
//! than a refusal.

pub fn flag(node: &roxmltree::Node<'_, '_>, attr: &str) -> bool {
    node.attribute(attr).is_some_and(|v| v.trim() != "0")
}

pub fn flag_f32(node: &roxmltree::Node<'_, '_>, attr: &str) -> f32 {
    node.attribute(attr)
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0.0)
}

/// Turn the file's `BusMode` number into an index, tolerating rubbish.
///
/// Saturating rather than wrapping: a corrupt file should land on the first
/// mode, not on whatever a wrapped cast happens to select.
pub fn mode_index(raw: f32) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let clamped = raw.round().clamp(0.0, f32::from(u16::MAX)) as u16;
    if raw.is_finite() {
        u32::from(clamped)
    } else {
        0
    }
}

/// Voicemeeter indexes from one; we index from zero.
pub fn index_of(node: &roxmltree::Node<'_, '_>) -> Option<usize> {
    node.attribute("index")?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)
}

/// `suffix("LabelStrip3", "LabelStrip")` is `Some(3)`. Used to turn a
/// position-named tag into an index.
pub fn suffix(tag: &str, prefix: &str) -> Option<usize> {
    tag.strip_prefix(prefix)?.parse().ok()
}
