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
//!
//! [`ChunkedCursor`] extends the cursor pattern to a *stream* of slices
//! (`&[&[T]]`), where scanning bridges chunk boundaries automatically. A span
//! that crosses a chunk cannot be a single contiguous `&[T]`, so cross-boundary
//! `take*` methods return a [`Pieces`] iterator — one zero-copy sub-slice per
//! chunk touched — and [`ChunkedCursor::split_whitespace`] splits a chunked
//! byte stream into [`Words`], each composed of its [`Pieces`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use memchr::{memchr, memmem, memrchr};
use std::hash::{Hash, Hasher};
use std::iter::FusedIterator;

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

/// Zero-copy cursor over a *stream* of slices.
///
/// [`SliceCursor`] advances a plain `&'a [T]`; `ChunkedCursor` is the
/// equivalent for input that arrives split across chunks (`&'a [&'a [T]]`): an
/// element position is tracked as `(chunk index, offset inside that chunk)`,
/// and scanning operations bridge chunk boundaries automatically. Empty chunks
/// are treated as nothing and skipped.
///
/// One operation cannot be copied verbatim from [`SliceCursor`]: a `take*`
/// method returns a single contiguous `&'a [T]`, and a span that crosses a
/// chunk boundary has no contiguous representation. The chunked `take*`
/// counterparts therefore return a zero-copy [`Pieces`] iterator with one
/// `&'a [T]` per chunk the span touches (the first piece starts at the offset
/// where the scan began; the last ends at the boundary that ended it). This is
/// also why `ChunkedCursor` cannot itself implement [`SliceCursor`].
///
/// ```
/// use aufhebung::{ByteSliceCursor, ChunkedCursor};
///
/// let mut input = ChunkedCursor::new(&[b"ab", b"c de", b"f"]);
///
/// let first: Vec<&[u8]> = input.take_until(|&b| b.is_ascii_whitespace()).collect();
/// assert_eq!(first, [&b"ab"[..], &b"c"[..]]);
/// assert_eq!(input.peek_byte(), Some(b' '));
///
/// let words: Vec<Vec<&[u8]>> = input
///     .split_whitespace()
///     .map(|w| w.collect())
///     .collect();
/// assert_eq!(words, [[&b"de"[..], &b"f"[..]]]);
/// ```
pub struct ChunkedCursor<'a, T> {
    chunks: &'a [&'a [T]],
    /// Index of the chunk currently pointed at.
    idx: usize,
    /// Offset within `chunks[idx]`; only meaningful while `idx < chunks.len()`.
    pos: usize,
}

impl<'a, T> Clone for ChunkedCursor<'a, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, T> Copy for ChunkedCursor<'a, T> {}

