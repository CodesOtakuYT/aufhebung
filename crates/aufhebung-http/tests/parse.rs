//! Integration tests for the HTTP/1.1 request parser.

use aufhebung_http::{Error, Request};

const REQUEST: &[u8] = b"POST /submit HTTP/1.1\r\n\
    Host: example.com\r\n\
    Content-Type: text/plain\r\n\
    Content-Length: 5\r\n\
    \r\n\
    hello";

/// Structural equality: same bytes everywhere, regardless of how the input was
/// chunked.
fn same(a: &Request<'_>, b: &Request<'_>) -> bool {
    a.method() == b.method()
        && a.target() == b.target()
        && a.version() == b.version()
        && a.body() == b.body()
        && a.headers().len() == b.headers().len()
        && a.headers().iter().zip(b.headers()).all(|(x, y)| x == y)
}

#[test]
fn parses_a_flat_request() {
    let chunks = [REQUEST];
    let req = Request::parse(&chunks).unwrap();
    assert!(req.method() == b"POST");
    assert!(req.target() == b"/submit");
    assert!(req.version() == b"HTTP/1.1");
    assert_eq!(req.headers().len(), 3);
    assert!(req.body() == b"hello");
    assert_eq!(req.content_length(), Some(5));
}

#[test]
fn header_lookup_is_case_insensitive() {
    let chunks = [REQUEST];
    let req = Request::parse(&chunks).unwrap();
    assert_eq!(req.header(b"HOST").map(|v| v == b"example.com"), Some(true));
    assert_eq!(req.header(b"content-length").map(|v| v == b"5"), Some(true));
    assert!(req.header(b"no-such-field").is_none());
}

#[test]
fn header_values_are_ows_trimmed() {
    let wire: &[u8] = b"GET /x HTTP/1.1\r\nHost:\texample.com   \r\nX-Empty:  \r\n\r\n";
    let chunks = [wire];
    let req = Request::parse(&chunks).unwrap();
    assert_eq!(req.header(b"host").map(|v| v == b"example.com"), Some(true));
    assert_eq!(req.header(b"X-Empty").map(|v| v.byte_len()), Some(0));
}

#[test]
fn duplicate_headers_keep_order_and_first_wins() {
    let wire: &[u8] = b"GET /x HTTP/1.1\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\n\r\n";
    let chunks = [wire];
    let req = Request::parse(&chunks).unwrap();
    assert_eq!(req.headers().len(), 2);
    assert_eq!(req.header(b"set-cookie").map(|v| v == b"a=1"), Some(true));
}

#[test]
fn bare_lf_line_endings_are_accepted() {
    let wire: &[u8] = b"GET /x HTTP/1.1\nHost: y\n\nbody";
    let chunks = [wire];
    let req = Request::parse(&chunks).unwrap();
    assert!(req.target() == b"/x");
    assert!(req.body() == b"body");
}

#[test]
fn request_without_body_or_headers() {
    let wire: &[u8] = b"GET /x HTTP/1.1\r\n\r\n";
    let chunks = [wire];
    let req = Request::parse(&chunks).unwrap();
    assert_eq!(req.headers().len(), 0);
    assert_eq!(req.body().byte_len(), 0);
    assert_eq!(req.content_length(), None);
}

#[test]
fn chunked_parse_matches_for_every_split_point() {
    let flat = [REQUEST];
    let baseline = Request::parse(&flat).unwrap();
    for cut in 1..REQUEST.len() {
        let chunks = [&REQUEST[..cut], &REQUEST[cut..]];
        let req = Request::parse(&chunks).unwrap();
        assert!(same(&baseline, &req), "split at byte {cut} diverged");
    }
}

#[test]
fn fields_may_straddle_chunk_boundaries() {
    let chunks: [&[u8]; 4] = [
        b"POST /submit HTT",
        b"P/1.1\r\nHost: examp",
        b"le.com\r\nContent-Type: text/plai",
        b"n\r\nContent-Length: 5\r\n\r\nhello",
    ];
    let req = Request::parse(&chunks).unwrap();
    let flat = [REQUEST];
    assert!(same(&Request::parse(&flat).unwrap(), &req));
    assert!(req.version() == b"HTTP/1.1"); // straddles chunks 1–2
    assert_eq!(req.header(b"host").map(|v| v == b"example.com"), Some(true)); // 2–3
    assert_eq!(
        req.header(b"content-type").map(|v| v == b"text/plain"),
        Some(true) // value straddles 3–4
    );
    assert_eq!(req.content_length(), Some(5));
}

#[test]
fn truncated_requests_report_incomplete() {
    let wires: [&[u8]; 7] = [
        b"",
        b"GET ",
        b"GET /x HT",
        b"GET /x HTTP/1.1\r",
        b"GET /x HTTP/1.1\r\nHost: example.com",
        b"GET /x HTTP/1.1\r\nHost: example.com\r\nX",
        b"GET /x HTTP/1.1\r\nHost: example.com\r\n\r",
    ];
    for wire in wires {
        let chunks = [wire];
        assert!(
            matches!(Request::parse(&chunks), Err(Error::Incomplete)),
            "{wire:?} should be Incomplete"
        );
    }
}

#[test]
fn fragmented_header_without_colon_distinguishes_incomplete_from_malformed() {
    let incomplete: [&[u8]; 2] = [b"GET /x HTTP/1.1\r\nHo", b"st"];
    assert!(matches!(
        Request::parse(&incomplete),
        Err(Error::Incomplete)
    ));

    let malformed: [&[u8]; 3] = [b"GET /x HTTP/1.1\r\nHo", b"st\r", b"\n\r\n"];
    assert!(matches!(Request::parse(&malformed), Err(Error::Malformed)));
}

#[test]
fn malformed_requests_report_malformed() {
    let wires: [&[u8]; 7] = [
        b"\r\n\r\n",                                // empty request line
        b" /x HTTP/1.1\r\n\r\n",                    // empty method
        b"GET  /x HTTP/1.1\r\n\r\n",                // double space
        b"GET /x custom/1\r\n\r\n",                 // version not HTTP/x
        b"GET /x HTTP/1.1\r\n: v\r\n\r\n",          // empty header name
        b"GET /x HTTP/1.1\r\nHost\r\n\r\n",         // header without ':'
        b"GET /x HTTP/1.1\r\nHost: v\rBAD\r\n\r\n", // bad line end
    ];
    for wire in wires {
        let chunks = [wire];
        assert!(
            matches!(Request::parse(&chunks), Err(Error::Malformed)),
            "{wire:?} should be Malformed"
        );
    }
}
