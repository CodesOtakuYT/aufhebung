//! Integration tests for the aufhebung cursor API.

use aufhebung::{ByteSliceCursor, SliceCursor};

// ---- SliceCursor: generic scanning ----

#[test]
fn take_basic() {
    let mut s: &[u8] = b"hello";
    assert_eq!(s.take(2), b"he");
    assert_eq!(s, b"llo");
}

#[test]
fn take_clamps_to_len() {
    let mut s: &[u8] = b"ab";
    assert_eq!(s.take(5), b"ab");
    assert!(s.is_empty());
}

#[test]
fn take_zero_is_noop() {
    let mut s: &[u8] = b"ab";
    assert_eq!(s.take(0), b"");
    assert_eq!(s, b"ab");
}

#[test]
fn take_on_empty() {
    let mut s: &[u8] = b"";
    assert_eq!(s.take(3), b"");
    assert!(s.is_empty());
}

#[test]
fn take_while_basic() {
    let mut s: &[u8] = b"abc def";
    assert_eq!(s.take_while(|&b| b != b' '), b"abc");
    assert_eq!(s, b" def");
}

#[test]
fn take_while_all_match() {
    let mut s: &[u8] = b"abc";
    assert_eq!(s.take_while(|&b| b.is_ascii_alphabetic()), b"abc");
    assert!(s.is_empty());
}

#[test]
fn take_while_no_match() {
    let mut s: &[u8] = b"  !";
    assert_eq!(s.take_while(|&b| b.is_ascii_alphabetic()), b"");
    assert_eq!(s, b"  !");
}

#[test]
fn take_while_fnmut_state() {
    let mut s: &[u8] = b"aaaab";
    let mut seen = 0;
    let taken = s.take_while(|&b| {
        if b == b'a' {
            seen += 1;
            true
        } else {
            false
        }
    });
    assert_eq!(taken, b"aaaa");
    assert_eq!(seen, 4);
    assert_eq!(s, b"b");
}

#[test]
fn take_while_incl_keeps_first_bad() {
    let mut s: &[u8] = b"abc!...";
    assert_eq!(s.take_while_incl(|&b| b != b'!'), b"abc!");
    assert_eq!(s, b"...");
}

#[test]
fn take_while_incl_all_match() {
    let mut s: &[u8] = b"abc";
    assert_eq!(s.take_while_incl(|&b| b != b'!'), b"abc");
    assert!(s.is_empty());
}

#[test]
fn take_until_basic() {
    let mut s: &[u8] = b"GET /x";
    assert_eq!(s.take_until(|&b| b == b' '), b"GET");
    assert_eq!(s, b" /x");
}

#[test]
fn take_until_no_match_consumes_all() {
    let mut s: &[u8] = b"abc";
    assert_eq!(s.take_until(|&b| b == b' '), b"abc");
    assert!(s.is_empty());
}

#[test]
fn take_until_matches_first_byte() {
    let mut s: &[u8] = b",x";
    assert_eq!(s.take_until(|&b| b == b','), b"");
    assert_eq!(s, b",x");
}

#[test]
fn take_until_incl_keeps_terminator() {
    let mut s: &[u8] = b"a,b";
    assert_eq!(s.take_until_incl(|&b| b == b','), b"a,");
    assert_eq!(s, b"b");
}

#[test]
fn take_until_incl_no_match_consumes_all() {
    let mut s: &[u8] = b"ab";
    assert_eq!(s.take_until_incl(|&b| b == b','), b"ab");
    assert!(s.is_empty());
}

#[test]
fn advance_basic_and_clamp() {
    let mut s: &[u8] = b"abcdef";
    s.advance(3);
    assert_eq!(s, b"def");
    s.advance(99);
    assert!(s.is_empty());
    s.advance(5);
    assert!(s.is_empty());
}

#[test]
fn skip_while_returns_count() {
    let mut s: &[u8] = b"   ab";
    assert_eq!(s.skip_while(|&b| b == b' '), 3);
    assert_eq!(s, b"ab");
}

#[test]
fn skip_until_returns_count() {
    let mut s: &[u8] = b"abc,def";
    assert_eq!(s.skip_until(|&b| b == b','), 3);
    assert_eq!(s, b",def");
}

#[test]
fn skip_until_incl_returns_count() {
    let mut s: &[u8] = b"abc,def";
    assert_eq!(s.skip_until_incl(|&b| b == b','), 4);
    assert_eq!(s, b"def");
}

