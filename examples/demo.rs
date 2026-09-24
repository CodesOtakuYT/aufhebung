//! The original `hello world!` demonstration, rewritten with trait methods,
//! plus a `split_whitespace` helper built on top of the cursor API.
//!
//! Run with: `cargo run --example demo`

use aufhebung::SliceCursor;

/// Split the remaining input on ASCII whitespace, consuming the cursor.
///
/// Returns borrowed sub-slices (zero-copy); on return the cursor has been
/// advanced past every word, ending at the end of input.
fn split_whitespace<'a>(input: &mut &'a [u8]) -> Vec<&'a [u8]> {
    let mut words = Vec::new();
    loop {
        input.skip_while(u8::is_ascii_whitespace);
        if input.remaining() == 0 {
            break;
        }
        words.push(input.take_until(u8::is_ascii_whitespace));
    }
    words
}

fn main() {
    let mut x: &[u8] = b"  hello \t world!\n";

    let words = split_whitespace(&mut x);

    for word in &words {
        println!("{:?}", str::from_utf8(word).unwrap());
    }
    dbg!(words);
    dbg!(x); // fully consumed
}
