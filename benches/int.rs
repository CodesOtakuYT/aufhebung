//! Integer parsing from byte spans: aufhebung's `Pieces::parse_integer` vs
//! `btoi`, `atoi`, and `std`'s `str::parse`.

use aufhebung::ChunkedCursor;
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

/// Parse `input` as one contiguous span (a single piece), which is the fair
/// comparison against the slice-based parsers.
fn parse_integer_aufhebung(input: &[u8]) -> Option<i64> {
    let mut c = ChunkedCursor::new(std::slice::from_ref(&input));
    c.take_rest().parse_integer()
}

fn bench_int(c: &mut Criterion) {
    let mut group = c.benchmark_group("int/parse");
    let cases: [(&str, &[u8]); 5] = [
        ("two_digit", b"42"),
        ("neg_four_digit", b"-1000"),
        ("five_digit", b"12345"),
        ("i64_max", b"9223372036854775807"),
        ("i64_min", b"-9223372036854775808"),
    ];
    for (name, digits) in cases {
        group.bench_with_input(BenchmarkId::new("aufhebung", name), digits, |b, input| {
            b.iter(|| black_box(parse_integer_aufhebung(input)));
        });
        group.bench_with_input(BenchmarkId::new("btoi", name), digits, |b, input| {
            b.iter(|| black_box(btoi::btoi::<i64>(input).ok()));
        });
        group.bench_with_input(BenchmarkId::new("atoi", name), digits, |b, input| {
            b.iter(|| {
                black_box(
                    <i64 as atoi::FromRadix10SignedChecked>::from_radix_10_signed_checked(input).0,
                )
            });
        });
        group.bench_with_input(BenchmarkId::new("std", name), digits, |b, input| {
            b.iter(|| {
                let s = std::str::from_utf8(input).unwrap();
                black_box(s.parse::<i64>().ok())
            });
        });
    }
    group.finish();

    // The span split across three pieces (one empty): only aufhebung reads it
    // zero-copy; the alternative flattens the pieces into an owned buffer
    // first, which is the cost the chunked design avoids.
    let mut group = c.benchmark_group("int/multi_piece");
    let pieces: [&[u8]; 3] = [&b"9223372036854"[..], &b"775807"[..], &b""[..]];
    group.bench_with_input(
        BenchmarkId::new("aufhebung", "i64_max"),
        &pieces,
        |b, input| {
            b.iter(|| {
                let mut c = ChunkedCursor::new(input);
                black_box(c.take_rest().parse_integer())
            });
        },
    );
    group.bench_with_input(
        BenchmarkId::new("flatten_then_btoi", "i64_max"),
        &pieces,
        |b, input| {
            b.iter(|| {
                let joined: Vec<u8> = input.iter().flat_map(|c| c.iter().copied()).collect();
                black_box(btoi::btoi::<i64>(&joined).ok())
            });
        },
    );
    group.finish();
}

criterion_group!(benches, bench_int);
criterion_main!(benches);
