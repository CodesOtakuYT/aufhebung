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

## Benchmarks

Run with `cargo bench` ([`criterion`](https://docs.rs/criterion), 100 samples
per benchmark). Median times; lower is better.

Machine: Intel Core i5-10400F @ 2.90 GHz (x86_64), rustc 1.100.0-nightly.

### HTTP/1 request + header parsing (contiguous buffer)

`benches/http.rs`. The same request line + header block, parsed by aufhebung's
demo parser (as-written and in a zero-alloc variant with a fixed-size header
array), by [`httparse`](https://docs.rs/httparse) (the zero-copy HTTP/1 parser
hyper is built on), and by a naive parser with no `memchr`.

| input      | aufhebung       | aufhebung-noalloc | httparse | naive |
|------------|-----------------|-------------------|----------|-------|
| small, 94 B | 64.5 ns (1.36 GiB/s) | 52.0 ns (1.68 GiB/s) | 136.8 ns (655 MiB/s) | 70.1 ns (1.25 GiB/s) |
| large, 458 B | 285.4 ns (1.61 GiB/s) | 156.3 ns (2.93 GiB/s) | 291.0 ns (1.57 GiB/s) | 436.3 ns (1.05 GiB/s) |

aufhebung's zero-alloc variant is **1.9–2.6× faster than httparse**. The
as-written demo parser pays for a growable `Vec` of headers (~13 ns small,
~130 ns for 12 headers); reusing the buffer or using the array variant
removes that. The naive parser holds up on the small request — a few dozen
bytes don't reward SIMD — and falls behind by ~1.5× on the header-heavy one.

### HTTP/1 parsing across chunk boundaries

`benches/http_chunked.rs`. The 94-byte request split into 3 or 16 chunks and
parsed with [`ChunkedCursor`], vs the flat parsers over the same bytes joined
into one buffer.

| chunking      | aufhebung |
|---------------|-----------|
| flat (1 chunk)| 63.6 ns   |
| 3 chunks      | 150.8 ns  |
| 16 chunks     | 207.4 ns  |
| flat-httparse (reference) | 124.3 ns |

Cross-chunk scanning costs about 2.4–3.3× the flat cursor but stays faster
than a single `httparse` pass on the same bytes versus the flat numbers above
— and every field remains zero-copy `Pieces` even when it straddles chunks.

### Integer parsing

`benches/int.rs`. `Pieces::parse_integer` vs `btoi`, `atoi`, and `std`
`str::parse::<i64>`, on a single contiguous span (the fair case for the
slice-based parsers).

| input      | aufhebung | btoi | atoi | std |
|------------|-----------|------|------|-----|
| `"42"`     | 19.3 ns   | 2.2 ns | 2.6 ns | 6.8 ns |
| `"-1000"`  | 24.4 ns   | 3.6 ns | 3.6 ns | 9.2 ns |
| `"12345"`  | 26.1 ns   | 4.3 ns | 4.4 ns | 10.3 ns |
| `i64::MAX` | 53.6 ns   | 16.3 ns | 14.3 ns | 18.5 ns |
| `i64::MIN` | 57.9 ns   | 16.4 ns | 14.2 ns | 25.4 ns |

On a contiguous span, the dedicated parsers win by 3–9×: `parse_integer`
carries ~17 ns of cursor/pieces preparation plus ~2 ns per digit, where
`btoi`/`atoi` spend ~1 ns per digit inline. Its edge is fragmentation — an
`i64` whose digits are split across three chunks still parses zero-copy:

| input (i64::MAX over 3 chunks) | time |
|--------------------------------|------|
| aufhebung `parse_integer`      | 58.7 ns |
| flatten to `Vec` then `btoi`   | 95.6 ns |

The flatten-then-parse alternative both copies every byte and is slower.

### Byte-scanning primitives

`benches/scan.rs`.

**Single-byte search** (first comma in 16 KiB of CSV; the needle sits at byte
6): aufhebung 2.15 ns ≈ raw `memchr` 2.03 ns ≈ naive `position` 2.32 ns — the
cursor wrapper is free over the raw search.

**Split on single-byte comma** (counting segments of 16 KiB of CSV):

| aufhebung | memmem `find_iter` | bstr `split_str` | naive count |
|-----------|--------------------|------------------|-------------|
| 15.4 µs   | 16.1 µs            | 16.2 µs          | 7.8 µs      |

aufhebung ≈ raw memmem ≈ bstr: the wrappers add nothing over the underlying
search. A hand-written single-byte counting loop is ~2× faster than all three
of the segment-producing iterators — an honest note that when all you need is
a count and the delimiter is one byte, `memchr`'s dedicated iteration wins.
`split_bytes` is a `memmem` (multi-byte pattern) operation, so it is not the
tool for that micro-task.

**Trim** (one SP/HTAB-padded line): aufhebung 8.1 ns vs a handwritten loop
6.0 ns — both negligible.

**Word splitting** (ASCII whitespace, 16 KiB): aufhebung `split_whitespace`
14.0 µs vs `std` `str::split_whitespace` 20.9 µs (~1.5× faster), and far ahead
of `bstr`'s `words()` at 761 µs — though that is an unfair comparison: bstr's
`words()` does Unicode (UAX #29) word segmentation, not ASCII whitespace
splitting.