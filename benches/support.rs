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
