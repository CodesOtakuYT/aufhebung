//! Zero-copy cursor operations on slices.
//!
//! This crate provides the "parser input cursor" pattern as plain trait
//! methods — no combinator machinery:
//!
//! ```
//! use aufhebung::SliceCursor;
//!
//! let mut input: &[u8] = b"hello world!";
//! let word = input.take_until(u8::is_ascii_whitespace);
//!
//! assert_eq!(word, b"hello");
//! assert_eq!(input, b" world!");
//! ```
//!
//! All methods are zero-copy: `take*` returns sub-slices that borrow from the
//! original input and advances an in-place `&mut &[T]` cursor. Byte-oriented
//! scanning ([`ByteSliceCursor`]) is accelerated with
//! [`memchr`](https://docs.rs/memchr)'s SIMD routines.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use memchr::{memchr, memmem, memrchr};

/// Cursor operations over a generic slice.
///
/// Implemented for `&'a [T]`. Methods consume from the front of the slice,
/// re-borrowing through `&mut` so the cursor advances with each call:
///
/// ```
/// use aufhebung::SliceCursor;
///
/// let mut input: &[u8] = b"GET / HTTP/1.1";
/// let method = input.take_until(|&b| b == b' ');
/// input.advance(1); // skip the space
/// let path = input.take_until(|&b| b == b' ');
///
/// assert_eq!(method, b"GET");
/// assert_eq!(path, b"/");
/// assert_eq!(input, b" HTTP/1.1");
/// ```
pub trait SliceCursor<'a, T> {
    /// Split off the first `n` elements and advance the cursor.
    ///
    /// Clamps to the available length: `take(n)` never panics and never
    /// consumes more than `remaining()` elements.
    fn take(&mut self, n: usize) -> &'a [T];

    /// Split off the longest prefix of elements for which `pred` returns
    /// `true`, and advance the cursor.
    ///
    /// The cursor ends just *before* the first element that failed `pred`
    /// (or at the end of the slice if no element failed).
    fn take_while<F>(&mut self, pred: F) -> &'a [T]
    where
        F: FnMut(&T) -> bool;

    /// Like [`take_while`](Self::take_while), but the taken slice also
    /// includes the first element that failed the predicate, if any.
    fn take_while_incl<F>(&mut self, pred: F) -> &'a [T]
    where
        F: FnMut(&T) -> bool;

    /// Split off the longest prefix of elements for which `pred` returns
    /// `false`, and advance the cursor.
    ///
    /// Stops *before* the first element that matches `pred`; if none matches,
    /// consumes the whole remaining slice.
    fn take_until<F>(&mut self, pred: F) -> &'a [T]
    where
        F: FnMut(&T) -> bool;

    /// Like [`take_until`](Self::take_until), but the taken slice also
    /// includes the first element that matched `pred`, if any.
    fn take_until_incl<F>(&mut self, pred: F) -> &'a [T]
    where
        F: FnMut(&T) -> bool;

    /// Consume and discard the first `n` elements (clamped).
    fn advance(&mut self, n: usize);

    /// Consume and discard the longest prefix matching `pred`; returns the
    /// number of elements skipped.
    fn skip_while<F>(&mut self, pred: F) -> usize
    where
        F: FnMut(&T) -> bool;

    /// Consume and discard elements until `pred` matches (exclusive of the
    /// match); returns the number of elements skipped.
    fn skip_until<F>(&mut self, pred: F) -> usize
    where
        F: FnMut(&T) -> bool;

    /// Like [`skip_until`](Self::skip_until), but also consumes the element
    /// that matched `pred`.
    fn skip_until_incl<F>(&mut self, pred: F) -> usize
    where
        F: FnMut(&T) -> bool;

    /// Return the first `n` elements *without* advancing the cursor.
    fn peek(&self, n: usize) -> &'a [T];

    /// Non-consuming variant of [`take_while`](Self::take_while).
    fn peek_while<F>(&self, pred: F) -> &'a [T]
    where
        F: FnMut(&T) -> bool;

    /// Non-consuming variant of [`take_until`](Self::take_until).
    fn peek_until<F>(&self, pred: F) -> &'a [T]
    where
        F: FnMut(&T) -> bool;

    /// Position of the first element matching `pred`, relative to the current
    /// cursor.
    fn find<F>(&self, pred: F) -> Option<usize>
    where
        F: FnMut(&T) -> bool;

    /// Position of the first occurrence of `tag`, relative to the current
    /// cursor. An empty `tag` matches at position `0`.
    fn find_tag(&self, tag: &[T]) -> Option<usize>
    where
        T: PartialEq;

    /// Split off everything before the first occurrence of `tag` and advance
    /// the cursor to the start of `tag`.
    ///
    /// If `tag` is not found, consumes the entire remaining slice. An empty
    /// `tag` matches immediately: `take_until_tag(&[])` takes nothing.
    fn take_until_tag(&mut self, tag: &[T]) -> &'a [T]
    where
        T: PartialEq;

    /// If the cursor starts with `prefix`, consume it and return the consumed
    /// slice; otherwise return `None` and leave the cursor unchanged.
    ///
    /// This differs from the inherent [`slice::strip_prefix`], which returns
    /// the *remainder* without consuming — `take_prefix` consumes the prefix
    /// from the cursor and returns what was taken, mirroring [`take`](Self::take).
    fn take_prefix(&mut self, prefix: &[T]) -> Option<&'a [T]>
    where
        T: PartialEq;

    /// If the cursor starts with `tag`, consume it and return `true`;
    /// otherwise leave the cursor unchanged and return `false`.
    fn skip_tag(&mut self, tag: &[T]) -> bool
    where
        T: PartialEq;

    /// Non-consuming prefix check.
    fn starts_with(&self, prefix: &[T]) -> bool
    where
        T: PartialEq;

    /// Number of elements left in the cursor.
    fn remaining(&self) -> usize;
}

