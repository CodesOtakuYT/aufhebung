//! Integration tests for the chunked cursor: `ChunkedCursor`, with its
//! `Pieces` and `Words` iterators, over a stream of slices (`&[&[T]]`).

use aufhebung::{ByteSliceCursor, ChunkedCursor, SliceCursor};

fn cursor<'a>(chunks: &'a [&'a [u8]]) -> ChunkedCursor<'a, u8> {
    ChunkedCursor::new(chunks)
}

fn words<'a>(chunks: &'a [&'a [u8]]) -> Vec<Vec<&'a [u8]>> {
    cursor(chunks)
        .split_whitespace()
        .map(|w| w.collect())
        .collect()
}

/// Concatenate the pieces of one chunked word back into a single byte vector.
fn concatenate<'a>(pieces: impl Iterator<Item = &'a [u8]>) -> Vec<u8> {
    pieces.flat_map(|p| p.iter().copied()).collect()
}

/// The whole remaining stream as a [`Pieces`] span (no `\0` present).
fn pieces<'a>(chunks: &'a [&'a [u8]]) -> aufhebung::Pieces<'a, u8> {
    cursor(chunks).take_until_byte(b'\0')
}

#[test]
fn split_whitespace_spans_chunks() {
    let chunks: &[&[u8]] = &[b"he", b"llo wo", b"rld par", b"t   two"];
    assert_eq!(
        words(chunks),
        vec![
            vec![&b"he"[..], &b"llo"[..]],
            vec![&b"wo"[..], &b"rld"[..]],
            vec![&b"par"[..], &b"t"[..]],
            vec![&b"two"[..]],
        ]
    );
}

#[test]
fn word_filling_a_chunk_exactly_continues() {
    let chunks: &[&[u8]] = &[b"ab", b"cd"];
    assert_eq!(words(chunks), vec![vec![&b"ab"[..], &b"cd"[..]]]);
}

#[test]
fn whitespace_run_spanning_a_boundary() {
    let chunks: &[&[u8]] = &[b"a ", b"  b"];
    assert_eq!(words(chunks), vec![vec![&b"a"[..]], vec![&b"b"[..]]]);
}

#[test]
fn whole_chunk_word_next_to_whitespace_chunk() {
    let chunks: &[&[u8]] = &[b"ab", b" ", b"cd"];
    assert_eq!(words(chunks), vec![vec![&b"ab"[..]], vec![&b"cd"[..]]]);
}

#[test]
fn empty_chunks_inside_a_word_are_skipped() {
    let chunks: &[&[u8]] = &[b"ab", b"", b"cd"];
    assert_eq!(words(chunks), vec![vec![&b"ab"[..], &b"cd"[..]]]);
}

#[test]
fn empty_stream_has_no_words() {
    assert_eq!(words(&[]), Vec::<Vec<&[u8]>>::new());
    let chunks: &[&[u8]] = &[b"", b"", b""];
    assert_eq!(words(chunks), Vec::<Vec<&[u8]>>::new());
}

#[test]
fn leading_trailing_whitespace_and_edge_chunks() {
    let chunks: &[&[u8]] = &[b"", b"  hi ", b"", b" yo  ", b""];
    assert_eq!(words(chunks), vec![vec![&b"hi"[..]], vec![&b"yo"[..]]]);
}

#[test]
fn word_boundary_at_the_first_byte_of_a_chunk() {
    let chunks: &[&[u8]] = &[b"ab", b" cd"];
    assert_eq!(words(chunks), vec![vec![&b"ab"[..]], vec![&b"cd"[..]]]);
}

/// Cross-check the chunked split against the flat `split_whitespace` of the
/// same data concatenated into one slice.
#[test]
fn chunked_words_match_flat_concatenation() {
    let cases: [&[&[u8]]; 8] = [
        &[],
        &[b"", b""],
        &[b"ab", b"cd"],
        &[b"a ", b"  b"],
        &[b"he", b"llo wo", b"rld par", b"t   two"],
        &[b"", b"  x", b"", b"y", b"  ", b"z "],
        &[b"a", b"", b"", b"b", b"c"],
        &[b"  ", b"", b"  "],
    ];
    for case in cases {
        let combined: Vec<u8> = case.iter().flat_map(|c| c.iter().copied()).collect();
        let flat: Vec<Vec<u8>> = combined
            .as_slice()
            .split_whitespace()
            .map(|w| w.to_vec())
            .collect();
        let chunked: Vec<Vec<u8>> = cursor(case).split_whitespace().map(concatenate).collect();
        assert_eq!(chunked, flat, "case {case:?}");
    }
}

#[test]
fn split_whitespace_is_a_snapshot() {
    let chunks: &[&[u8]] = &[b"a", b" b"];
    let mut c = cursor(chunks);
    c.next_byte(); // consume 'a'
    let words: Vec<Vec<u8>> = c.split_whitespace().map(concatenate).collect();
    assert_eq!(words, [b"b".to_vec()]);
    // the cursor itself was not consumed by the split:
    assert_eq!(c.peek_byte(), Some(b' '));
}

#[test]
fn words_is_fused() {
    let chunks: &[&[u8]] = &[b"ab", b" cd"];
    let mut it = cursor(chunks).split_whitespace();
    let first: Vec<&[u8]> = it.next().unwrap().collect();
    assert_eq!(first, [&b"ab"[..]]);
    let second: Vec<&[u8]> = it.next().unwrap().collect();
    assert_eq!(second, [&b"cd"[..]]);
    assert!(it.next().is_none());
    assert!(it.next().is_none());
}

#[test]
fn pieces_is_exact_size_and_fused() {
    let chunks: &[&[u8]] = &[b"he", b"llo wo", b"rld par", b"t   two"];
    for word in cursor(chunks).split_whitespace() {
        let n = word.len();
        assert_eq!(word.count(), n);
        assert_eq!(word.size_hint(), (n, Some(n)));
        let mut it = word;
        while it.next().is_some() {}
        assert_eq!(it.len(), 0);
        assert_eq!(it.next(), None);
    }
}

#[test]
fn take_until_bridges_chunks_and_leaves_cursor_at_match() {
    let chunks: &[&[u8]] = &[b"ab", b"c de", b"f"];
    let mut c = cursor(chunks);
    let pieces: Vec<&[u8]> = c.take_until(u8::is_ascii_whitespace).collect();
    assert_eq!(pieces, [&b"ab"[..], &b"c"[..]]);
    assert_eq!(c.peek_byte(), Some(b' '));
    c.skip_while(u8::is_ascii_whitespace);
    let rest: Vec<&[u8]> = c.take_until(u8::is_ascii_whitespace).collect();
    assert_eq!(rest, [&b"de"[..], &b"f"[..]]);
    assert!(c.is_empty());
}

#[test]
fn take_until_with_immediate_match_yields_no_pieces() {
    let chunks: &[&[u8]] = &[b"  a", b"b"];
    let mut c = cursor(chunks);
    let pieces: Vec<&[u8]> = c.take_until(u8::is_ascii_whitespace).collect();
    assert!(pieces.is_empty());
    assert_eq!(c.peek_byte(), Some(b' '));
}

#[test]
fn take_until_without_match_consumes_everything() {
    let chunks: &[&[u8]] = &[b"ab", b"", b"cd"];
    let mut c = cursor(chunks);
    let pieces: Vec<&[u8]> = c.take_until(u8::is_ascii_whitespace).collect();
    assert_eq!(pieces, [&b"ab"[..], &b"cd"[..]]);
    assert!(c.is_empty());
}

#[test]
fn skip_while_counts_across_chunks() {
    let chunks: &[&[u8]] = &[b" \t", b" ", b"x"];
    let mut c = cursor(chunks);
    assert_eq!(c.skip_while(u8::is_ascii_whitespace), 3);
    assert_eq!(c.peek_byte(), Some(b'x'));

    let chunks: &[&[u8]] = &[b" ", b"\t", b" "];
    let mut c = cursor(chunks);
    assert_eq!(c.skip_while(u8::is_ascii_whitespace), 3);
    assert!(c.is_empty());
}

#[test]
fn skip_until_counts_across_chunks() {
    let chunks: &[&[u8]] = &[b"ab", b"c de"];
    let mut c = cursor(chunks);
    assert_eq!(c.skip_until(u8::is_ascii_whitespace), 3);
    assert_eq!(c.peek_byte(), Some(b' '));
}

#[test]
fn next_byte_bridges_chunk_boundaries() {
    let chunks: &[&[u8]] = &[b"ab", b"", b"cd"];
    let mut c = cursor(chunks);
    assert_eq!(c.peek_byte(), Some(b'a'));
    let mut out = Vec::new();
    while let Some(b) = c.next_byte() {
        out.push(b);
    }
    assert_eq!(out, b"abcd");
    assert!(c.is_empty());
    assert_eq!(c.next_byte(), None);
}

#[test]
fn peek_skips_exhausted_and_empty_chunks() {
    let chunks: &[&[u8]] = &[b"", b"x", b""];
    let c = cursor(chunks);
    assert_eq!(c.peek(), Some(&b'x'));
    assert_eq!(c.peek_byte(), Some(b'x'));
    assert_eq!(c.remaining(), 1);
}

#[test]
fn remaining_sums_across_chunks() {
    let chunks: &[&[u8]] = &[b"ab", b"", b"cde"];
    let mut c = cursor(chunks);
    assert_eq!(c.remaining(), 5);
    c.next_byte();
    assert_eq!(c.remaining(), 4);
    assert_eq!(cursor(&[]).remaining(), 0);
}

#[test]
fn is_empty_for_empty_streams() {
    assert!(cursor(&[]).is_empty());
    let chunks: &[&[u8]] = &[b"", b""];
    assert!(cursor(chunks).is_empty());
    let chunks: &[&[u8]] = &[b"", b"a"];
    assert!(!cursor(chunks).is_empty());
}

#[test]
fn take_until_byte_uses_memchr_across_chunks() {
    let chunks: &[&[u8]] = &[b"he", b"llo wo", b"rld"];
    let mut c = cursor(chunks);
    let first: Vec<&[u8]> = c.take_until_byte(b' ').collect();
    assert_eq!(first, [&b"he"[..], &b"llo"[..]]);
    assert_eq!(c.peek_byte(), Some(b' '));
    c.next_byte(); // eat the space
    let second: Vec<&[u8]> = c.take_until_byte(b' ').collect();
    assert_eq!(second, [&b"wo"[..], &b"rld"[..]]);
    assert!(c.is_empty());
}

#[test]
fn skip_until_byte_bridges_chunks() {
    let chunks: &[&[u8]] = &[b"ab", b"c d"];
    let mut c = cursor(chunks);
    assert_eq!(c.skip_until_byte(b' '), 3);
    assert_eq!(c.peek_byte(), Some(b' '));
}

/// `take_until` on a mis-aligned cursor (leading empty/exhausted chunks) must
/// normalize the start: no empty leading piece, and the pieces must match the
/// flat `take_until` of the same data concatenated into one slice.
#[test]
fn take_until_normalizes_start() {
    let cases: [&[&[u8]]; 6] = [
        &[],
        &[b""],
        &[b"", b"hello"],
        &[b"hello", b""],
        &[b"", b"hello", b""],
        &[b"", b"", b"hello"],
    ];
    for case in cases {
        let combined: Vec<u8> = case.iter().flat_map(|c| c.iter().copied()).collect();
        let mut flat: &[u8] = &combined;
        let flat_taken = flat.take_until(|&b| b == b'o');

        let mut c = cursor(case);
        let pieces: Vec<&[u8]> = c.take_until(|&b| b == b'o').collect();

        assert!(
            pieces.iter().all(|p| !p.is_empty()),
            "case {case:?}: empty piece yielded"
        );
        assert_eq!(concatenate(pieces.into_iter()), flat_taken, "case {case:?}");
        assert_eq!(c.peek_byte(), flat.peek_byte(), "case {case:?}");
    }
}

/// Same as [`take_until_normalizes_start`], through the memchr-accelerated
/// `take_until_byte`.
#[test]
fn take_until_byte_normalizes_start() {
    let cases: [&[&[u8]]; 6] = [
        &[],
        &[b""],
        &[b"", b"hello"],
        &[b"hello", b""],
        &[b"", b"hello", b""],
        &[b"", b"", b"hello"],
    ];
    for case in cases {
        let combined: Vec<u8> = case.iter().flat_map(|c| c.iter().copied()).collect();
        let mut flat: &[u8] = &combined;
        let flat_taken = flat.take_until_byte(b'o');

        let mut c = cursor(case);
        let pieces: Vec<&[u8]> = c.take_until_byte(b'o').collect();

        assert!(
            pieces.iter().all(|p| !p.is_empty()),
            "case {case:?}: empty piece yielded"
        );
        assert_eq!(concatenate(pieces.into_iter()), flat_taken, "case {case:?}");
        assert_eq!(c.peek_byte(), flat.peek_byte(), "case {case:?}");
    }
}

/// `split_whitespace` must tolerate leading, trailing, and in-stream empty
/// chunks without producing empty words or empty pieces.
#[test]
fn split_whitespace_with_empty_chunks_everywhere() {
    let chunks: &[&[u8]] = &[b"", b"", b"  hi ", b"", b"yo  ", b""];
    assert_eq!(words(chunks), vec![vec![&b"hi"[..]], vec![&b"yo"[..]]]);
}

#[test]
fn generic_chunks_work_with_any_t() {
    let chunks: &[&[i32]] = &[&[1, 2], &[3], &[4, 5]];
    let mut c = ChunkedCursor::new(chunks);
    assert_eq!(c.peek(), Some(&1));
    let pieces: Vec<&[i32]> = c.take_until(|&x| x % 3 == 0).collect();
    assert_eq!(pieces, [&[1, 2][..]]);
    assert_eq!(c.peek(), Some(&3));
    assert_eq!(c.skip_while(|&x| x < 5), 2);
    assert_eq!(c.peek(), Some(&5));
    assert_eq!(c.remaining(), 1);
}

#[test]
fn pieces_eq_ignores_chunk_layout() {
    let a = pieces(&[b"he", b"llo"]);
    let b = pieces(&[b"h", b"ello"]);

    // split differently but same bytes
    assert!(a == b);
    assert!(b == a);
    assert!(a == a);

    // against byte-string literals
    assert!(a == b"hello");
    assert!(b == b"hello");
    assert!(a != b"hallo");

    // length mismatch
    assert!(a != b"hellox");
    assert!(pieces(&[]) != b"x");

    // empty span == empty slice, regardless of empty chunks
    assert!(pieces(&[]) == b"");
    assert!(pieces(&[b"", b""]) == b"");
    assert!(pieces(&[]) == pieces(&[b"", b""]));
}

#[test]
fn pieces_hash_matches_collecting() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut ha = DefaultHasher::new();
    pieces(&[b"he", b"llo"]).hash(&mut ha);
    let mut hb = DefaultHasher::new();
    pieces(&[b"h", b"ello"]).hash(&mut hb);
    assert_eq!(
        ha.finish(),
        hb.finish(),
        "split layout must not affect the hash"
    );

    // equal to hashing the contiguous bytes
    let mut hc = DefaultHasher::new();
    b"hello".to_vec().hash(&mut hc);
    assert_eq!(ha.finish(), hc.finish());
}

#[test]
fn parse_integer_across_chunks() {
    assert_eq!(pieces(&[b"4", b"2"]).parse_integer(), Some(42));
    assert_eq!(pieces(&[b"-1", b"7"]).parse_integer(), Some(-17));
    assert_eq!(pieces(&[b"+", b"3"]).parse_integer(), Some(3));
    assert_eq!(pieces(&[b"-0"]).parse_integer(), Some(0));
    assert_eq!(
        pieces(&[b"-9223372036854775808"]).parse_integer(),
        Some(i64::MIN)
    );

    assert_eq!(pieces(&[b"-"]).parse_integer(), None);
    assert_eq!(pieces(&[b"+-"]).parse_integer(), None);
    assert_eq!(pieces(&[b""]).parse_integer(), None);
    assert_eq!(pieces(&[b"1", b"2-"]).parse_integer(), None);
    assert_eq!(pieces(&[b" 1"]).parse_integer(), None);
    assert_eq!(pieces(&[b"1 2"]).parse_integer(), None);
    assert_eq!(pieces(&[b"9223372036854775808"]).parse_integer(), None);
    assert_eq!(
        pieces(&[b"99999999999999999999999999"]).parse_integer(),
        None
    );
}