impl<'a, T> ChunkedCursor<'a, T> {
    /// Build a cursor over a stream of chunks.
    ///
    /// The cursor starts at the first element of the first chunk; empty chunks
    /// are skipped as the cursor advances.
    #[inline]
    pub fn new(chunks: &'a [&'a [T]]) -> Self {
        Self {
            chunks,
            idx: 0,
            pos: 0,
        }
    }

    /// Advance past exhausted and empty chunks so the current chunk is
    /// readable, or so that `idx` points past the end of the stream.
    fn skip_exhausted(&mut self) {
        while self.idx < self.chunks.len() && self.pos >= self.chunks[self.idx].len() {
            self.idx += 1;
            self.pos = 0;
        }
    }

    /// Whether any elements remain in the stream.
    #[inline]
    pub fn is_empty(&self) -> bool {
        let mut cursor = *self;
        cursor.skip_exhausted();
        cursor.idx >= cursor.chunks.len()
    }

    /// Number of elements remaining in the stream, summed across chunks.
    ///
    /// Runs in time linear in the number of remaining chunks.
    pub fn remaining(&self) -> usize {
        let Some(first) = self.chunks.get(self.idx) else {
            return 0;
        };
        first.len().saturating_sub(self.pos)
            + self.chunks[self.idx + 1..]
                .iter()
                .map(|chunk| chunk.len())
                .sum::<usize>()
    }

    /// Peek the current element without consuming it.
    ///
    /// Bridging to a later chunk if the current one is exhausted or empty.
    pub fn peek(&self) -> Option<&'a T> {
        let mut cursor = *self;
        cursor.skip_exhausted();
        let chunk = *cursor.chunks.get(cursor.idx)?;
        chunk.get(cursor.pos)
    }

    /// Consume and discard elements while `pred` holds, bridging chunk
    /// boundaries. Returns the number of elements skipped.
    pub fn skip_while<F>(&mut self, mut pred: F) -> usize
    where
        F: FnMut(&T) -> bool,
    {
        let mut skipped = 0;
        loop {
            self.skip_exhausted();
            if self.idx >= self.chunks.len() {
                break;
            }
            let chunk = self.chunks[self.idx];
            let mut cursor = &chunk[self.pos..];
            let n = cursor.skip_while(&mut pred);
            skipped += n;
            self.pos += n;
            if self.pos < chunk.len() {
                break;
            }
        }
        skipped
    }

    /// Consume and discard elements until `pred` matches, bridging chunk
    /// boundaries. Returns the number of elements skipped; the cursor ends at
    /// the first matching element (exclusive of the skip), or at the end of
    /// the stream if none matches.
    pub fn skip_until<F>(&mut self, mut pred: F) -> usize
    where
        F: FnMut(&T) -> bool,
    {
        let mut skipped = 0;
        loop {
            self.skip_exhausted();
            if self.idx >= self.chunks.len() {
                break;
            }
            let chunk = self.chunks[self.idx];
            let mut cursor = &chunk[self.pos..];
            let n = cursor.skip_until(&mut pred);
            skipped += n;
            self.pos += n;
            if self.pos < chunk.len() {
                break;
            }
        }
        skipped
    }

    /// Split off everything before the first element matching `pred` and
    /// return it as a zero-copy [`Pieces`] iterator, advancing the cursor to
    /// the matching element.
    ///
    /// If no element matches, the pieces cover the whole remaining stream and
    /// the cursor ends at the end of the stream. An immediate match yields an
    /// empty [`Pieces`] iterator, mirroring the flat
    /// [`take_until`](SliceCursor::take_until) returning an empty slice.
    ///
    /// The starting position is normalized first: if the cursor points into an
    /// exhausted or empty chunk, both the span and the cursor begin at the
    /// first readable element, so no empty leading piece is produced.
    pub fn take_until<F>(&mut self, mut pred: F) -> Pieces<'a, T>
    where
        F: FnMut(&T) -> bool,
    {
        self.skip_exhausted();
        let start_idx = self.idx;
        let start_pos = self.pos;
        let mut end = *self;
        end.skip_until(&mut pred);
        self.idx = end.idx;
        self.pos = end.pos;
        Pieces::new(self.chunks, start_idx, start_pos, end.idx, end.pos)
    }
}

impl<'a> ChunkedCursor<'a, u8> {
    /// Peek the next byte without consuming it, bridging chunk boundaries.
    #[inline]
    pub fn peek_byte(&self) -> Option<u8> {
        self.peek().copied()
    }

    /// Consume and return the next byte, bridging chunk boundaries.
    pub fn next_byte(&mut self) -> Option<u8> {
        self.skip_exhausted();
        if self.idx >= self.chunks.len() {
            return None;
        }
        let chunk = self.chunks[self.idx];
        let byte = chunk[self.pos];
        self.pos += 1;
        Some(byte)
    }

