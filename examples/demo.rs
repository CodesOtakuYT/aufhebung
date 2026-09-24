//! Demonstrations of the library's cursor pattern:
//!
//! 1. `split_whitespace` on a single slice, and on a *chunked* stream
//!    ([`ChunkedCursor`]) where words may span chunk boundaries.
//! 2. A small zero-copy HTTP/1 parser for a single buffer, built on the flat
//!    cursor methods.
//! 3. The same request delivered across three chunks, parsed with
//!    [`ChunkedCursor`] — fields may straddle chunk boundaries, and the
//!    `Pieces` value ops (`==`, `Hash`, `parse_integer`) are used directly on
//!    the pieces without collecting.
//!
//! Run with: `cargo run --example demo`

use aufhebung::{ByteSliceCursor, ChunkedCursor};

/// Render a byte slice for display.
fn as_str(s: &[u8]) -> String {
    String::from_utf8_lossy(s).into_owned()
}

/// Flatten an iterator of sub-slices into a new `[u8]` buffer.
fn concat<'a>(pieces: impl Iterator<Item = &'a [u8]>) -> Vec<u8> {
    pieces.flat_map(|p| p.iter().copied()).collect()
}

/// Read one CRLF (or bare-LF) terminated line, returned without its
/// terminator. `None` when nothing is left.
fn read_line<'a>(rest: &mut &'a [u8]) -> Option<&'a [u8]> {
    if rest.is_empty() {
        return None;
    }
    let line = rest.take_until_byte_incl(b'\n');
    let line = line.strip_suffix(b"\r\n").unwrap_or(line);
    Some(line.strip_suffix(b"\n").unwrap_or(line))
}

/// Trim optional whitespace (SP, HTAB) from both ends.
fn trim_ows(s: &[u8]) -> &[u8] {
    let is_ows = |b: &u8| *b == b' ' || *b == b'\t';
    let start = s.iter().position(|b| !is_ows(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !is_ows(b)).map_or(start, |i| i + 1);
    &s[start..end]
}

/// A zero-copy HTTP/1 request: every field borrows from the input buffer.
struct Request<'a> {
    method: &'a [u8],
    target: &'a [u8],
    version: &'a [u8],
    headers: Vec<(&'a [u8], &'a [u8])>,
    body: &'a [u8],
}

/// Parse an HTTP/1 request line and header block, with `body` as the rest.
fn parse_request<'a>(input: &'a [u8]) -> Option<Request<'a>> {
    let mut rest: &'a [u8] = input;

    // request line: METHOD SP target SP HTTP-version CRLF
    let method = rest.take_until_byte(b' ');
    rest.skip_byte(b' ');
    let target = rest.take_until_byte(b' ');
    rest.skip_byte(b' ');
    let version = read_line(&mut rest)?; // "HTTP/1.1\r\n"

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
        let value = trim_ows(field);
        headers.push((name, value));
    }

    Some(Request {
        method,
        target,
        version,
        headers,
        body: rest,
    })
}

fn main() {
    // The library's split_whitespace on a single slice:
    let flat: &[u8] = b"  hello \t world!\n";
    println!("flat stream: {:?}", str::from_utf8(flat).unwrap());
    for word in flat.split_whitespace() {
        println!("  word: {:?}", str::from_utf8(word).unwrap());
    }
    println!();

    // Words spanning a chunked byte stream:
    let chunks: &[&[u8]] = &[b"he", b"llo wo", b"rld par", b"t   two"];
    println!(
        "chunked stream: {:?}",
        chunks
            .iter()
            .map(|c| String::from_utf8_lossy(c))
            .collect::<Vec<_>>()
    );

    for (n, pieces) in ChunkedCursor::new(chunks).split_whitespace().enumerate() {
        let mut whole = Vec::new();
        for piece in pieces {
            whole.extend_from_slice(piece);
            println!("  word {n}: piece {:?}", str::from_utf8(piece).unwrap());
        }
        println!("word {n}: {:?}", str::from_utf8(&whole).unwrap());
    }

    println!();
    println!("=== HTTP/1: one contiguous buffer (flat cursor) ===");

    let wire: &[u8] = b"POST /submit HTTP/1.1\r\n\
        Host: example.com\r\n\
        Content-Type: text/plain\r\n\
        Content-Length: 5\r\n\
        \r\n\
        hello";

    match parse_request(wire) {
        Some(req) => {
            println!(
                "request line: {} {} {}",
                as_str(req.method),
                as_str(req.target),
                as_str(req.version)
            );
            println!("{} header field(s):", req.headers.len());
            for (name, value) in &req.headers {
                println!("  {}: {}", as_str(name), as_str(value));
            }
            println!("body: {:?} ({} bytes)", as_str(req.body), req.body.len());
        }
        None => println!("malformed request"),
    }

    println!();
    println!("=== HTTP/1: one request across three chunks ====");

    // The version and Content-Length fields straddle chunk boundaries.
    let chunks: &[&[u8]] = &[
        b"POST /submit HTT",
        b"P/1.1\r\nHost: example.com",
        b"\r\nContent-Length: 5\r\n\r\nhello",
    ];
    let mut c = ChunkedCursor::new(chunks);

    // Request line: METHOD SP target SP HTTP-version CRLF.
    let method = c.take_until(|&b| b == b' ' || b == b'\r' || b == b'\n');
    c.next_byte(); // SP
    let target = c.take_until(|&b| b == b' ' || b == b'\r' || b == b'\n');
    c.next_byte(); // SP
    let version = c.take_until(|&b| b == b'\r' || b == b'\n');
    c.next_byte(); // CR
    c.next_byte(); // LF

    println!(
        "request line: {} {} {}",
        as_str(&concat(method)),
        as_str(&concat(target)),
        as_str(&concat(version))
    );
    println!(
        "  method  {:?} (matches b\"POST\")     -> {}",
        as_str(&concat(method)),
        method == b"POST"
    );
    println!(
        "  version {:?} (matches b\"HTTP/1.1\") -> {}",
        as_str(&concat(version)),
        version == b"HTTP/1.1"
    );
    println!("  header fields:");

    let mut declared_length = None;
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
                let name = c.take_until(|&b| b == b':');
                c.next_byte(); // ':'
                c.skip_while(|&b| b == b' ' || b == b'\t'); // leading OWS
                let value = c.take_until(|&b| b == b'\r' || b == b'\n');
                c.next_byte(); // CR
                c.next_byte(); // LF
                if name == b"Content-Length" {
                    declared_length = value.parse_integer();
                }
                println!("    {}: {}", as_str(&concat(name)), as_str(&concat(value)));
            }
        }
    }

    let body: Vec<u8> = concat(c.take_until(|_| false));
    println!("  body: {:?} ({} bytes)", as_str(&body), body.len());
    println!("  Content-Length parsed straight from the pieces: {declared_length:?}");
}
