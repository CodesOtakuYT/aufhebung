//! The original `hello world!` demonstration, rewritten with trait methods,
//! plus a `split_whitespace` iterator built on top of the cursor API.
//!
//! Run with: `cargo run --example demo`

use aufhebung::SliceCursor;

/// Iterator over the ASCII-whitespace-separated words of a byte slice.
///
/// Yields borrowed sub-slices (zero-copy). Mirrors [`str::split_whitespace`]
/// but for `&[u8]`.
struct SplitWhitespace<'a> {
    input: &'a [u8],
}

impl<'a> Iterator for SplitWhitespace<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        self.input.skip_while(u8::is_ascii_whitespace);
        if self.input.remaining() == 0 {
            None
        } else {
            Some(self.input.take_until(u8::is_ascii_whitespace))
        }
    }
}

/// Split `input` on ASCII whitespace, returning an iterator over the words.
fn split_whitespace(input: &[u8]) -> SplitWhitespace<'_> {
    SplitWhitespace { input }
}

fn main() {
    let x: &[u8] = b"  hello \t world!\n";

    for word in split_whitespace(x) {
        println!("{:?}", str::from_utf8(word).unwrap());
    }

    let words: Vec<&[u8]> = split_whitespace(x).collect();
    dbg!(words);
}
