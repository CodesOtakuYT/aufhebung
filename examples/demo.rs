//! The original `hello world!` demonstration, rewritten with trait methods,
//! plus word splitting via the library's `split_whitespace` iterator, and a
//! *slice-of-slices* variant using [`ChunkedCursor`]: a chunked byte stream is
//! split on whitespace across chunk boundaries, with each word yielding a
//! [`Pieces`] iterator over the zero-copy sub-slices that compose it.
//!
//! Run with: `cargo run --example demo`

use aufhebung::{ByteSliceCursor, ChunkedCursor};

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
        let mut whole = Vec::new();
        for piece in pieces {
            whole.extend_from_slice(piece);
            println!("  word {n}: piece {:?}", str::from_utf8(piece).unwrap());
        }
        println!("word {n}: {:?}", str::from_utf8(&whole).unwrap());
    }
}
