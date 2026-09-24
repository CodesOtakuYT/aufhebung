//! Shared fixtures, parsers, and baselines for the benchmark harnesses.
//!
//! The parsers here mirror the demo's HTTP/1 parser (flat and chunked) so the
//! benches measure the exact code the library demonstrates. Each bench binary
//! compiles this module via `mod support;`, so use only what you need.

#![allow(dead_code)]

use aufhebung::{ByteSliceCursor, ChunkedCursor, Pieces};

/// The request from the demo: request line + 3 headers + body.
pub const SMALL_REQUEST: &[u8] = b"POST /submit HTTP/1.1\r\n\
    Host: example.com\r\n\
    Content-Type: text/plain\r\n\
    Content-Length: 5\r\n\
    \r\n\
    hello";

/// A header-heavy request: request line + 12 headers, longer values.
pub const LARGE_REQUEST: &[u8] = b"GET /search?q=aufhebung&lang=rust&page=1 HTTP/1.1\r\n\
    Host: example.com\r\n\
    User-Agent: aufhebung-bench/0.1 (memchr-powered)\r\n\
    Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,*/*;q=0.8\r\n\
    Accept-Language: en-US,en;q=0.9\r\n\
    Accept-Encoding: gzip, deflate, br\r\n\
    Connection: keep-alive\r\n\
    Referer: https://example.com/previous-page\r\n\
    Cookie: session=abc123def456; theme=dark; lang=rust\r\n\
    Cache-Control: no-cache\r\n\
    Sec-Fetch-Dest: document\r\n\
    Sec-Fetch-Mode: navigate\r\n\
    Content-Length: 0\r\n\
    \r\n";

/// Read one CRLF (or bare-LF) terminated line, without its terminator.
/// `None` when nothing is left.
pub fn read_line<'a>(rest: &mut &'a [u8]) -> Option<&'a [u8]> {
    if rest.is_empty() {
        return None;
    }
    let line = rest.take_until_byte_incl(b'\n');
    let line = line.strip_suffix(b"\r\n").unwrap_or(line);
    Some(line.strip_suffix(b"\n").unwrap_or(line))
}

/// The demo's flat parser, verbatim: header fields as `(name, value)`.
pub fn parse_request(input: &[u8]) -> Option<Vec<(&[u8], &[u8])>> {
    let mut rest: &[u8] = input;

    // request line: METHOD SP target SP HTTP-version CRLF
    rest.take_until_byte(b' ');
    rest.skip_byte(b' ');
    rest.take_until_byte(b' ');
    rest.skip_byte(b' ');
    read_line(&mut rest)?; // "HTTP/1.1\r\n"

    // header fields until the blank line
    let mut headers = Vec::new();
    loop {
        let line = read_line(&mut rest)?;
        if line.is_empty() {
            break;
        }
        let mut field: &[u8] = line;
        let name = field.take_until_byte(b':');
        field.skip_byte(b':');
        // trim OWS (SP, HTAB) off both ends of the value
        let value = field.trim(b" \t");
        headers.push((name, value));
    }
    Some(headers)
}

/// The same parser with a fixed-size header array instead of a `Vec`, so the
/// benches can separate scanning speed from allocator cost (httparse is
/// zero-alloc, so this is the apples-to-apples variant).
pub fn parse_request_noalloc(input: &[u8]) -> Option<(usize, usize)> {
    let mut rest: &[u8] = input;

    rest.take_until_byte(b' ');
    rest.skip_byte(b' ');
    rest.take_until_byte(b' ');
    rest.skip_byte(b' ');
    read_line(&mut rest)?;

    let mut n = 0;
    let mut total = 0;
    loop {
        let line = read_line(&mut rest)?;
        if line.is_empty() {
            break;
        }
        let mut field: &[u8] = line;
        let name = field.take_until_byte(b':');
        field.skip_byte(b':');
        let value = field.trim(b" \t");
        total += name.len() + value.len();
        n += 1;
    }
    Some((n, total))
}

