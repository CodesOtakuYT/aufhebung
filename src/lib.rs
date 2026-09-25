//! # aufhebung
//!
//! Zero-copy slice-cursor operations, accelerated with
//! [`memchr`](https://docs.rs/memchr), provided as an umbrella crate over the
//! engine crate [`aufhebung_core`].
//!
//! The entire core API is re-exported at the crate root — [`SliceCursor`],
//! [`ByteSliceCursor`], [`ChunkedCursor`], [`Pieces`], and the split
//! iterators — so this crate reads exactly like the single-crate library it
//! used to be:
//!
//! ```
//! use aufhebung::{ByteSliceCursor, SliceCursor};
//!
//! let mut input: &[u8] = b"GET /index.html HTTP/1.1\r\n\r\n";
//! let method = input.take_until(u8::is_ascii_whitespace);
//! assert_eq!(method, b"GET");
//! ```
//!
//! The crate intentionally contains no implementation of its own; see the
//! [`aufhebung_core`] documentation for the full API. Add-on crates are
//! re-exported here the same way — currently the HTTP/1.1 request parser
//! [`aufhebung_http`] — so `aufhebung` stays the one name to depend on.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// The engine crate: the cursor primitives themselves (also flattened into
/// this crate's root via the glob re-export below).
pub use aufhebung_core;

/// Re-export of the complete core API at the crate root.
pub use aufhebung_core::*;

/// The add-on HTTP/1.1 request parser crate (also flattened into this
/// crate's root via the glob re-export below).
pub use aufhebung_http;

/// Re-export of the complete HTTP API at the crate root.
pub use aufhebung_http::*;
