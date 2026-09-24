//! The original `hello world!` demonstration, rewritten with trait methods.
//!
//! Run with: `cargo run --example demo`

use aufhebung::SliceCursor;

fn main() {
    let mut x: &[u8] = b"hello world!";

    let whitespace = x.take_until(u8::is_ascii_whitespace);

    dbg!(str::from_utf8(whitespace).unwrap());
    dbg!(x);
}
