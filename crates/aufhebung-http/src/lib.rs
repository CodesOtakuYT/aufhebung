//! # aufhebung-http
//!
//! A simple zero-copy [HTTP/1.1] request parser built on `aufhebung-core`'s
//! chunked cursor. It reads the request line and header block straight out of
//! a chunk list (`&[&[u8]]`) — a contiguous buffer is just a one-element
//! chunk list — so every field stays a zero-copy [`Pieces`] span even when it
//! straddles chunk boundaries. Nothing is copied or allocated on the scanning
//! path except the header list itself.
//!
//! The parser covers the request line and header block only: it applies no
//! body framing (no `Content-Length` or chunked transfer-encoding accounting,
//! no responses). The bytes after the header block are exposed
//! uninterpreted as the body.
//!
//! # Examples
//!
//! Flat input (a single buffer) as a one-element chunk list:
//!
//! ```
//! use aufhebung_http::Request;
//!
//! const WIRE: &[&[u8]] = &[
//!     b"GET /index.html HTTP/1.1\r\n",
//!     b"Host: example.com\r\n",
//!     b"Content-Length: 0\r\n",
//!     b"\r\n",
//! ];
//! let req = Request::parse(WIRE).unwrap();
//! assert!(req.method() == b"GET");
//! assert!(req.target() == b"/index.html");
//! assert_eq!(req.header(b"host").map(|v| v == b"example.com"), Some(true));
//! assert_eq!(req.content_length(), Some(0));
//! ```
//!
//! Fragmented input — fields may straddle chunk boundaries:
//!
//! ```
//! use aufhebung_http::Request;
//!
//! const WIRE: &[&[u8]] = &[
//!     b"POST /submit HTT",
//!     b"P/1.1\r\nHost: exa",
//!     b"mple.com\r\nContent-Length: 5\r\n\r\nhello",
//! ];
//! let req = Request::parse(WIRE).unwrap();
//! assert!(req.version() == b"HTTP/1.1"); // straddles chunks 1–2
//! assert_eq!(req.content_length(), Some(5)); // digits straddle too
//! assert!(req.body() == b"hello");
//! ```
//!
//! A request that borrows a runtime buffer: hold the chunk list yourself, the
//! request borrows it:
//!
//! ```
//! use aufhebung_http::Request;
//!
//! let wire: &[u8] = b"GET / HTTP/1.1\r\nHost: x\r\n\r\n";
//! let chunks = [wire];
//! let req = Request::parse(&chunks).unwrap();
//! assert_eq!(req.content_length(), None);
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use aufhebung_core::ChunkedCursor;

/// The zero-copy span type of `aufhebung-core` — here so the type returned by
/// this crate's accessors is nameable through this crate alone.
pub use aufhebung_core::Pieces;

/// Errors from parsing an HTTP/1.1 request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The input ended in the middle of a request: more bytes may still be on
    /// the way, so a streaming caller should keep the chunks and retry once
    /// additional input arrives.
    Incomplete,
    /// The input violates the HTTP/1.1 request grammar (a bad request line or
    /// a malformed header field).
    Malformed,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Incomplete => f.write_str("input ended mid-request"),
            Error::Malformed => f.write_str("malformed HTTP/1.1 request"),
        }
    }
}

impl core::error::Error for Error {}

/// A zero-copy HTTP/1.1 request: the request line, header fields, and body
/// all borrow from the chunk list given to [`Request::parse`].
///
/// Header names and values are trimmed of optional whitespace (OWS: SP and
/// HTAB), as HTTP/1.1 defines field values. Field lookup is case-insensitive,
/// because header names are.
#[derive(Clone)]
pub struct Request<'a> {
    method: Pieces<'a, u8>,
    target: Pieces<'a, u8>,
    version: Pieces<'a, u8>,
    headers: Vec<(Pieces<'a, u8>, Pieces<'a, u8>)>,
    body: Pieces<'a, u8>,
}

