//! Cursor experiments with a *slice of slices*: splitting a chunked byte
//! stream on whitespace, where a single word may span chunk boundaries.
//!
//! The parent iterator (`Words`) finds each word's boundaries across the
//! chunks, then yields a child iterator (`WordPieces`) over the zero-copy
//! pieces that compose that word. The first piece starts at an offset inside
//! its chunk, and the last piece ends at a length inside its chunk — both
//! boundaries were computed by the parent.
//!
//! Run with: `cargo run --example demo`

use aufhebung::ByteSliceCursor;
use aufhebung::SliceCursor;

/// Parent iterator over the words of a chunked byte stream.
struct Words<'a> {
    chunks: &'a [&'a [u8]],
    /// Index of the current chunk.
    idx: usize,
    /// Byte offset within `chunks[idx]`.
    pos: usize,
}

/// Child iterator over the zero-copy pieces of a single word.
///
/// A word may span several chunks; this iterator yields one `&[u8]` per chunk
/// it touches (empty chunks in the middle are skipped).
struct WordPieces<'a> {
    chunks: &'a [&'a [u8]],
    idx: usize,
    pos: usize,
    end_idx: usize,
    end_pos: usize,
    done: bool,
}

/// Split a chunked byte stream on ASCII whitespace.
///
/// Words are contiguous runs of non-whitespace bytes that may span chunk
/// boundaries. The iterator yields a child iterator of pieces per word.
fn split_whitespace<'a>(chunks: &'a [&'a [u8]]) -> Words<'a> {
    Words {
        chunks,
        idx: 0,
        pos: 0,
    }
}

impl<'a> Iterator for Words<'a> {
    type Item = WordPieces<'a>;

    fn next(&mut self) -> Option<WordPieces<'a>> {
        // Advance past empty chunks and whitespace to find the word start.
        loop {
            while self.idx < self.chunks.len() && self.pos >= self.chunks[self.idx].len() {
                self.idx += 1;
                self.pos = 0;
            }
            if self.idx >= self.chunks.len() {
                return None;
            }
            let chunk = self.chunks[self.idx];
            let mut cursor = &chunk[self.pos..];
            cursor.skip_while(u8::is_ascii_whitespace);
            if cursor.is_empty() {
                // The rest of this chunk is whitespace; move to the next one.
                self.pos = chunk.len();
            } else {
                self.pos = chunk.len() - cursor.len();
                break;
            }
        }

        let start_idx = self.idx;
        let start_pos = self.pos;

        // Scan forward to the first whitespace (or the end of the stream):
        // that is the word's end boundary.
        let mut end_idx = start_idx;
        let mut end_pos = start_pos;
        loop {
            while end_idx < self.chunks.len() && end_pos >= self.chunks[end_idx].len() {
                end_idx += 1;
                end_pos = 0;
            }
            if end_idx >= self.chunks.len() {
                break;
            }
            let chunk = self.chunks[end_idx];
            let mut cursor = &chunk[end_pos..];
            cursor.take_until(u8::is_ascii_whitespace);
            if cursor.is_empty() {
                // No whitespace in this chunk: the word continues past it.
                end_pos = chunk.len();
            } else {
                // The word ends just before this whitespace.
                end_pos = chunk.len() - cursor.len();
                break;
            }
        }

        // The next word starts at this whitespace (skipped on the next call).
        self.idx = end_idx;
        self.pos = end_pos;

        Some(WordPieces {
            chunks: self.chunks,
            idx: start_idx,
            pos: start_pos,
            end_idx,
            end_pos,
            done: false,
        })
    }
}

impl<'a> Iterator for WordPieces<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.done {
            return None;
        }
        if self.idx == self.end_idx {
            // Last piece: from the current offset up to the end boundary
            // decided by the parent.
            if self.pos >= self.end_pos {
                self.done = true;
                return None;
            }
            let piece = &self.chunks[self.idx][self.pos..self.end_pos];
            self.done = true;
            return Some(piece);
        }
        // The word continues past this chunk: take everything from the
        // current offset to the chunk end, then move to the next chunk.
        let piece = &self.chunks[self.idx][self.pos..];
        self.idx += 1;
        self.pos = 0;
        while self.idx < self.end_idx && self.chunks[self.idx].is_empty() {
            self.idx += 1;
        }
        Some(piece)
    }
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

    for (n, word) in split_whitespace(chunks).enumerate() {
        let mut whole = Vec::new();
        for piece in word {
            whole.extend_from_slice(piece);
            println!("  word {n}: piece {:?}", str::from_utf8(piece).unwrap());
        }
        println!("word {n}: {:?}", str::from_utf8(&whole).unwrap());
    }
}
