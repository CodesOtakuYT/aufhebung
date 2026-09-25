//! A small, zero-copy pull parser for UTF-8 XML on [`aufhebung_core`]'s
//! chunked cursors.
//!
//! [`Parser::next_event`] consumes a list of byte chunks and returns borrowed
//! [`Pieces`] values for names, text, comments, CDATA, and attribute values.
//! A contiguous input is just a one-element chunk list. Values remain borrowed
//! even when a tag, attribute, or text run crosses a chunk boundary.
//!
//! The parser performs lightweight structural checks: element names are
//! matched, one root is required, attributes are checked for duplicates, and
//! malformed or truncated markup is reported separately. It intentionally does
//! not implement a complete XML processor. Namespace prefixes are kept
//! literally, DTD contents are opaque, and entity references are never
//! expanded or interpreted: `&amp;`, for example, remains the five input bytes
//! `&amp;` in the returned [`Pieces`].
//!
//! # Example
//!
//! ```
//! use aufhebung_xml::{Attribute, Event, Parser};
//!
//! const WIRE: &[&[u8]] = &[
//!     b"<?xml version=\"1.0\"?>\n<atlas>",
//!     b"<SubTexture name=\"hero\" x=\"0\"",
//!     b" y=\"1\" width=\"32\" height=\"64\"/>",
//!     b"</atlas>",
//! ];
//!
//! let mut parser = Parser::new(WIRE);
//! let mut subtextures = 0;
//! while let Some(event) = parser.next_event()? {
//!     if let Event::EmptyElement { name, attributes } = event {
//!         if name == b"SubTexture" {
//!             assert!(attributes[0].name() == b"name");
//!             assert!(attributes[0].value() == b"hero");
//!             subtextures += 1;
//!         }
//!     }
//! }
//! assert_eq!(subtextures, 1);
//! # Ok::<(), aufhebung_xml::Error>(())
//! ```
//!
//! The input is supplied as a complete chunk list. If the list ends in the
//! middle of a token, [`Error::Incomplete`] is returned without advancing the
//! parser; a caller with more bytes can construct a new parser over the
//! expanded list.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use aufhebung_core::ChunkedCursor;

/// The zero-copy span type borrowed by XML event names, text, and values.
pub use aufhebung_core::Pieces;

const XML_WHITESPACE: &[u8] = b" \t\r\n";
const NAME_DELIMITERS: &[u8] = b" \t\r\n/>=<!?\"'";

/// Errors returned while consuming XML events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The supplied chunk list ended before the current event was complete.
    ///
    /// More input may complete the event. The parser leaves its cursor at the
    /// beginning of that event when it returns this error.
    Incomplete,
    /// The bytes violate the parser's XML structure or local markup grammar.
    Malformed,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Incomplete => f.write_str("input ended mid-XML event"),
            Error::Malformed => f.write_str("malformed XML"),
        }
    }
}

impl core::error::Error for Error {}

/// One XML attribute, borrowing its name and value from the input chunks.
///
/// The value excludes its surrounding quote characters. Entity references are
/// left exactly as they appeared in the source.
#[derive(Clone, PartialEq, Eq)]
pub struct Attribute<'a> {
    name: Pieces<'a, u8>,
    value: Pieces<'a, u8>,
}

impl<'a> Attribute<'a> {
    /// The attribute name, including any namespace prefix.
    pub fn name(&self) -> Pieces<'a, u8> {
        self.name
    }

    /// The raw attribute value, excluding its surrounding quotes.
    pub fn value(&self) -> Pieces<'a, u8> {
        self.value
    }
}