#[test]
fn peek_does_not_advance() {
    let s: &[u8] = b"abcdef";
    assert_eq!(s.peek(3), b"abc");
    assert_eq!(s.peek(0), b"");
    assert_eq!(s.peek(99), b"abcdef");
    assert_eq!(s, b"abcdef");
}

#[test]
fn peek_while_does_not_advance() {
    let s: &[u8] = b"abc def";
    assert_eq!(s.peek_while(|&b| b != b' '), b"abc");
    assert_eq!(s, b"abc def");
}

#[test]
fn peek_until_does_not_advance() {
    let s: &[u8] = b"abc,def";
    assert_eq!(s.peek_until(|&b| b == b','), b"abc");
    assert_eq!(s, b"abc,def");
}

#[test]
fn find_relative_to_cursor() {
    let mut s: &[u8] = b"a,b,c";
    assert_eq!(s.find(|&b| b == b','), Some(1));
    s.advance(2);
    assert_eq!(s.find(|&b| b == b','), Some(1));
    assert_eq!(s.find(|&b| b == b'z'), None);
}

#[test]
fn find_tag_basic() {
    let s: &[u8] = b"xxHTTPyy";
    assert_eq!(s.find_tag(b"HTTP"), Some(2));
    assert_eq!(s.find_tag(b"zzz"), None);
    assert_eq!(s.find_tag(b""), Some(0));
}

#[test]
fn take_until_tag_leaves_cursor_at_tag() {
    let mut s: &[u8] = b"GET / HTTP/1.1";
    assert_eq!(s.take_until_tag(b" HTTP"), b"GET /");
    assert_eq!(s, b" HTTP/1.1");
}

#[test]
fn take_until_tag_missing_consumes_all() {
    let mut s: &[u8] = b"abc";
    assert_eq!(s.take_until_tag(b"zzz"), b"abc");
    assert!(s.is_empty());
}

#[test]
fn take_until_tag_empty_tag() {
    let mut s: &[u8] = b"abc";
    assert_eq!(s.take_until_tag(b""), b"");
    assert_eq!(s, b"abc");
}

#[test]
fn take_prefix_ok() {
    let mut s: &[u8] = b"prefix-rest";
    assert_eq!(s.take_prefix(b"prefix-"), Some(&b"prefix-"[..]));
    assert_eq!(s, b"rest");
}

#[test]
fn take_prefix_miss_leaves_input() {
    let mut s: &[u8] = b"nope";
    assert_eq!(s.take_prefix(b"yes"), None);
    assert_eq!(s, b"nope");
}

#[test]
fn take_prefix_empty_prefix() {
    let mut s: &[u8] = b"abc";
    assert_eq!(s.take_prefix(b""), Some(&b""[..]));
    assert_eq!(s, b"abc");
}

#[test]
fn skip_tag_ok() {
    let mut s: &[u8] = b"ab-cd";
    assert!(s.skip_tag(b"ab-"));
    assert_eq!(s, b"cd");
}

#[test]
fn skip_tag_miss() {
    let mut s: &[u8] = b"ab-cd";
    assert!(!s.skip_tag(b"xy"));
    assert_eq!(s, b"ab-cd");
}

#[test]
fn starts_with_ok() {
    let s: &[u8] = b"abcdef";
    assert!(s.starts_with(b"abc"));
    assert!(!s.starts_with(b"abd"));
    assert!(s.starts_with(b""));
    assert!(!s.starts_with(b"abcdefg"));
}

#[test]
fn generic_element_type() {
    let mut s: &[i32] = &[1, 2, 3, -4, 5];
    assert_eq!(s.take_until(|&x| x < 0), &[1, 2, 3]);
    assert_eq!(s, &[-4, 5]);
    assert_eq!(s.take(1), &[-4]);
    assert_eq!(s, &[5]);
    assert_eq!(s.take_prefix(&[5]), Some(&[5][..]));
}

// ---- ByteSliceCursor: memchr-backed ----

#[test]
fn take_until_byte() {
    let mut s: &[u8] = b"a;b";
    assert_eq!(s.take_until_byte(b';'), b"a");
    assert_eq!(s, b";b");
}

#[test]
fn take_until_byte_incl() {
    let mut s: &[u8] = b"a;b";
    assert_eq!(s.take_until_byte_incl(b';'), b"a;");
    assert_eq!(s, b"b");
}

#[test]
fn take_until_byte_missing_consumes_all() {
    let mut s: &[u8] = b"abc";
    assert_eq!(s.take_until_byte(b'z'), b"abc");
    assert!(s.is_empty());
}

#[test]
fn take_until_bytes() {
    let mut s: &[u8] = b"GET /x";
    assert_eq!(s.take_until_bytes(b" /"), b"GET");
    assert_eq!(s, b" /x");
}

