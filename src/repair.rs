//! Making a Voicemeeter settings file well-formed enough to parse.
//!
//! Two malformations show up in real files and neither is our doing: the
//! `<VoiceMeeterParameters>` element is opened twice and closed once, and
//! device names carry bare ampersands. Both are repaired on the way in
//! rather than reaching for a lenient parser, which would give up every
//! other check at the same time.

/// Repair the one malformation Voicemeeter actually writes.
///
/// Real settings files open `<VoiceMeeterParameters>` twice in a row and
/// close it once, which is not well-formed XML and which every strict parser
/// rejects. Rather than reach for a lenient parser and lose all the other
/// checks, the duplicate opening tag is dropped first.
///
/// Only *consecutive* duplicates are removed, so a genuinely nested document
/// is left alone.
pub(crate) fn repair(xml: &str) -> std::borrow::Cow<'_, str> {
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
pub(crate) fn escape_bare_ampersands(xml: &str) -> String {
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