/// An event emitted by [`Parser::next_event`].
///
/// All byte-valued fields are borrowed from the chunk list supplied to
/// [`Parser::new`]. The parser does not decode entity references.
#[derive(Clone, PartialEq, Eq)]
pub enum Event<'a> {
    /// An XML declaration such as `<?xml version="1.0"?>`.
    XmlDeclaration {
        /// The declaration's attributes, in source order.
        attributes: Vec<Attribute<'a>>,
    },
    /// A processing instruction other than the XML declaration.
    ProcessingInstruction {
        /// The instruction target.
        target: Pieces<'a, u8>,
        /// The instruction data after the target and its separating whitespace.
        data: Pieces<'a, u8>,
    },
    /// A non-empty start tag.
    StartElement {
        /// The element name, including any namespace prefix.
        name: Pieces<'a, u8>,
        /// The element's attributes, in source order.
        attributes: Vec<Attribute<'a>>,
    },
    /// An end tag. Its name must match the current open element.
    EndElement {
        /// The element name, including any namespace prefix.
        name: Pieces<'a, u8>,
    },
    /// A self-closing element. No corresponding [`Event::EndElement`] follows.
    EmptyElement {
        /// The element name, including any namespace prefix.
        name: Pieces<'a, u8>,
        /// The element's attributes, in source order.
        attributes: Vec<Attribute<'a>>,
    },
    /// A character-data run. Whitespace is emitted too, and `&` is ordinary
    /// input rather than the start of an expanded entity.
    Text(Pieces<'a, u8>),
    /// A CDATA section's contents, without the `<![CDATA[` and `]]>` markers.
    CData(Pieces<'a, u8>),
    /// A comment's contents, without the `<!--` and `-->` markers.
    Comment(Pieces<'a, u8>),
    /// A doctype declaration's contents, without the `<!DOCTYPE` and `>` markers.
    ///
    /// The declaration is not interpreted and entity declarations inside it
    /// are not expanded.
    Doctype(Pieces<'a, u8>),
}

/// A stateful pull parser over a list of XML byte chunks.
///
/// The parser checks basic document structure while producing events. It does
/// not build a DOM, decode entities, resolve namespaces, or process DTDs.
/// `Pieces` returned by events may contain bytes from several input chunks.
pub struct Parser<'a> {
    cursor: ChunkedCursor<'a, u8>,
    open_elements: Vec<Pieces<'a, u8>>,
    root_seen: bool,
    root_closed: bool,
    declaration_seen: bool,
    doctype_seen: bool,
    started: bool,
    finished: bool,
}

impl<'a> Parser<'a> {
    /// Create a parser over `chunks`.
    ///
    /// A leading UTF-8 BOM is ignored. A contiguous input is represented as
    /// `[input]`. The parser does not copy the chunks.
    pub fn new(chunks: &'a [&'a [u8]]) -> Self {
        let mut cursor = ChunkedCursor::new(chunks);
        let mut probe = cursor;
        if probe.next_byte() == Some(0xef)
            && probe.next_byte() == Some(0xbb)
            && probe.next_byte() == Some(0xbf)
        {
            cursor = probe;
        }

        Self {
            cursor,
            open_elements: Vec::new(),
            root_seen: false,
            root_closed: false,
            declaration_seen: false,
            doctype_seen: false,
            started: false,
            finished: false,
        }
    }

    /// Read the next event.
    ///
    /// Returns `Ok(None)` only after a complete document has been consumed.
    /// A token that reaches the end of the current chunk list returns
    /// [`Error::Incomplete`]. Event parsing is atomic: an incomplete or
    /// malformed event does not advance the parser's cursor.
    pub fn next_event(&mut self) -> Result<Option<Event<'a>>, Error> {
        if self.finished {
            return Ok(None);
        }

        let Some(first) = self.cursor.peek_byte() else {
            if self.root_closed && self.open_elements.is_empty() {
                self.finished = true;
                return Ok(None);
            }
            return Err(Error::Incomplete);
        };

        let mut cursor = self.cursor;
        let event = if first == b'<' {
            self.parse_markup(&mut cursor)?
        } else {
            self.parse_text(&mut cursor)?
        };

        self.validate_event(&event)?;
        self.cursor = cursor;
        self.record_event(&event);
        Ok(Some(event))
    }

    /// Whether the parser has reached a complete document end.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Consume all remaining events, returning an error if the document is
    /// incomplete or malformed.
    pub fn finish(&mut self) -> Result<(), Error> {
        while self.next_event()?.is_some() {}
        Ok(())
    }

    fn parse_text(&self, cursor: &mut ChunkedCursor<'a, u8>) -> Result<Event<'a>, Error> {
        let text = cursor.take_until_byte(b'<');
        if cursor.is_empty() {
            let whitespace = is_xml_whitespace(text);
            if self.open_elements.is_empty() && !whitespace {
                return Err(Error::Malformed);
            }
            if !self.root_closed {
                return Err(Error::Incomplete);
            }
        }
        Ok(Event::Text(text))
    }

    fn parse_markup(&self, cursor: &mut ChunkedCursor<'a, u8>) -> Result<Event<'a>, Error> {
        if !cursor.skip_byte(b'<') {
            return Err(if cursor.is_empty() {
                Error::Incomplete
            } else {
                Error::Malformed
            });
        }

        match cursor.peek_byte() {
            None => Err(Error::Incomplete),
            Some(b'?') => {
                cursor.next_byte();
                if is_xml_declaration_start(cursor) {
                    parse_declaration(cursor)
                } else {
                    parse_processing_instruction(cursor)
                }
            }
            Some(b'!') => {
                cursor.next_byte();
                parse_declaration_block(cursor)
            }
            Some(b'/') => {
                cursor.next_byte();
                let name = take_name(cursor)?;
                cursor.skip_while_any(XML_WHITESPACE);
                match cursor.peek_byte() {
                    Some(b'>') => {
                        cursor.next_byte();
                        Ok(Event::EndElement { name })
                    }
                    None => Err(Error::Incomplete),
                    Some(_) => Err(Error::Malformed),
                }
            }
            Some(_) => parse_start_element(cursor),
        }
    }

    fn validate_event(&self, event: &Event<'a>) -> Result<(), Error> {
        match event {
            Event::XmlDeclaration { .. } => {
                if self.started || self.declaration_seen || self.root_seen {
                    Err(Error::Malformed)
                } else {
                    Ok(())
                }
            }
            Event::Doctype(_) => {
                if self.root_seen || self.doctype_seen {
                    Err(Error::Malformed)
                } else {
                    Ok(())
                }
            }
            Event::StartElement { .. } | Event::EmptyElement { .. } => {
                if self.root_closed {
                    Err(Error::Malformed)
                } else {
                    Ok(())
                }
            }
            Event::EndElement { name } => {
                if self.open_elements.last().is_some_and(|open| open == name) {
                    Ok(())
                } else {
                    Err(Error::Malformed)
                }
            }
            Event::Text(text) => {
                if self.open_elements.is_empty()
                    && text.byte_len() != 0
                    && !is_xml_whitespace(*text)
                {
                    Err(Error::Malformed)
                } else {
                    Ok(())
                }
            }
            Event::CData(_) if self.open_elements.is_empty() => Err(Error::Malformed),
            Event::CData(_) | Event::Comment(_) | Event::ProcessingInstruction { .. } => Ok(()),
        }
    }

    fn record_event(&mut self, event: &Event<'a>) {
        self.started = true;
        match event {
            Event::XmlDeclaration { .. } => self.declaration_seen = true,
            Event::Doctype(_) => self.doctype_seen = true,
            Event::StartElement { name, .. } => {
                self.root_seen = true;
                self.open_elements.push(*name);
            }
            Event::EndElement { .. } => {
                self.open_elements.pop();
                if self.open_elements.is_empty() {
                    self.root_closed = true;
                }
            }
            Event::EmptyElement { .. } => {
                self.root_seen = true;
                if self.open_elements.is_empty() {
                    self.root_closed = true;
                }
            }
            Event::Text(_)
            | Event::CData(_)
            | Event::Comment(_)
            | Event::ProcessingInstruction { .. } => {}
        }
    }
}

impl<'a> Iterator for Parser<'a> {
    type Item = Result<Event<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_event().transpose()
    }
}

fn parse_start_element<'a>(cursor: &mut ChunkedCursor<'a, u8>) -> Result<Event<'a>, Error> {
    let name = take_name(cursor)?;
    let mut attributes = Vec::new();

    loop {
        let whitespace = cursor.skip_while_any(XML_WHITESPACE);
        match cursor.peek_byte() {
            Some(b'>') => {
                cursor.next_byte();
                return Ok(Event::StartElement { name, attributes });
            }
            Some(b'/') => {
                cursor.next_byte();
                if !cursor.skip_byte(b'>') {
                    return Err(if cursor.is_empty() {
                        Error::Incomplete
                    } else {
                        Error::Malformed
                    });
                }
                return Ok(Event::EmptyElement { name, attributes });
            }
            None => return Err(Error::Incomplete),
            Some(_) => {
                if whitespace == 0 {
                    return Err(Error::Malformed);
                }
                let attribute = parse_attribute(cursor)?;
                if attributes
                    .iter()
                    .any(|other: &Attribute<'a>| other.name() == attribute.name())
                {
                    return Err(Error::Malformed);
                }
                attributes.push(attribute);
            }
        }
    }
}