impl<'a, T> SliceCursor<'a, T> for &'a [T] {
    #[inline]
    fn take(&mut self, n: usize) -> &'a [T] {
        let n = n.min(self.len());
        let (taken, rest) = self.split_at(n);
        *self = rest;
        taken
    }

    #[inline]
    fn take_while<F>(&mut self, mut pred: F) -> &'a [T]
    where
        F: FnMut(&T) -> bool,
    {
        let n = self.iter().position(|x| !pred(x)).unwrap_or(self.len());
        self.take(n)
    }

    #[inline]
    fn take_while_incl<F>(&mut self, mut pred: F) -> &'a [T]
    where
        F: FnMut(&T) -> bool,
    {
        let n = self.iter().position(|x| !pred(x)).unwrap_or(self.len());
        self.take(if n < self.len() { n + 1 } else { n })
    }

    #[inline]
    fn take_until<F>(&mut self, pred: F) -> &'a [T]
    where
        F: FnMut(&T) -> bool,
    {
        let n = self.iter().position(pred).unwrap_or(self.len());
        self.take(n)
    }

    #[inline]
    fn take_until_incl<F>(&mut self, pred: F) -> &'a [T]
    where
        F: FnMut(&T) -> bool,
    {
        let n = self.iter().position(pred).unwrap_or(self.len());
        self.take(if n < self.len() { n + 1 } else { n })
    }

    #[inline]
    fn advance(&mut self, n: usize) {
        let n = n.min(self.len());
        *self = &self[n..];
    }

    #[inline]
    fn skip_while<F>(&mut self, pred: F) -> usize
    where
        F: FnMut(&T) -> bool,
    {
        self.take_while(pred).len()
    }

    #[inline]
    fn skip_until<F>(&mut self, pred: F) -> usize
    where
        F: FnMut(&T) -> bool,
    {
        self.take_until(pred).len()
    }

    #[inline]
    fn skip_until_incl<F>(&mut self, pred: F) -> usize
    where
        F: FnMut(&T) -> bool,
    {
        self.take_until_incl(pred).len()
    }

    #[inline]
    fn peek(&self, n: usize) -> &'a [T] {
        let s = *self;
        &s[..n.min(s.len())]
    }

    #[inline]
    fn peek_while<F>(&self, mut pred: F) -> &'a [T]
    where
        F: FnMut(&T) -> bool,
    {
        let n = self.iter().position(|x| !pred(x)).unwrap_or(self.len());
        self.peek(n)
    }

    #[inline]
    fn peek_until<F>(&self, pred: F) -> &'a [T]
    where
        F: FnMut(&T) -> bool,
    {
        let n = self.iter().position(pred).unwrap_or(self.len());
        self.peek(n)
    }

    #[inline]
    fn find<F>(&self, pred: F) -> Option<usize>
    where
        F: FnMut(&T) -> bool,
    {
        self.iter().position(pred)
    }

    #[inline]
    fn find_tag(&self, tag: &[T]) -> Option<usize>
    where
        T: PartialEq,
    {
        if tag.is_empty() {
            return Some(0);
        }
        if tag.len() > self.len() {
            return None;
        }
        self.windows(tag.len()).position(|w| w == tag)
    }

    #[inline]
    fn take_until_tag(&mut self, tag: &[T]) -> &'a [T]
    where
        T: PartialEq,
    {
        let pos = self.find_tag(tag).unwrap_or(self.len());
        self.take(pos)
    }

    #[inline]
    fn take_prefix(&mut self, prefix: &[T]) -> Option<&'a [T]>
    where
        T: PartialEq,
    {
        if self.starts_with(prefix) {
            Some(self.take(prefix.len()))
        } else {
            None
        }
    }

    #[inline]
    fn skip_tag(&mut self, tag: &[T]) -> bool
    where
        T: PartialEq,
    {
        if self.starts_with(tag) {
            self.advance(tag.len());
            true
        } else {
            false
        }
    }

    #[inline]
    fn starts_with(&self, prefix: &[T]) -> bool
    where
        T: PartialEq,
    {
        <[T]>::starts_with(self, prefix)
    }

    #[inline]
    fn remaining(&self) -> usize {
        self.len()
    }
}