    /// Split off everything before the first occurrence of `byte` and return
    /// it as zero-copy [`Pieces`], advancing the cursor to the `byte`.
    ///
    /// Searching is memchr-accelerated within each chunk. If `byte` is absent,
    /// the pieces cover the remaining stream.
    ///
    /// Like [`take_until`](Self::take_until), the starting position is
    /// normalized first, so a leading exhausted or empty chunk produces no
    /// empty leading piece.
    pub fn take_until_byte(&mut self, byte: u8) -> Pieces<'a, u8> {
        self.skip_exhausted();
        let start_idx = self.idx;
        let start_pos = self.pos;
        let mut end = *self;
        end.skip_until_byte(byte);
        self.idx = end.idx;
        self.pos = end.pos;
        Pieces::new(self.chunks, start_idx, start_pos, end.idx, end.pos)
    }

    /// Skip bytes until `byte` (exclusive of the byte), bridging chunks. The
    /// cursor ends at the `byte`. Returns the number of bytes skipped.
    ///
    /// Searching is memchr-accelerated within each chunk.
    pub fn skip_until_byte(&mut self, byte: u8) -> usize {
        let mut skipped = 0;
        loop {
            self.skip_exhausted();
            if self.idx >= self.chunks.len() {
                break;
            }
            let chunk = self.chunks[self.idx];
            let mut cursor = &chunk[self.pos..];
            let n = cursor.skip_until_byte(byte);
            skipped += n;
            self.pos += n;
            if self.pos < chunk.len() {
                break;
            }
        }
        skipped
    }

    /// Return an iterator over the ASCII-whitespace-separated words of the
    /// remaining stream, where a word may span chunk boundaries.
    ///
    /// Each yielded item is a [`Pieces`] iterator over the zero-copy sub-slices
    /// composing the word — one piece per chunk the word touches (empty chunks
    /// are skipped). Whitespace runs are collapsed, mirroring
    /// [`ByteSliceCursor::split_whitespace`] for a single contiguous slice.
    ///
    /// This is a non-consuming snapshot: the cursor keeps its position.
    pub fn split_whitespace(&self) -> Words<'a> {
        Words {
            chunks: self.chunks,
            idx: self.idx,
            pos: self.pos,
        }
    }
}

/// Iterator over the zero-copy sub-slices of a span that crosses chunk
/// boundaries.
///
/// Produced by [`ChunkedCursor::take_until`] (and
/// [`ChunkedCursor::take_until_byte`]). A span touching several chunks has no
/// single contiguous `&'a [T]`; instead it yields one `&'a [T]` per chunk the
/// span touches: the first piece starts at the offset where the scan began,
/// intermediate pieces are whole chunks, and the last piece ends at the
/// boundary that ended the scan. Empty chunks in the middle of a span are
/// skipped, so no empty piece is ever yielded. Implements
/// [`ExactSizeIterator`], since the boundaries were fixed by the cursor.
///
/// Byte spans also behave like their contiguous contents without collecting:
/// they compare with `==` (including against byte-string literals such as
/// `b"GET"`), implement [`Hash`], and can be parsed with
/// [`parse_integer`](Pieces::parse_integer).
pub struct Pieces<'a, T> {
    chunks: &'a [&'a [T]],
    idx: usize,
    pos: usize,
    end_idx: usize,
    end_pos: usize,
    done: bool,
    len: usize,
}

impl<'a, T> Clone for Pieces<'a, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, T> Copy for Pieces<'a, T> {}

impl<'a, T> Pieces<'a, T> {
    fn new(
        chunks: &'a [&'a [T]],
        start_idx: usize,
        start_pos: usize,
        end_idx: usize,
        end_pos: usize,
    ) -> Self {
        let len = if start_idx == end_idx {
            usize::from(start_pos < end_pos)
        } else {
            let middles = chunks[start_idx + 1..end_idx]
                .iter()
                .filter(|chunk| !chunk.is_empty())
                .count();
            1 + middles + usize::from(end_pos > 0)
        };
        Self {
            chunks,
            idx: start_idx,
            pos: start_pos,
            end_idx,
            end_pos,
            done: false,
            len,
        }
    }
}

impl<'a, T> Iterator for Pieces<'a, T> {
    type Item = &'a [T];