/// A reference parser with no memchr: plain linear scans, so the benches show
/// what the acceleration buys.
pub fn parse_request_naive(input: &[u8]) -> Option<Vec<(&[u8], &[u8])>> {
    fn take_until<'a>(rest: &mut &'a [u8], needle: u8) -> Option<&'a [u8]> {
        let n = rest.iter().position(|&b| b == needle)?;
        let out = &rest[..n];
        *rest = &rest[n + 1..];
        Some(out)
    }
    fn take_line<'a>(rest: &mut &'a [u8]) -> Option<&'a [u8]> {
        if rest.is_empty() {
            return None;
        }
        let n = rest.iter().position(|&b| b == b'\n')?;
        let line = &rest[..n];
        *rest = &rest[n + 1..];
        Some(line.strip_suffix(b"\r").unwrap_or(line))
    }
    let mut rest = input;
    take_until(&mut rest, b' ')?;
    take_until(&mut rest, b' ')?;
    take_line(&mut rest)?;
    let mut headers = Vec::new();
    loop {
        let line = take_line(&mut rest)?;
        if line.is_empty() {
            break;
        }
        let mut split = line.splitn(2, |&b| b == b':');
        let name = split.next()?;
        let raw = split.next()?;
        let start = raw
            .iter()
            .position(|&b| b != b' ' && b != b'\t')
            .unwrap_or(raw.len());
        let value = &raw[start..];
        headers.push((name, value));
    }
    Some(headers)
}

/// The demo's chunked parser: request line + headers read across chunk
/// boundaries, fields may straddle chunks. Fields are returned as `Pieces`
/// (one sub-slice per chunk touched), the same type the demo works with.
pub type ChunkedRequest<'a> = (usize, usize, usize, Vec<(Pieces<'a, u8>, Pieces<'a, u8>)>);