/// [`SliceCursor`] specializations for `&'a [u8]`.
///
/// Byte-haystack searches (single bytes or sub-slices) use
/// [`memchr`](https://docs.rs/memchr)'s SIMD-accelerated routines.
pub trait ByteSliceCursor<'a>: SliceCursor<'a, u8> {
    /// Split off everything before the first occurrence of `byte`, leaving the
    /// cursor at `byte`. If `byte` is absent, consumes everything.
    fn take_until_byte(&mut self, byte: u8) -> &'a [u8];

    /// Like [`take_until_byte`](Self::take_until_byte), but the taken slice
    /// also contains the `byte` itself.
    fn take_until_byte_incl(&mut self, byte: u8) -> &'a [u8];

    /// Split off everything before the first occurrence of `pattern`, leaving
    /// the cursor at the pattern. An empty `pattern` matches immediately.
    fn take_until_bytes(&mut self, pattern: &[u8]) -> &'a [u8];

    /// Skip bytes until `byte`; the cursor ends at `byte`. Returns the number
    /// of bytes skipped.
    fn skip_until_byte(&mut self, byte: u8) -> usize;

    /// Like [`skip_until_byte`](Self::skip_until_byte), but also skips the
    /// `byte` itself.
    fn skip_until_byte_incl(&mut self, byte: u8) -> usize;

    /// Skip bytes until `pattern`; the cursor ends at the pattern start.
    /// Returns the number of bytes skipped.
    fn skip_until_bytes(&mut self, pattern: &[u8]) -> usize;