impl<'a> Request<'a> {
    /// Parse an HTTP/1.1 request from a chunk list.
    ///
    /// `chunks` is the byte stream the request was received in; the returned
    /// request borrows it, and its fields may straddle chunk boundaries
    /// freely. A contiguous buffer is a one-element chunk list: `[input]`.
    ///
    /// Grammar accepted: `METHOD SP request-target SP HTTP/x.y CRLF`, then
    /// header fields `name ":" OWS value OWS CRLF` until a blank line; the
    /// span after the blank line becomes the [body](Request::body). CRLF and
    /// bare-LF line endings are both accepted.
    ///
    /// Returns [`Error::Incomplete`] when the input ends in the middle of a
    /// request — a streaming caller should wait for more data — and
    /// [`Error::Malformed`] when present bytes violate the grammar.
    pub fn parse(chunks: &'a [&'a [u8]]) -> Result<Request<'a>, Error> {
        let mut c = ChunkedCursor::new(chunks);

        // request line: METHOD SP request-target SP HTTP-version CRLF
        let method = c.take_until_any(b" \r\n");
        if method.byte_len() == 0 {
            return Err(eof_or_malformed(&c));
        }
        expect_sp(&mut c)?;
        let target = c.take_until_any(b" \r\n");
        if target.byte_len() == 0 {
            return Err(eof_or_malformed(&c));
        }
        expect_sp(&mut c)?;
        let version = c.take_until_any(b"\r\n");
        if version.byte_len() == 0 {
            return Err(eof_or_malformed(&c));
        }
        if !version.starts_with(b"HTTP/") {
            // a completed version line must carry the HTTP/ prefix; but if the
            // input ended before the line end we cannot tell a truncated
            // version from a bad one, so a streaming caller gets Incomplete
            return Err(eof_or_malformed(&c));
        }
        expect_line_end(&mut c)?;

        // header fields until the blank line
        let mut headers = Vec::new();
        loop {
            match c.peek_byte() {
                // blank line ends the header block
                Some(b'\r') | Some(b'\n') => {
                    expect_line_end(&mut c)?;
                    break;
                }
                None => return Err(Error::Incomplete),
                Some(_) => {
                    let name = c.take_until_byte(b':');
                    match c.peek_byte() {
                        Some(b':') => {
                            let _ = c.next_byte();
                        }
                        None => {
                            // no colon seen: if a line end slipped into the
                            // span the field is malformed, otherwise the rest
                            // of the line may still be on the way
                            return Err(if pieces_contain_crlf(name) {
                                Error::Malformed
                            } else {
                                Error::Incomplete
                            });
                        }
                        Some(_) => return Err(Error::Malformed),
                    }
                    if name.byte_len() == 0 {
                        return Err(Error::Malformed);
                    }
                    c.skip_while_any(b" \t"); // leading OWS
                    let value = c.take_until_any(b"\r\n").trim(b" \t"); // trailing OWS
                    expect_line_end(&mut c)?;
                    headers.push((name, value));
                }
            }
        }

        let body = c.take_rest();
        Ok(Request {
            method,
            target,
            version,
            headers,
            body,
        })
    }

    /// The request method (`GET`, `POST`, …).
    pub fn method(&self) -> Pieces<'a, u8> {
        self.method
    }

    /// The request target (`/index.html`, `*`, …).
    pub fn target(&self) -> Pieces<'a, u8> {
        self.target
    }

    /// The protocol version (`HTTP/1.1`, …).
    pub fn version(&self) -> Pieces<'a, u8> {
        self.version
    }

    /// The header fields, in order, as zero-copy `(name, value)` pairs.
    pub fn headers(&self) -> &[(Pieces<'a, u8>, Pieces<'a, u8>)] {
        &self.headers
    }

    /// The first header field whose name equals `name` (ignoring ASCII case),
    /// trimmed of optional whitespace. `None` when no such field exists.
    pub fn header(&self, name: &[u8]) -> Option<Pieces<'a, u8>> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| *v)
    }

    /// The value of the `Content-Length` header as an integer, when the field
    /// is present and well-formed (an optional sign and decimal digits within
    /// `i64`'s range).
    pub fn content_length(&self) -> Option<i64> {
        self.header(b"content-length")
            .and_then(|v| v.parse_integer())
    }

    /// The bytes after the header block, uninterpreted: no `Content-Length`
    /// or transfer-encoding framing is applied by this parser.
    pub fn body(&self) -> Pieces<'a, u8> {
        self.body
    }
}

/// `Incomplete` when the cursor is at end-of-input, `Malformed` otherwise —
/// for a required request-line field that came up empty.
fn eof_or_malformed(c: &ChunkedCursor<'_, u8>) -> Error {
    if c.peek_byte().is_none() {
        Error::Incomplete
    } else {
        Error::Malformed
    }
}

/// Consume exactly one SP, the request-line field separator.
fn expect_sp(c: &mut ChunkedCursor<'_, u8>) -> Result<(), Error> {
    match c.peek_byte() {
        Some(b' ') => {
            let _ = c.next_byte();
            Ok(())
        }
        None => Err(Error::Incomplete),
        Some(_) => Err(Error::Malformed),
    }
}

/// Consume exactly one CRLF or bare-LF line ending.
fn expect_line_end(c: &mut ChunkedCursor<'_, u8>) -> Result<(), Error> {
    match c.peek_byte() {
        Some(b'\r') => {
            let _ = c.next_byte();
            match c.peek_byte() {
                Some(b'\n') => {
                    let _ = c.next_byte();
                    Ok(())
                }
                None => Err(Error::Incomplete),
                Some(_) => Err(Error::Malformed),
            }
        }
        Some(b'\n') => {
            let _ = c.next_byte();
            Ok(())
        }
        None => Err(Error::Incomplete),
        Some(_) => Err(Error::Malformed),
    }
}

/// Whether any piece of `span` contains a CR or LF byte.
fn pieces_contain_crlf(span: Pieces<'_, u8>) -> bool {
    span.into_iter()
        .any(|piece| piece.iter().any(|&b| b == b'\r' || b == b'\n'))
}