#[test]
fn skip_until_byte() {
    let mut s: &[u8] = b"abc;def";
    assert_eq!(s.skip_until_byte(b';'), 3);
    assert_eq!(s, b";def");
}

#[test]
fn skip_until_byte_incl() {
    let mut s: &[u8] = b"abc;def";
    assert_eq!(s.skip_until_byte_incl(b';'), 4);
    assert_eq!(s, b"def");
}

#[test]
fn skip_until_bytes_incl() {
    let mut s: &[u8] = b"a\r\nb\r\nc";
    assert_eq!(s.skip_until_bytes_incl(b"\r\n"), 3);
    assert_eq!(s, b"b\r\nc");
}

#[test]
fn find_and_rfind_byte() {
    let s: &[u8] = b"a,b,a";
    assert_eq!(s.find_byte(b','), Some(1));
    assert_eq!(s.rfind_byte(b','), Some(3));
    assert_eq!(s.find_byte(b'z'), None);
}

#[test]
fn find_and_rfind_bytes() {
    let s: &[u8] = b"a,bb,c";
    assert_eq!(s.find_bytes(b",b"), Some(1));
    assert_eq!(s.rfind_bytes(b",b"), Some(1));
    assert_eq!(s.find_bytes(b"zz"), None);
}

#[test]
fn next_byte_streams() {
    let mut s: &[u8] = b"ab";
    assert_eq!(s.next_byte(), Some(b'a'));
    assert_eq!(s.next_byte(), Some(b'b'));
    assert_eq!(s.next_byte(), None);
    assert!(s.is_empty());
}

#[test]
fn peek_byte_does_not_advance() {
    let s: &[u8] = b"ab";
    assert_eq!(s.peek_byte(), Some(b'a'));
    assert_eq!(s, b"ab");
    let e: &[u8] = b"";
    assert_eq!(e.peek_byte(), None);
}

#[test]
fn skip_byte_ok_and_miss() {
    let mut s: &[u8] = b"ab";
    assert!(s.skip_byte(b'a'));
    assert_eq!(s, b"b");
    assert!(!s.skip_byte(b'a'));
    assert_eq!(s, b"b");
}

#[test]
fn original_demo_regression() {
    let mut x: &[u8] = b"hello world!";
    let whitespace = x.take_until(u8::is_ascii_whitespace);
    assert_eq!(str::from_utf8(whitespace).unwrap(), "hello");
    assert_eq!(x, b" world!");
}

#[test]
fn split_into_words() {
    let mut s: &[u8] = b" the quick brown fox ";
    let mut words: Vec<&[u8]> = Vec::new();
    loop {
        s.skip_while(|&b| b == b' ');
        if s.remaining() == 0 {
            break;
        }
        words.push(s.take_until(|&b| b == b' '));
    }
    assert_eq!(
        words,
        vec![&b"the"[..], &b"quick"[..], &b"brown"[..], &b"fox"[..]]
    );
}

// ---- Segmenting iterators ----

#[test]
fn split_on_mirrors_slice_split() {
    let s: &[u8] = b"a,b,,c";
    let parts: Vec<&[u8]> = s.split_on(|&b| b == b',').collect();
    assert_eq!(parts, vec![&b"a"[..], &b"b"[..], &b""[..], &b"c"[..]]);
}

#[test]
fn split_on_leading_and_trailing_separators() {
    let s: &[u8] = b",a,";
    let parts: Vec<&[u8]> = s.split_on(|&b| b == b',').collect();
    // leading separator -> empty first segment; trailing -> no empty tail
    assert_eq!(parts, vec![&b""[..], &b"a"[..]]);
}

#[test]
fn split_on_no_separator_yields_whole() {
    let s: &[u8] = b"abc";
    let parts: Vec<&[u8]> = s.split_on(|&b| b == b',').collect();
    assert_eq!(parts, vec![&b"abc"[..]]);
}

#[test]
fn split_on_non_byte_type() {
    let s: &[i32] = &[0, 1, 2, 0, 3];
    let parts: Vec<&[i32]> = s.split_on(|&x| x == 0).collect();
    assert_eq!(parts, vec![&[][..], &[1, 2][..], &[3][..]]);
}

#[test]
fn split_on_does_not_advance_cursor() {
    let s: &[u8] = b"a,b";
    let parts: Vec<&[u8]> = s.split_on(|&b| b == b',').collect();
    assert_eq!(parts, vec![&b"a"[..], &b"b"[..]]);
    assert_eq!(s, b"a,b");
}

// ---- split-family contract: split_terminator semantics ----