    /// Like [`skip_until_bytes`](Self::skip_until_bytes), but also skips past
    /// the pattern.
    fn skip_until_bytes_incl(&mut self, pattern: &[u8]) -> usize;

    /// Position of the first `byte` relative to the cursor.
    fn find_byte(&self, byte: u8) -> Option<usize>;

    /// Position of the first `pattern` relative to the cursor.
    fn find_bytes(&self, pattern: &[u8]) -> Option<usize>;

    /// Position of the last `byte` relative to the cursor.
    fn rfind_byte(&self, byte: u8) -> Option<usize>;

    /// Position of the last `pattern` relative to the cursor.
    fn rfind_bytes(&self, pattern: &[u8]) -> Option<usize>;

    /// Consume and return the next byte, or `None` at the end of input.
    fn next_byte(&mut self) -> Option<u8>;

    /// Peek the next byte without consuming.
    fn peek_byte(&self) -> Option<u8>;

    /// If the cursor starts with `byte`, consume it and return `true`;
    /// otherwise leave the cursor unchanged and return `false`.
    fn skip_byte(&mut self, byte: u8) -> bool;
}

impl<'a> ByteSliceCursor<'a> for &'a [u8] {
    #[inline]
    fn take_until_byte(&mut self, byte: u8) -> &'a [u8] {
        let n = memchr(byte, self).unwrap_or(self.len());
        self.take(n)
    }

    #[inline]
    fn take_until_byte_incl(&mut self, byte: u8) -> &'a [u8] {
        match memchr(byte, self) {
            Some(pos) => self.take(pos + 1),
            None => self.take(self.len()),
        }
    }

    #[inline]
    fn take_until_bytes(&mut self, pattern: &[u8]) -> &'a [u8] {
        let n = memmem::find(self, pattern).unwrap_or(self.len());
        self.take(n)
    }

    #[inline]
    fn skip_until_byte(&mut self, byte: u8) -> usize {
        let n = memchr(byte, self).unwrap_or(self.len());
        self.advance(n);
        n
    }

    #[inline]
    fn skip_until_byte_incl(&mut self, byte: u8) -> usize {
        match memchr(byte, self) {
            Some(pos) => {
                self.advance(pos + 1);
                pos + 1
            }
            None => {
                let n = self.len();
                self.advance(n);
                n
            }
        }
    }

    #[inline]
    fn skip_until_bytes(&mut self, pattern: &[u8]) -> usize {
        let n = memmem::find(self, pattern).unwrap_or(self.len());
        self.advance(n);
        n
    }

    #[inline]
    fn skip_until_bytes_incl(&mut self, pattern: &[u8]) -> usize {
        match memmem::find(self, pattern) {
            Some(pos) => {
                let n = pos + pattern.len();
                self.advance(n);
                n
            }
            None => {
                let n = self.len();
                self.advance(n);
                n
            }
        }
    }

    #[inline]
    fn find_byte(&self, byte: u8) -> Option<usize> {
        memchr(byte, self)
    }

    #[inline]
    fn find_bytes(&self, pattern: &[u8]) -> Option<usize> {
        memmem::find(self, pattern)
    }

    #[inline]
    fn rfind_byte(&self, byte: u8) -> Option<usize> {
        memrchr(byte, self)
    }

    #[inline]
    fn rfind_bytes(&self, pattern: &[u8]) -> Option<usize> {
        memmem::rfind(self, pattern)
    }

    #[inline]
    fn next_byte(&mut self) -> Option<u8> {
        let (byte, rest) = self.split_first()?;
        *self = rest;
        Some(*byte)
    }

    #[inline]
    fn peek_byte(&self) -> Option<u8> {
        self.first().copied()
    }

    #[inline]
    fn skip_byte(&mut self, byte: u8) -> bool {
        if matches!(self.first(), Some(&b) if b == byte) {
            let rest = &self[1..];
            *self = rest;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ByteSliceCursor, SliceCursor};

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
}