fn parse_attribute<'a>(cursor: &mut ChunkedCursor<'a, u8>) -> Result<Attribute<'a>, Error> {
    let name = cursor.take_until_any(NAME_DELIMITERS);
    if name.byte_len() == 0 {
        return Err(if cursor.is_empty() {
            Error::Incomplete
        } else {
            Error::Malformed
        });
    }
    if !is_name(name) {
        return Err(Error::Malformed);
    }
    if cursor.is_empty() {
        return Err(Error::Incomplete);
    }
    cursor.skip_while_any(XML_WHITESPACE);
    if cursor.peek_byte() != Some(b'=') {
        return Err(if cursor.is_empty() {
            Error::Incomplete
        } else {
            Error::Malformed
        });
    }
    cursor.next_byte();
    cursor.skip_while_any(XML_WHITESPACE);

    let quote = match cursor.next_byte() {
        Some(quote @ (b'\'' | b'"')) => quote,
        None => return Err(Error::Incomplete),
        Some(_) => return Err(Error::Malformed),
    };
    let value = cursor.take_until_byte(quote);
    if value.contains_any(b"<") {
        return Err(Error::Malformed);
    }
    if cursor.is_empty() {
        return Err(Error::Incomplete);
    }
    cursor.next_byte();
    Ok(Attribute { name, value })
}