#[test]
fn split_on_empty_input_yields_nothing() {
    let s: &[u8] = b"";
    assert_eq!(s.split_on(|&b| b == b',').count(), 0);
}

#[test]
fn split_bytes_terminator_semantics() {
    // trailing separator: the final empty segment is omitted (split_terminator,
    // not split)
    let s: &[u8] = b"a,b,";
    let parts: Vec<&[u8]> = s.split_bytes(b",").collect();
    assert_eq!(parts, vec![&b"a"[..], &b"b"[..]]);

    // leading separator still yields a leading empty segment
    let s: &[u8] = b",a";
    let parts: Vec<&[u8]> = s.split_bytes(b",").collect();
    assert_eq!(parts, vec![&b""[..], &b"a"[..]]);

    // interior empty segments are preserved
    let s: &[u8] = b"a,,b";
    let parts: Vec<&[u8]> = s.split_bytes(b",").collect();
    assert_eq!(parts, vec![&b"a"[..], &b""[..], &b"b"[..]]);

    // empty input yields no segments
    let s: &[u8] = b"";
    assert_eq!(s.split_bytes(b",").count(), 0);
}

#[test]
fn split_any_terminator_semantics() {
    // trailing separator omits the final empty segment (spaces kept in the raw
    // segments)
    let s: &[u8] = b"a, b,";
    let parts: Vec<&[u8]> = s.split_any(b",").collect();
    assert_eq!(parts, vec![&b"a"[..], &b" b"[..]]);

    // empty input yields no segments
    let s: &[u8] = b"";
    assert_eq!(s.split_any(b",").count(), 0);
}

#[test]
fn split_whitespace_words() {
    let s: &[u8] = b"  hello \t world!\n";
    let parts: Vec<&[u8]> = s.split_whitespace().collect();
    assert_eq!(parts, vec![&b"hello"[..], &b"world!"[..]]);
}

#[test]
fn split_whitespace_whitespace_only() {
    let s: &[u8] = b"\t \n  ";
    assert_eq!(s.split_whitespace().count(), 0);
}

#[test]
fn split_whitespace_empty() {
    let s: &[u8] = b"";
    assert_eq!(s.split_whitespace().count(), 0);
}

#[test]
fn split_bytes_multi_byte_pattern() {
    let s: &[u8] = b"a\r\nb\r\nc";
    let parts: Vec<&[u8]> = s.split_bytes(b"\r\n").collect();
    assert_eq!(parts, vec![&b"a"[..], &b"b"[..], &b"c"[..]]);
}

#[test]
fn split_bytes_consecutive_patterns() {
    let s: &[u8] = b"a,,b";
    let parts: Vec<&[u8]> = s.split_bytes(b",").collect();
    assert_eq!(parts, vec![&b"a"[..], &b""[..], &b"b"[..]]);
}

#[test]
fn split_bytes_no_match_yields_whole() {
    let s: &[u8] = b"abc";
    let parts: Vec<&[u8]> = s.split_bytes(b"zz").collect();
    assert_eq!(parts, vec![&b"abc"[..]]);
}

#[test]
#[should_panic(expected = "split_bytes: empty pattern")]
fn split_bytes_empty_pattern_panics() {
    let s: &[u8] = b"abc";
    let _ = s.split_bytes(b"");
}

#[test]
fn take_rest_consumes_everything() {
    let mut s: &[u8] = b"hello";
    assert_eq!(s.take_rest(), b"hello");
    assert!(s.is_empty());

    // an exhausted cursor yields an empty rest
    assert_eq!(s.take_rest(), b"");

    // empty cursor
    let mut s: &[u8] = b"";
    assert_eq!(s.take_rest(), b"");
    assert!(s.is_empty());
}

// ---- set scanning: take_until_any / skip_while_any / split_any / trim ----

#[test]
fn take_until_any_matches_first_of_set() {
    let mut s: &[u8] = b"GET /x HTTP/1.1";
    assert_eq!(s.take_until_any(b" \t"), b"GET");
    assert_eq!(s, b" /x HTTP/1.1");
    // single-byte set behaves like take_until_byte
    let mut s: &[u8] = b"a,b";
    assert_eq!(s.take_until_any(b","), b"a");
    assert_eq!(s, b",b");
}

#[test]
fn take_until_any_absent_and_empty_set_consume_all() {
    let mut s: &[u8] = b"abc";
    assert_eq!(s.take_until_any(b":;"), b"abc");
    assert!(s.is_empty());

    let mut s: &[u8] = b"abc";
    assert_eq!(s.take_until_any(b""), b"abc");
    assert!(s.is_empty());
}

