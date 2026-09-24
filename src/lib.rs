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
//!
//! The cursor methods also compose into zero-copy segmenting iterators:
//! [`SliceCursor::split_on`] for generic predicates, plus the byte-specialized
//! [`ByteSliceCursor::split_whitespace`] and
//! [`ByteSliceCursor::split_bytes`] (SIMD-accelerated).

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
    /// This differs from the inherent `<[T]>::strip_prefix`, which returns
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

    /// Return an iterator over the sub-slices between elements matching
    /// `split`, mirroring the inherent `<[T]>::split` but as a non-consuming
    /// snapshot of the cursor.
    ///
    /// Each separator element is consumed by the iterator; consecutive
    /// separators yield empty segments, and a trailing separator yields no
    /// trailing empty segment.
    fn split_on<F>(&self, split: F) -> SplitOn<'a, T, F>
    where
        F: FnMut(&T) -> bool;
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

    #[inline]
    fn split_on<F>(&self, split: F) -> SplitOn<'a, T, F>
    where
        F: FnMut(&T) -> bool,
    {
        SplitOn::new(self, split)
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

    /// Return an iterator over the ASCII-whitespace-separated words of the
    /// remaining input, mirroring [`str::split_whitespace`] but for `&[u8]`.
    ///
    /// Runs of whitespace are collapsed: leading/trailing whitespace and
    /// consecutive separators never produce empty words.
    fn split_whitespace(&self) -> SplitWhitespace<'a>;

    /// Return an iterator over the sub-slices between occurrences of
    /// `pattern`, mirroring the semantics of [`str::split`].
    ///
    /// Searching is accelerated with a precomputed **memmem** `Finder`
    /// (SIMD-optimized by memchr). Consecutive occurrences yield empty
    /// segments; a trailing occurrence yields no trailing empty segment.
    /// Panics if `pattern` is empty.
    fn split_bytes<'p>(&self, pattern: &'p [u8]) -> SplitBytes<'a, 'p>;
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

    #[inline]
    fn split_whitespace(&self) -> SplitWhitespace<'a> {
        SplitWhitespace::new(self)
    }

    #[inline]
    fn split_bytes<'p>(&self, pattern: &'p [u8]) -> SplitBytes<'a, 'p> {
        SplitBytes::new(self, pattern)
    }
}

/// Iterator over the ASCII-whitespace-separated words of a byte slice.
///
/// Produced by [`ByteSliceCursor::split_whitespace`] and mirroring
/// [`str::split_whitespace`]: yields non-empty, zero-copy sub-slices, with
/// runs of whitespace collapsed.
pub struct SplitWhitespace<'a> {
    input: &'a [u8],
}

impl<'a> SplitWhitespace<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input }
    }
}

impl<'a> Iterator for SplitWhitespace<'a> {
    type Item = &'a [u8];

    #[inline]
    fn next(&mut self) -> Option<&'a [u8]> {
        self.input.skip_while(u8::is_ascii_whitespace);
        if self.input.is_empty() {
            None
        } else {
            Some(self.input.take_until(u8::is_ascii_whitespace))
        }
    }
}

/// Iterator over the sub-slices of a slice separated by elements matching a
/// predicate.
///
/// Produced by [`SliceCursor::split_on`] and mirroring the inherent
/// `<[T]>::split`: each separator element is consumed, consecutive separators
/// yield empty segments, and a trailing separator yields no trailing empty
/// segment.
pub struct SplitOn<'a, T, F> {
    input: &'a [T],
    split: F,
}

impl<'a, T, F> SplitOn<'a, T, F> {
    fn new(input: &'a [T], split: F) -> Self {
        Self { input, split }
    }
}

impl<'a, T, F: FnMut(&T) -> bool> Iterator for SplitOn<'a, T, F> {
    type Item = &'a [T];

    #[inline]
    fn next(&mut self) -> Option<&'a [T]> {
        if self.input.is_empty() {
            return None;
        }
        let segment = self.input.take_until(&mut self.split);
        if !self.input.is_empty() {
            self.input.advance(1);
        }
        Some(segment)
    }
}

/// Iterator over the sub-slices of a byte slice separated by a byte pattern.
///
/// Produced by [`ByteSliceCursor::split_bytes`] and mirroring the semantics of
/// [`str::split`]: consecutive occurrences yield empty segments, and a
/// trailing occurrence yields no trailing empty segment. Searching uses a
/// precomputed **memmem** `Finder`, so repeated searches are SIMD-accelerated.
/// The pattern must be non-empty.
pub struct SplitBytes<'a, 'p> {
    input: &'a [u8],
    finder: memmem::Finder<'p>,
    pattern_len: usize,
}

impl<'a, 'p> SplitBytes<'a, 'p> {
    fn new(input: &'a [u8], pattern: &'p [u8]) -> Self {
        assert!(!pattern.is_empty(), "split_bytes: empty pattern");
        Self {
            input,
            finder: memmem::Finder::new(pattern),
            pattern_len: pattern.len(),
        }
    }
}

impl<'a, 'p> Iterator for SplitBytes<'a, 'p> {
    type Item = &'a [u8];

    #[inline]
    fn next(&mut self) -> Option<&'a [u8]> {
        if self.input.is_empty() {
            return None;
        }
        match self.finder.find(self.input) {
            Some(pos) => {
                let (segment, rest) = self.input.split_at(pos);
                self.input = &rest[self.pattern_len..];
                Some(segment)
            }
            None => {
                let segment = self.input;
                self.input = &[];
                Some(segment)
            }
        }
    }
}