pub fn parse_request_chunked<'a>(input: &'a [&'a [u8]]) -> Option<ChunkedRequest<'a>> {
    let mut c = ChunkedCursor::new(input);

    // request line: METHOD SP target SP HTTP-version CRLF
    let method = c.take_until_any(b" \r\n");
    let method_len = method.byte_len();
    c.next_byte(); // SP
    let target = c.take_until_any(b" \r\n");
    let target_len = target.byte_len();
    c.next_byte(); // SP
    let version = c.take_until_any(b"\r\n");
    let version_len = version.byte_len();
    c.next_byte(); // CR
    c.next_byte(); // LF

    // header fields until the blank line
    let mut headers = Vec::new();
    loop {
        match c.peek_byte() {
            None => break,
            // blank line ends the header block
            Some(b'\r') | Some(b'\n') => {
                c.next_byte();
                c.next_byte();
                break;
            }
            Some(_) => {
                let name = c.take_until_byte(b':');
                c.next_byte(); // ':'
                c.skip_while_any(b" \t"); // leading OWS
                let value = c.take_until_any(b"\r\n");
                c.next_byte(); // CR
                c.next_byte(); // LF
                headers.push((name, value));
            }
        }
    }
    Some((method_len, target_len, version_len, headers))
}

/// Split `input` into `n` roughly equal pieces, preserving order.
pub fn split_chunks(input: &[u8], n: usize) -> Vec<&[u8]> {
    let base = input.len() / n;
    let rem = input.len() % n;
    let mut out = Vec::with_capacity(n);
    let mut start = 0;
    for i in 0..n {
        let end = start + base + usize::from(i < rem);
        out.push(&input[start..end]);
        start = end;
    }
    out
}
// ---- streaming-fragmentation fixtures (benches/fragmentation.rs) -----------

/// Split `input` into slices of at most `size` bytes, preserving order.
pub fn split_by_size(input: &[u8], size: usize) -> Vec<&[u8]> {
    input.chunks(size).collect()
}

/// A ~7 KiB HTTP/1 request (request line + 140 headers + blank line).
pub fn build_http_payload() -> Vec<u8> {
    let mut v = Vec::with_capacity(8192);
    v.extend_from_slice(b"GET /search?q=aufhebung&lang=rust&page=1 HTTP/1.1\r\n");
    for i in 0..140 {
        v.extend_from_slice(
            format!("X-Header-{i:03}: value-{i} padding-0123456789abcdef\r\n").as_bytes(),
        );
    }
    v.extend_from_slice(b"Host: example.com\r\nContent-Length: 0\r\n\r\n");
    v
}

/// An ~7.6 KiB JSON document (86 objects with nested strings and arrays).
pub fn build_json_payload() -> Vec<u8> {
    let mut v = Vec::with_capacity(8192);
    v.extend_from_slice(b"{\n");
    for i in 0..86 {
        v.extend_from_slice(
            format!(
                "  \"field_{i:03}\": {{\"id\": {i}, \"name\": \"item-{i}-abcdefgh\", \
                 \"tags\": [\"alpha\", \"beta\", \"gamma\"]}},\n"
            )
            .as_bytes(),
        );
    }
    v.extend_from_slice(b"  \"last\": true\n}\n");
    v
}

/// An ~8 KiB XML document (88 elements, each with an attribute-bearing child).
pub fn build_xml_payload() -> Vec<u8> {
    let mut v = Vec::with_capacity(8192);
    v.extend_from_slice(b"<?xml version=\"1.0\"?>\n<root>\n");
    for i in 0..88 {
        v.extend_from_slice(
            format!(
                "  <item id=\"{i}\" name=\"item-{i}\">\n    <child key=\"value-{i}\" \
                 note=\"0123456789abcdef\"/>\n  </item>\n"
            )
            .as_bytes(),
        );
    }
    v.extend_from_slice(b"</root>\n");
    v
}

/// A synthetic ~7.7 KiB stream of DNS-like messages, each a 12-byte header, a
/// zero-terminated name (length-prefixed labels), and 4 fixed tail bytes.
/// Synthetic because real DNS messages are far smaller; this paces the parsers
/// with a realistic binary layout.
pub fn build_dns_payload() -> Vec<u8> {
    let mut v = Vec::with_capacity(8192);
    for _ in 0..240 {
        v.extend_from_slice(&[0u8; 12]); // message header
        for label in [&b"www"[..], &b"example"[..], &b"com"[..]] {
            v.push(label.len() as u8);
            v.extend_from_slice(label);
        }
        v.push(0); // label terminator
        v.extend_from_slice(&[0u8; 4]); // type + class
    }
    v
}

/// Count-returning wrapper so the HTTP parser fits the shared scan signature.
pub fn scan_http(input: &[&[u8]]) -> usize {
    parse_request_chunked(input)
        .map(|(m, t, v, h)| m + t + v + h.len())
        .unwrap_or(0)
}

/// Minimal JSON lexer: counts structural tokens, strings, and scalars.
pub fn scan_json_tokens(input: &[&[u8]]) -> usize {
    let mut c = ChunkedCursor::new(input);
    let mut tokens = 0;
    loop {
        c.skip_while_any(b" \t\r\n");
        match c.peek_byte() {
            None => break,
            Some(b'{') | Some(b'}') | Some(b'[') | Some(b']') | Some(b',') | Some(b':') => {
                c.next_byte();
                tokens += 1;
            }
            Some(b'"') => {
                c.next_byte(); // opening quote
                c.skip_until_byte(b'"'); // contents (payloads carry no escapes)
                c.next_byte(); // closing quote
                tokens += 1;
            }
            Some(_) => {
                let scalar = c.take_until_any(b" \t\r\n,]}");
                if scalar.byte_len() == 0 {
                    break; // nothing to lex; stop
                }
                tokens += 1;
            }
        }
    }
    tokens
}

/// Minimal XML lexer: counts text runs and element/tag names.
pub fn scan_xml_tokens(input: &[&[u8]]) -> usize {
    let mut c = ChunkedCursor::new(input);
    let mut tokens = 0;
    loop {
        // text run up to '<'
        let text = c.take_until_byte(b'<');
        if text.byte_len() > 0 {
            tokens += 1;
        }
        match c.peek_byte() {
            None => break,
            Some(b'<') => {
                c.next_byte();
                // closing tags ("</item>") and processing instructions ("<?xml")
                if matches!(c.peek_byte(), Some(b'/') | Some(b'?')) {
                    c.next_byte();
                }
                let name = c.take_until_any(b"> \t\r\n");
                if name.byte_len() == 0 {
                    break; // e.g. "<" at end of stream
                }
                tokens += 1;
                // skip attributes up to '>'
                c.skip_until_byte(b'>');
                match c.peek_byte() {
                    Some(b'>') => {
                        c.next_byte();
                    }
                    _ => break,
                }
            }
            Some(_) => break,
        }
    }
    tokens
}

/// Skip `n` bytes, bridging chunks; `false` if the stream runs out first.
fn skip_bytes(c: &mut ChunkedCursor<'_, u8>, n: usize) -> bool {
    for _ in 0..n {
        if c.next_byte().is_none() {
            return false;
        }
    }
    true
}

/// Walk a synthetic DNS record stream: 12-byte header, zero-terminated name,
/// 4 fixed tail bytes per record. Returns the number of complete records.
pub fn scan_dns_records(input: &[&[u8]]) -> usize {
    let mut c = ChunkedCursor::new(input);
    let mut records = 0;
    loop {
        if !skip_bytes(&mut c, 12) {
            break; // header
        }
        let name = c.take_until_byte(0);
        if name.byte_len() == 0 {
            break; // no name bytes
        }
        c.next_byte(); // the zero terminator
        if !skip_bytes(&mut c, 4) {
            break; // type + class
        }
        records += 1;
    }
    records
}
