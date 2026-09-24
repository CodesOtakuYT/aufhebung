//! Demonstrations of the library's cursor pattern:
//!
//! 1. `split_whitespace` on a single slice, and on a *chunked* stream
//!    ([`ChunkedCursor`]) where words may span chunk boundaries.
//! 2. A small zero-copy HTTP/1 parser for a single buffer, built on the flat
//!    cursor methods.
//! 3. The same request delivered across three chunks, parsed with
//!    [`ChunkedCursor`] — fields may straddle chunk boundaries, and the
//!    `Pieces` value ops (`==`, `starts_with`, `Hash`, `parse_integer`,
//!    `byte_len`, `Display`) are used directly on the pieces: fields are
//!    validated and printed with no concatenation at all.
//! 4. Set-scanning: `take_until_any`, `skip_while_any`, `trim` and
//!    `split_any` replace `|&b| b == ... || b == ...` predicates with plain
//!    byte sets — and show what happens to the spaces in "a, b, c".
//!
//! Run with: `cargo run --example demo`

use aufhebung::{ByteSliceCursor, ChunkedCursor};

/// Render a byte slice for display.
fn as_str(s: &[u8]) -> String {
    String::from_utf8_lossy(s).into_owned()
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
        // trim OWS (SP, HTAB) off both ends of the value
        let value = field.trim(b" \t");
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
        let word = pieces;
        for piece in word {
            println!("  word {n}: piece {:?}", str::from_utf8(piece).unwrap());
        }
        // `Display` prints the whole word straight from the pieces — no
        // `Vec`/`String` needed (the summary line that used to force a
        // collect now prints the span directly).
        println!("word {n}: {pieces}");
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

    // Request line: METHOD SP target SP HTTP-version CRLF. Each field stops at
    // the first byte of a set: spaces and line ends for method/target, line
    // ends alone for version.
    let method = c.take_until_any(b" \r\n");
    c.next_byte(); // SP
    let target = c.take_until_any(b" \r\n");
    c.next_byte(); // SP
    let version = c.take_until_any(b"\r\n");
    c.next_byte(); // CR
    c.next_byte(); // LF

    println!("request line: {method} {target} {version}");
    println!(
        "  method  \"{method}\" (matches b\"POST\")  -> {}",
        method == b"POST"
    );
    println!(
        "  version \"{version}\" (starts with b\"HTTP/\") -> {}",
        version.starts_with(b"HTTP/")
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
                let name = c.take_until_byte(b':');
                c.next_byte(); // ':'
                c.skip_while_any(b" \t"); // leading OWS
                let value = c.take_until_any(b"\r\n");
                c.next_byte(); // CR
                c.next_byte(); // LF
                if name == b"Content-Length" {
                    declared_length = value.parse_integer();
                }
                println!("    {name}: {value}");
            }
        }
    }

    let body = c.take_rest();
    println!("  body: \"{body}\" ({} bytes, zero-copy)", body.byte_len());
    println!("  Content-Length parsed straight from the pieces: {declared_length:?}");
    println!(
        "  Content-Length matches actual body length: {}",
        declared_length == Some(body.byte_len() as i64)
    );

    println!();
    println!("=== set-scanning: what happens to the spaces in \"a, b, c\"? ===");

    let list: &[u8] = b"a, b, c";
    println!(
        "  split_any(b\",\") segments: {:?}",
        list.split_any(b",").map(as_str).collect::<Vec<_>>()
    );
    println!(
        "  then trim(b\" \\t\") each:      {:?}",
        list.split_any(b",")
            .map(|s| as_str(s.trim(b" \t")))
            .collect::<Vec<_>>()
    );
}