    fn next(&mut self) -> Option<&'a [T]> {
        if self.done {
            return None;
        }
        let piece = if self.idx == self.end_idx {
            if self.pos >= self.end_pos {
                self.done = true;
                return None;
            }
            self.done = true;
            &self.chunks[self.idx][self.pos..self.end_pos]
        } else {
            let piece = &self.chunks[self.idx][self.pos..];
            self.idx += 1;
            self.pos = 0;
            while self.idx < self.end_idx && self.chunks[self.idx].is_empty() {
                self.idx += 1;
            }
            piece
        };
        self.len -= 1;
        Some(piece)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<'a, T> ExactSizeIterator for Pieces<'a, T> {
    fn len(&self) -> usize {
        self.len
    }
}

impl<'a, T> FusedIterator for Pieces<'a, T> {}

impl<'a> Pieces<'a, u8> {
    /// Parse the span's bytes as a signed decimal integer.
    ///
    /// An optional leading `+` or `-` is accepted, followed by at least one
    /// ASCII digit. Digits may straddle piece boundaries. Any other byte —
    /// whitespace, or a sign anywhere but first — yields `None`, as do an
    /// empty span and values outside [`i64`]'s range. Accumulation uses
    /// checked arithmetic, so `i64::MIN` parses, while `i64::MAX + 1` and any
    /// overflow return `None`. No allocation occurs.
    ///
    /// ```
    /// use aufhebung::ChunkedCursor;
    ///
    /// let digits = ChunkedCursor::new(&[b"4", b"2!"]).take_until_byte(b'!');
    /// assert_eq!(digits.parse_integer(), Some(42));
    /// ```
    pub fn parse_integer(&self) -> Option<i64> {
        let it = *self;
        let mut value: i128 = 0;
        let mut has_digit = false;
        let mut sign = false;
        let mut negative = false;
        for piece in it {
            for &b in piece {
                match b {
                    b'+' | b'-' if !sign && !has_digit => {
                        sign = true;
                        negative = b == b'-';
                    }
                    b'0'..=b'9' => {
                        has_digit = true;
                        value = value.checked_mul(10)?.checked_add(i128::from(b - b'0'))?;
                    }
                    _ => return None,
                }
            }
        }
        if !has_digit {
            return None;
        }
        let value = if negative { -value } else { value };
        i64::try_from(value).ok()
    }
}

/// Element-wise equality of two piece sequences that may split the data
/// differently (e.g. `["he", "llo"]` equals `["h", "ello"]`). Lead or tail
/// slices of length zero (which `Pieces` itself never produces) are drained.
fn span_bytes_eq<'a, 'b, A, B>(mut a: A, mut b: B) -> bool
where
    A: Iterator<Item = &'a [u8]>,
    B: Iterator<Item = &'b [u8]>,
{
    let (mut ap, mut bp) = (a.next(), b.next());
    loop {
        match (ap, bp) {
            (None, None) => return true,
            (None, Some([])) => bp = b.next(),
            (Some([]), None) => ap = a.next(),
            (None, Some(_)) | (Some(_), None) => return false,
            (Some(asl), Some(bsl)) => {
                let n = asl.len().min(bsl.len());
                if asl[..n] != bsl[..n] {
                    return false;
                }
                ap = if asl.len() == n {
                    a.next()
                } else {
                    Some(&asl[n..])
                };
                bp = if bsl.len() == n {
                    b.next()
                } else {
                    Some(&bsl[n..])
                };
            }
        }
    }
}

impl<'a> PartialEq for Pieces<'a, u8> {
    fn eq(&self, other: &Self) -> bool {
        span_bytes_eq(*self, *other)
    }
}

impl<'a, 'b, const N: usize> PartialEq<&'b [u8; N]> for Pieces<'a, u8> {
    fn eq(&self, other: &&'b [u8; N]) -> bool {
        span_bytes_eq(*self, std::iter::once(other.as_slice()))
    }
}

impl<'a> Eq for Pieces<'a, u8> {}

impl<'a> Hash for Pieces<'a, u8> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Mirror `Hash for [u8]` (and therefore `Vec<u8>`): a length prefix
        // followed by the bytes in order, so hashing a `Pieces` span is
        // identical to hashing the same bytes collected into one slice.
        let len: usize = (*self).map(|piece| piece.len()).sum();
        state.write_usize(len);
        for piece in *self {
            state.write(piece);
        }
    }
}

/// Iterator over the ASCII-whitespace-separated words of a *chunked* byte
/// stream, where words may span chunk boundaries.
///
/// Produced by [`ChunkedCursor::split_whitespace`]; each item is a [`Pieces`]
/// iterator over the zero-copy sub-slices composing the word.
#[derive(Clone, Copy)]
pub struct Words<'a> {
    chunks: &'a [&'a [u8]],
    idx: usize,
    pos: usize,
}

impl<'a> Iterator for Words<'a> {
    type Item = Pieces<'a, u8>;

    fn next(&mut self) -> Option<Pieces<'a, u8>> {
        let mut cursor = ChunkedCursor {
            chunks: self.chunks,
            idx: self.idx,
            pos: self.pos,
        };
        cursor.skip_while(u8::is_ascii_whitespace);
        if cursor.is_empty() {
            return None;
        }
        let pieces = cursor.take_until(u8::is_ascii_whitespace);
        self.idx = cursor.idx;
        self.pos = cursor.pos;
        Some(pieces)
    }
}

impl<'a> FusedIterator for Words<'a> {}
