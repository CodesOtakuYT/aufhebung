//! The add-on HTTP parser, exercised through the `aufhebung` umbrella, which
//! re-exports `aufhebung_http` at its root.

use aufhebung::Request;

const REQUEST: &[u8] = b"POST /submit HTTP/1.1\r\n\
    Host: example.com\r\n\
    Content-Length: 5\r\n\
    \r\n\
    hello";

#[test]
fn umbrella_re_exports_the_http_parser() {
    let chunks = [REQUEST];
    let req = Request::parse(&chunks).unwrap();
    assert!(req.method() == b"POST");
    assert!(req.target() == b"/submit");
    assert_eq!(req.content_length(), Some(5));
}
