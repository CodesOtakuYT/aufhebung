//! The original `hello world!` demonstration, rewritten with trait methods,
//! plus word splitting via the library's `split_whitespace` iterator.
//!
//! Run with: `cargo run --example demo`

use aufhebung::ByteSliceCursor;

fn main() {
    let x: &[u8] = b"  hello \t world!\n";

    for word in x.split_whitespace() {
        println!("{:?}", str::from_utf8(word).unwrap());
    }

    let words: Vec<&[u8]> = x.split_whitespace().collect();
    dbg!(words);
}