#[test]
fn take_until_any_incl_includes_match() {
    let mut s: &[u8] = b"a, b";
    assert_eq!(s.take_until_any_incl(b", "), b"a,");
    assert_eq!(s, b" b");

    // no match: consumes everything, nothing extra
    let mut s: &[u8] = b"ab";
    assert_eq!(s.take_until_any_incl(b";"), b"ab");
    assert!(s.is_empty());
}

#[test]
fn take_until_any_large_set_uses_bitmap() {
    // more than three bytes forces the bitmap scan path
    let mut s: &[u8] = b"hello=world";
    assert_eq!(s.take_until_any(b"=;!@#$"), b"hello");
    assert_eq!(s, b"=world");
}

#[test]
fn skip_while_any_skips_leading_set() {
    let mut s: &[u8] = b"  \t foo";
    assert_eq!(s.skip_while_any(b" \t"), 4);
    assert_eq!(s, b"foo");

    // empty set: skips nothing
    let mut s: &[u8] = b"  x";
    assert_eq!(s.skip_while_any(b""), 0);
    assert_eq!(s, b"  x");

    // everything in the set: skips all
    let mut s: &[u8] = b"   ";
    assert_eq!(s.skip_while_any(b" "), 3);
    assert!(s.is_empty());
}

#[test]
fn trim_strips_both_ends_non_consuming() {
    let s: &[u8] = b"  foo \t ";
    assert_eq!(s.trim(b" \t"), b"foo");
    assert_eq!(s, b"  foo \t "); // cursor unchanged

    let s: &[u8] = b"  foo";
    assert_eq!(s.trim(b" \t"), b"foo");
    let s: &[u8] = b"foo  ";
    assert_eq!(s.trim(b" \t"), b"foo");
    let s: &[u8] = b"foo";
    assert_eq!(s.trim(b" \t"), b"foo");
    let s: &[u8] = b"   ";
    assert_eq!(s.trim(b" \t"), b"");
    let s: &[u8] = b"";
    assert_eq!(s.trim(b" \t"), b"");
    let s: &[u8] = b"foo";
    assert_eq!(s.trim(b""), b"foo"); // empty set: no-op
    let s: &[u8] = b"a  b";
    assert_eq!(s.trim(b" "), b"a  b"); // interior bytes are never removed
}

#[test]
fn split_any_segments_expose_spaces() {
    // "a, b, c": the spaces are part of the segments, not the separator
    let s: &[u8] = b"a, b, c";
    let parts: Vec<&[u8]> = s.split_any(b",").collect();
    assert_eq!(parts, vec![&b"a"[..], &b" b"[..], &b" c"[..]]);

    // compose with trim for clean tokens
    let parts: Vec<&[u8]> = s.split_any(b",").map(|seg| seg.trim(b" \t")).collect();
    assert_eq!(parts, vec![&b"a"[..], &b"b"[..], &b"c"[..]]);
}

#[test]
fn split_any_consecutive_trailing_and_absent() {
    // consecutive separators yield empty segments, trailing yields none
    let s: &[u8] = b"a,,b,";
    let parts: Vec<&[u8]> = s.split_any(b",").collect();
    assert_eq!(parts, vec![&b"a"[..], &b""[..], &b"b"[..]]);

    // no separator: one segment
    let s: &[u8] = b"a,b";
    let parts: Vec<&[u8]> = s.split_any(b"\t").collect();
    assert_eq!(parts, vec![&b"a,b"[..]]);

    // empty set: never matches, whole input is one segment
    let s: &[u8] = b"abc";
    let parts: Vec<&[u8]> = s.split_any(b"").collect();
    assert_eq!(parts, vec![&b"abc"[..]]);
}

#[test]
fn split_any_set_sizes() {
    // one byte
    let s: &[u8] = b"a-b-c";
    let parts: Vec<&[u8]> = s.split_any(b"-").collect();
    assert_eq!(parts, vec![&b"a"[..], &b"b"[..], &b"c"[..]]);

    // two bytes (memchr2)
    let s: &[u8] = b"a b\tc";
    let parts: Vec<&[u8]> = s.split_any(b" \t").collect();
    assert_eq!(parts, vec![&b"a"[..], &b"b"[..], &b"c"[..]]);

    // more than three bytes (256-bit bitmap)
    let s: &[u8] = b"a;b|c\td";
    let parts: Vec<&[u8]> = s.split_any(b";|\t:=").collect();
    assert_eq!(parts, vec![&b"a"[..], &b"b"[..], &b"c"[..], &b"d"[..]]);
}