fn take_name<'a>(cursor: &mut ChunkedCursor<'a, u8>) -> Result<Pieces<'a, u8>, Error> {
    let name = cursor.take_until_any(NAME_DELIMITERS);
    if name.byte_len() == 0 {
        return Err(if cursor.is_empty() {
            Error::Incomplete
        } else {
            Error::Malformed
        });
    }
    if !is_name(name) {
        return Err(Error::Malformed);
    }
    if cursor.is_empty() {
        return Err(Error::Incomplete);
    }
    Ok(name)
}

fn is_xml_declaration_start(cursor: &ChunkedCursor<'_, u8>) -> bool {
    if !starts_with(cursor, b"xml") {
        return false;
    }
    let mut probe = *cursor;
    probe.next_byte();
    probe.next_byte();
    probe.next_byte();
    matches!(
        probe.peek_byte(),
        None | Some(b' ' | b'\t' | b'\r' | b'\n' | b'?')
    )
}

fn parse_declaration<'a>(cursor: &mut ChunkedCursor<'a, u8>) -> Result<Event<'a>, Error> {
    consume_bytes(cursor, b"xml")?;
    if cursor.is_empty() {
        return Err(Error::Incomplete);
    }
    if cursor.skip_while_any(XML_WHITESPACE) == 0 {
        return Err(Error::Malformed);
    }

    let mut attributes = Vec::new();
    let mut first = true;
    loop {
        let whitespace = if first {
            first = false;
            1
        } else {
            cursor.skip_while_any(XML_WHITESPACE)
        };
        match cursor.peek_byte() {
            Some(b'?') => {
                consume_bytes(cursor, b"?>")?;
                return Ok(Event::XmlDeclaration { attributes });
            }
            None => return Err(Error::Incomplete),
            Some(_) => {
                if whitespace == 0 {
                    return Err(Error::Malformed);
                }
                let attribute = parse_attribute(cursor)?;
                if attributes
                    .iter()
                    .any(|other: &Attribute<'a>| other.name() == attribute.name())
                {
                    return Err(Error::Malformed);
                }
                attributes.push(attribute);
            }
        }
    }
}

fn parse_processing_instruction<'a>(
    cursor: &mut ChunkedCursor<'a, u8>,
) -> Result<Event<'a>, Error> {
    let target = cursor.take_until_any(b" \t\r\n?");
    if target.byte_len() == 0 {
        return Err(if cursor.is_empty() {
            Error::Incomplete
        } else {
            Error::Malformed
        });
    }
    if !is_name(target) {
        return Err(Error::Malformed);
    }
    if cursor.is_empty() {
        return Err(Error::Incomplete);
    }

    let whitespace = cursor.skip_while_any(XML_WHITESPACE);
    if whitespace == 0 {
        match cursor.peek_byte() {
            Some(b'?') => {
                let mut probe = *cursor;
                probe.next_byte();
                match probe.peek_byte() {
                    Some(b'>') => {}
                    None => return Err(Error::Incomplete),
                    Some(_) => return Err(Error::Malformed),
                }
            }
            None => return Err(Error::Incomplete),
            Some(_) => return Err(Error::Malformed),
        }
    }

    let (data, found) = take_until_sequence(cursor, b"?>");
    if !found {
        return Err(Error::Incomplete);
    }
    consume_bytes(cursor, b"?>")?;

    Ok(Event::ProcessingInstruction {
        target,
        data: data.trim_start(XML_WHITESPACE),
    })
}

