//! Regression tests for multi-byte searches over fragmented byte streams.

use aufhebung_core::ChunkedCursor;

#[test]
fn takes_a_pattern_without_itself() {
    let chunks: &[&[u8]] = &[b"", b"ab", b"cd", b"ef", b"tail"];
    let mut cursor = ChunkedCursor::new(chunks);
    let prefix = cursor.take_until_bytes(b"de");
    assert!(prefix == b"abc");
    assert!(cursor.take_rest() == b"deftail");
}

#[test]
fn handles_overlapping_candidates_and_empty_chunks() {
    let chunks: &[&[u8]] = &[b"", b"a-", b"", b"--", b">b", b"", b"-x"];
    let mut cursor = ChunkedCursor::new(chunks);
    let prefix = cursor.take_until_bytes(b"-->");
    assert!(prefix == b"a-");
    assert!(cursor.take_rest() == b"-->b-x");
}

#[test]
fn consumes_to_end_when_the_pattern_is_absent() {
    let chunks: &[&[u8]] = &[b"one", b"", b"two", b"three"];
    let mut cursor = ChunkedCursor::new(chunks);
    assert!(cursor.take_until_bytes(b"missing") == b"onetwothree");
    assert!(cursor.is_empty());
}
