# aufhebung

Zero-copy slice-cursor operations, accelerated with [`memchr`](https://docs.rs/memchr) for bytes.

`slice.f(...)` extension traits keep the cursor pattern — `take_*` returns
sub-slices that borrow from the input while advancing an in-place `&mut &[T]`
cursor, so nothing is ever copied or allocated on the scanning path.

```rust
use aufhebung::ByteSliceCursor;

let mut rest: &[u8] = b"GET /index.html HTTP/1.1\r\n\r\n";
let method = rest.take_until_byte(b' ');   // b"GET"
rest.skip_byte(b' ');                      // SP
let target = rest.take_until_byte(b' ');   // b"/index.html"
```

Byte-haystack searching (single bytes, `memmem` sub-slices, and sets of bytes
via `memchr2`/`memchr3`, with a bitmap fallback for larger sets) is delegated
to `memchr`'s optimized routines, so the flat methods cost the same as calling
`memchr` directly.

## Chunked streams

[`ChunkedCursor`] extends the cursor to a *stream* of slices (`&[&[T]]`),
scanning across chunk boundaries automatically. Spans that cross a chunk are
returned as a [`Pieces`] iterator — one zero-copy sub-slice per chunk touched
— and fields stay usable as zero-copy values via the `Pieces` value ops
(`==`, `starts_with`, `Hash`, `parse_integer`, `byte_len`, `Display`).

See `examples/demo.rs` (`cargo run --example demo`) for a full HTTP/1 parser
built on both the flat and chunked cursors.

## Workspace layout

This repository is a Cargo workspace. The `aufhebung` crate at the root is an
umbrella (facade): it re-exports the engine crate and owns the tests,
examples, and benchmarks, so `cargo test`, `cargo bench`, and
`cargo run --example demo` work exactly as for a single crate.

- `crates/aufhebung-core` — the implementation: slice and chunked cursors,
  `Pieces`, split iterators, `memchr` acceleration. Depend on this directly
  for the narrow primitive API.
- `aufhebung` (root) — `pub use aufhebung_core::*`, so one dependency gives
  the full API. Future add-on crates (`aufhebung-http`, `aufhebung-json`, …)
  land in `crates/` and are re-exported here.
- `benches/`, `tests/`, `examples/` — owned by the root crate; they exercise
  the umbrella's public surface.

## Benchmarks

Run with `cargo bench` ([`criterion`](https://docs.rs/criterion) 0.5.1, 100
samples per benchmark). Median times; lower is better. All numbers are *on
these benchmarks*, *on this machine*; no claims about other workloads are
made.

Methodology: Intel Core i5-10400F @ 2.90 GHz (x86_64), rustc 1.100.0-nightly,
criterion 0.5.1 with default settings (100 samples, estimated measurement
time), release-profile defaults — no `target-cpu=native`, no LTO. Where two
parsers are compared they run on identical byte inputs and both sides are
zero-allocation unless stated. Sources: `benches/*.rs`.

### HTTP/1 request + header parsing (contiguous buffer)

`benches/http.rs`. The same request line + header block, parsed by aufhebung's
demo parser (as-written and in a zero-alloc variant with a fixed-size header
array), by [`httparse`](https://docs.rs/httparse) (the zero-copy HTTP/1 parser
hyper is built on), and by a naive parser with no `memchr`.

| input      | aufhebung | aufhebung-noalloc | httparse | naive |
|------------|-----------|-------------------|----------|-------|
| small, 94 B | 63.6 ns  | 51.7 ns           | 131.0 ns | 72.4 ns |
| large, 458 B | 269.0 ns | 159.7 ns          | 289.7 ns | 406.2 ns |

On these benchmarks, the zero-allocation version of the demo parser is
**1.8–2.5× faster than httparse**. The as-written demo parser pays for a
growable `Vec` of headers (~12 ns small, ~110 ns for 12 headers); reusing the
buffer or using the array variant removes that. The naive parser holds up on
the small request — a few dozen bytes don't reward SIMD — and falls behind by
~1.5× on the header-heavy one.

### HTTP/1 parsing across chunk boundaries

`benches/http_chunked.rs`. The same 94-byte request split into 3 or 16 chunks
and parsed with [`ChunkedCursor`], vs the flat parsers over the same bytes
joined into one buffer.

| chunking      | aufhebung |
|---------------|-----------|
| flat (1 chunk)| 64.9 ns   |
| 3 chunks      | 154.9 ns  |
| 16 chunks     | 209.0 ns  |
| flat-httparse (reference) | 131.6 ns |

On this 94-byte input, cross-chunk scanning costs about 2.4–3.2× the flat
cursor but stays faster than a single `httparse` pass on the same bytes — and
every field remains zero-copy `Pieces` even when it straddles chunks. The
first split is the expensive one (1→3 chunks more than doubles the time — the
request line and most fields now cross boundaries); past that each additional
boundary costs roughly 4 ns. Whether this trade is worth it depends entirely
on whether your input arrives fragmented — the crate's position is that
*accepting* fragmentation beats *paying to remove it*, which is what the
streaming-fragmentation section below measures.

### Integer parsing

`benches/int.rs`. `Pieces::parse_integer` vs `btoi`, `atoi`, and `std`
`str::parse::<i64>`, on a single contiguous span (the fair case for the
slice-based parsers).

| input      | aufhebung | btoi | atoi | std |
|------------|-----------|------|------|-----|
| `"42"`     | 19.8 ns   | 2.2 ns | 2.7 ns | 6.7 ns |
| `"-1000"`  | 25.1 ns   | 3.7 ns | 3.7 ns | 10.2 ns |
| `"12345"`  | 26.4 ns   | 4.4 ns | 4.5 ns | 9.9 ns |
| `i64::MAX` | 54.9 ns   | 16.8 ns | 14.7 ns | 18.8 ns |
| `i64::MIN` | 59.5 ns   | 16.7 ns | 14.6 ns | 27.1 ns |

On a contiguous span, the dedicated parsers win by 3–9×: `parse_integer`
carries ~17 ns of cursor/pieces preparation plus ~2 ns per digit, where
`btoi`/`atoi` spend ~1 ns per digit inline. Its edge is fragmentation — an
`i64` whose digits cross chunk boundaries still parses zero-copy. The sweep
below splits `i64::MAX` (19 digits) into N equal chunks:

| chunks | aufhebung `parse_integer` | flatten to `Vec` then `btoi` |
|--------|---------------------------|------------------------------|
| 1      | 54.8 ns                   | 39.6 ns                      |
| 2      | 57.9 ns                   | 53.5 ns                      |
| 3      | 61.3 ns                   | 107.0 ns                     |
| 5      | 68.4 ns                   | 113.9 ns                     |
| 10     | 87.0 ns                   | 111.6 ns                     |
| 18     | 125.0 ns                  | 119.2 ns                     |

Only digits that actually straddle a boundary cost `parse_integer` anything —
contiguous is 54.8 ns and each added boundary is roughly 4 ns. The
flatten-then-parse alternative copies all 19 bytes on every call: ~40–53 ns at
one or two spans, where the tiny copy is cheap enough to win, then jumps to a
~107–119 ns plateau once the fold iterates three or more pieces. `parse_integer`
therefore wins the middle of the sweep (3, 5, 10 pieces: 61–87 ns vs 107–114
ns) and the two converge at the extremes — at 18 pieces both sit at ~120–125
ns. The edge is real but modest on a 19-digit `i64`, whose copy cost is
inherently small; longer values, or spans that are re-fragmented repeatedly,
would widen it.

### Streaming fragmentation: the crate's reason to exist

`benches/fragmentation.rs`. Four similar-sized payloads (~7–8 KiB each: an
HTTP/1 request, a JSON document, an XML document, and a synthetic DNS-style
binary stream) are parsed two ways with the *identical* parser:

- **Strategy A — keep contiguity**: fold the chunk list into one buffer (copy
  every byte), then parse.
- **Strategy B — accept fragmentation**: parse the chunk list directly via
  [`ChunkedCursor`], zero copies.

The only difference between the two is the copy and the chunk-bookkeeping, so
the table shows the real price of maintaining contiguity. Fragment sizes:
contiguous (single buffer), 4 KiB, 64 B, and 8 B chunks (the last is ~1,000
pieces). Median times in µs:

| payload | strategy | contiguous | 4 KiB | 64 B | 8 B |
|---------|----------|------------|-------|------|-----|
| HTTP | A concat | 10.6 | 11.3 | 11.4 | 11.3 |
| HTTP | B chunked | 6.4 | 6.4 | 7.9 | 9.2 |
| JSON | A concat | 37.4 | 37.5 | 38.7 | 38.5 |
| JSON | B chunked | 31.3 | 31.3 | 32.5 | 34.1 |
| XML  | A concat | 18.5 | 18.9 | 19.5 | 19.7 |
| XML  | B chunked | 13.1 | 13.2 | 13.9 | 18.3 |
| DNS  | A concat | 10.9 | 11.2 | 11.7 | 11.8 |
| DNS  | B chunked | 6.0 | 6.0 | 7.0 | 10.6 |

On these benchmarks strategy B wins at *every* fragmentation level, including
8 B chunks: the zero-copy parse of a heavily fragmented stream still beats
copying the whole message first. Strategy A's rows barely move because its
cost is dominated by the copy itself (~5 µs incl. a fresh `Vec` allocation for
these payloads), which hides the parse underneath. The price of *accepting*
fragmentation — B(8 B) vs B(contiguous) — is 1.1× (JSON) to 1.8× (DNS).

Honest caveats:

- Strategy A here is the *worst case* for a contiguous handler: every byte of
  every message is copied into a fresh `Vec` via an iterator `collect`, not a
  pooled buffer and not an optimized `memcpy`. A mature streamer that reuses a
  buffer and memcpys ~8 KiB (~0.3 µs) would flip the contiguous rows and
  probably the 4 KiB rows too. The point stands where fragmentation is the
  rule: strategy B also avoids the source-side copy entirely — the socket
  `recv` can land straight in per-message buffers and hand the pointers over.
- The DNS payload is synthetic — real DNS messages are far too small to make a
  fragmentation table — with a realistic binary layout (fixed 12-byte header,
  length-prefixed labels, fixed tail fields).
- Each benchmark asserts its parser returns the identical result for the
  contiguous and the fragmented inputs, so the numbers compare like for like.

### Byte-scanning primitives

`benches/scan.rs`.

**Single-byte search** (first comma in 16 KiB of CSV; the needle sits at byte
6): aufhebung 2.20 ns ≈ raw `memchr` 2.07 ns ≈ naive `position` 2.36 ns — the
cursor wrapper is free over the raw search.

**Split on single-byte comma** (counting segments of 16 KiB of CSV):

| aufhebung | memmem `find_iter` | bstr `split_str` | naive count |
|-----------|--------------------|------------------|-------------|
| 15.9 µs   | 16.5 µs            | 16.5 µs          | 8.0 µs      |

aufhebung ≈ raw memmem ≈ bstr: the wrappers add nothing over the underlying
search. A hand-written single-byte counting loop is ~2× faster than all three
of the segment-producing iterators — an honest note that when all you need is
a count and the delimiter is one byte, `memchr`'s dedicated iteration wins.
`split_bytes` is a `memmem` (multi-byte pattern) operation, so it is not the
tool for that micro-task — on these benchmarks, at least.

**Trim** (one SP/HTAB-padded line): aufhebung 8.1 ns vs a handwritten loop
6.0 ns — both negligible.

**Word splitting** (ASCII whitespace, 16 KiB): aufhebung `split_whitespace`
14.4 µs vs `std` `str::split_whitespace` 21.2 µs (~1.5× faster), and far ahead
of `bstr`'s `words()` at 777 µs — though that is an unfair comparison: bstr's
`words()` does Unicode (UAX #29) word segmentation, not ASCII whitespace
splitting, so it is recorded here only to say why it is not compared.

**Chunked word splitting** (same 16 KiB text, handed over in pieces): the
`Words` iterator over [`ChunkedCursor`] reads each word zero-copy across piece
boundaries, vs the naive path that must first flatten the pieces into one
buffer ("the collect is forced by non-contiguity") and then split:

| fragmentation | aufhebung (Pieces) | flatten + std `split_whitespace` |
|---------------|--------------------|----------------------------------|
| contiguous    | 26.8 µs            | 34.4 µs                          |
| 4 KiB         | 26.9 µs            | 34.7 µs                          |
| 64 B          | 29.7 µs            | 35.5 µs                          |
| 8 B           | 30.4 µs            | 35.5 µs                          |

Zero-copy wins at every fragmentation level (1.2–1.3×). Neither row is
anywhere near the 14.4 µs of the flat cursor above: `Pieces`-word iteration
starts ~1.9× above the flat `split_whitespace` (per-piece bookkeeping, even on
one chunk) but then degrades gently — ~13% extra at 8 B pieces (~2,000
chunks) — while the collect path's ~flat rows show that the forced copy, not
the split, is what it pays.