fn parse_declaration_block<'a>(cursor: &mut ChunkedCursor<'a, u8>) -> Result<Event<'a>, Error> {
    if starts_with(cursor, b"--") {
        consume_bytes(cursor, b"--")?;
        let (contents, found) = take_until_sequence(cursor, b"-->");
        if !found {
            return Err(Error::Incomplete);
        }
        if contains_double_hyphen(contents) {
            return Err(Error::Malformed);
        }
        consume_bytes(cursor, b"-->")?;
        return Ok(Event::Comment(contents));
    }

    if starts_with(cursor, b"[CDATA[") {
        consume_bytes(cursor, b"[CDATA[")?;
        let (contents, found) = take_until_sequence(cursor, b"]]>");
        if !found {
            return Err(Error::Incomplete);
        }
        consume_bytes(cursor, b"]]>")?;
        return Ok(Event::CData(contents));
    }

    if starts_with(cursor, b"DOCTYPE") {
        consume_bytes(cursor, b"DOCTYPE")?;
        if cursor.is_empty() {
            return Err(Error::Incomplete);
        }
        if cursor.skip_while_any(XML_WHITESPACE) == 0 {
            return Err(Error::Malformed);
        }

        let mut depth = 0usize;
        let mut quote = None;
        let mut invalid = false;
        let contents = cursor.take_until(|&byte| {
            if invalid {
                return true;
            }
            if let Some(open_quote) = quote {
                if byte == open_quote {
                    quote = None;
                }
                return false;
            }
            match byte {
                b'\'' | b'"' => {
                    quote = Some(byte);
                    false
                }
                b'[' => {
                    depth += 1;
                    false
                }
                b']' => {
                    if depth == 0 {
                        invalid = true;
                        true
                    } else {
                        depth -= 1;
                        false
                    }
                }
                b'>' if depth == 0 => true,
                _ => false,
            }
        });
        if invalid {
            return Err(Error::Malformed);
        }
        if cursor.is_empty() {
            return Err(Error::Incomplete);
        }
        consume_bytes(cursor, b">")?;
        let contents = contents.trim(XML_WHITESPACE);
        if !is_doctype_name(contents) {
            return Err(Error::Malformed);
        }
        return Ok(Event::Doctype(contents));
    }

    Err(Error::Malformed)
}

fn take_until_sequence<'a>(
    cursor: &mut ChunkedCursor<'a, u8>,
    pattern: &[u8],
) -> (Pieces<'a, u8>, bool) {
    let contents = cursor.take_until_bytes(pattern);
    let mut probe = *cursor;
    let found = pattern
        .iter()
        .all(|expected| probe.next_byte() == Some(*expected));
    (contents, found)
}

fn consume_bytes(cursor: &mut ChunkedCursor<'_, u8>, expected: &[u8]) -> Result<(), Error> {
    for &byte in expected {
        match cursor.next_byte() {
            Some(actual) if actual == byte => {}
            Some(_) => return Err(Error::Malformed),
            None => return Err(Error::Incomplete),
        }
    }
    Ok(())
}

fn starts_with(cursor: &ChunkedCursor<'_, u8>, prefix: &[u8]) -> bool {
    let mut probe = *cursor;
    prefix
        .iter()
        .all(|expected| probe.next_byte() == Some(*expected))
}

fn is_name(name: Pieces<'_, u8>) -> bool {
    let mut first = true;
    let mut any = false;
    for piece in name {
        for &byte in piece {
            let valid = if first {
                byte.is_ascii_alphabetic() || matches!(byte, b'_' | b':') || byte >= 0x80
            } else {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'_' | b':' | b'-' | b'.')
                    || byte >= 0x80
            };
            if !valid {
                return false;
            }
            first = false;
            any = true;
        }
    }
    any
}

fn is_xml_whitespace(value: Pieces<'_, u8>) -> bool {
    value
        .into_iter()
        .flat_map(|piece| piece.iter().copied())
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

fn contains_double_hyphen(value: Pieces<'_, u8>) -> bool {
    let mut previous = false;
    for piece in value {
        for &byte in piece {
            if previous && byte == b'-' {
                return true;
            }
            previous = byte == b'-';
        }
    }
    false
}

fn is_doctype_name(value: Pieces<'_, u8>) -> bool {
    let mut first = true;
    let mut any = false;
    for piece in value {
        for &byte in piece {
            if matches!(
                byte,
                b' ' | b'\t' | b'\r' | b'\n' | b'[' | b'>' | b'\'' | b'"'
            ) {
                return any && !first;
            }
            let valid = if first {
                byte.is_ascii_alphabetic() || matches!(byte, b'_' | b':') || byte >= 0x80
            } else {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'_' | b':' | b'-' | b'.')
                    || byte >= 0x80
            };
            if !valid {
                return false;
            }
            first = false;
            any = true;
        }
    }
    any
}
