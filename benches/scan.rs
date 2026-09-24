//! Byte-scanning primitives: aufhebung's memchr-backed cursor ops vs raw
//! `memchr`/`memmem`, vs `bstr` (itself memchr-based), vs plain linear scans.
//! These show that the cursor wrappers cost nothing over the raw searches.

use aufhebung::ByteSliceCursor;
use bstr::ByteSlice;
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use memchr::{memchr, memmem};

/// Find the first comma (16 KiB of repeating CSV text).
fn bench_find_first(c: &mut Criterion, csv: &[u8]) {
    let mut group = c.benchmark_group("scan/find_first");
    group.throughput(Throughput::Bytes(csv.len() as u64));
    group.bench_with_input(BenchmarkId::new("aufhebung", "csv"), csv, |b, input| {
        b.iter(|| {
            let mut s: &[u8] = input;
            black_box(s.take_until_byte(b',').len())
        });
    });
    group.bench_with_input(BenchmarkId::new("memchr", "csv"), csv, |b, input| {
        b.iter(|| black_box(memchr(b',', input).unwrap()));
    });
    group.bench_with_input(BenchmarkId::new("naive", "csv"), csv, |b, input| {
        b.iter(|| black_box(input.iter().position(|&b| b == b',').unwrap()));
    });
    group.finish();
}

/// Split the CSV on every comma.
fn bench_split(c: &mut Criterion, csv: &[u8]) {
    let mut group = c.benchmark_group("scan/split_comma");
    group.throughput(Throughput::Bytes(csv.len() as u64));
    group.bench_with_input(BenchmarkId::new("aufhebung", "csv"), csv, |b, input| {
        b.iter(|| black_box(input.split_bytes(b",").count()));
    });
    group.bench_with_input(BenchmarkId::new("memmem", "csv"), csv, |b, input| {
        b.iter(|| black_box(memmem::find_iter(input, b",").count()));
    });
    group.bench_with_input(BenchmarkId::new("bstr", "csv"), csv, |b, input| {
        b.iter(|| black_box(input.split_str(b",").count()));
    });
    group.bench_with_input(BenchmarkId::new("naive", "csv"), csv, |b, input| {
        b.iter(|| black_box(input.iter().filter(|&&b| b == b',').count()));
    });
    group.finish();
}

/// Trim one SP/HTAB-padded line. (bstr's `trim`/`trim_with` are Unicode
/// char-based operations, not byte-set trims, so there is no fair bstr
/// comparison here.)
fn bench_trim(c: &mut Criterion, padded: &[u8]) {
    let mut group = c.benchmark_group("scan/trim");
    group.bench_with_input(
        BenchmarkId::new("aufhebung", "padded"),
        padded,
        |b, input| {
            b.iter(|| {
                let s: &[u8] = input;
                black_box(<&[u8] as aufhebung::ByteSliceCursor<'_>>::trim(&s, b" \t").len())
            });
        },
    );
    group.bench_with_input(BenchmarkId::new("naive", "padded"), padded, |b, input| {
        b.iter(|| {
            let mut start = 0;
            let mut end = input.len();
            while start < end && (input[start] == b' ' || input[start] == b'\t') {
                start += 1;
            }
            while end > start && (input[end - 1] == b' ' || input[end - 1] == b'\t') {
                end -= 1;
            }
            black_box(end - start)
        });
    });
    group.finish();
}

/// ASCII-whitespace word splitting over 16 KiB of text.
fn bench_words(c: &mut Criterion, text: &[u8]) {
    let mut group = c.benchmark_group("scan/words");
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_with_input(BenchmarkId::new("aufhebung", "text"), text, |b, input| {
        b.iter(|| black_box(input.split_whitespace().count()));
    });
    group.bench_with_input(BenchmarkId::new("bstr", "text"), text, |b, input| {
        b.iter(|| black_box(input.words().count()));
    });
    group.bench_with_input(BenchmarkId::new("std", "text"), text, |b, input| {
        b.iter(|| {
            black_box(
                std::str::from_utf8(input)
                    .unwrap()
                    .split_whitespace()
                    .count(),
            )
        });
    });
    group.finish();
}

fn bench_scan(c: &mut Criterion) {
    let csv_line: &[u8] = b"alpha,beta,gamma,delta,epsilon,zeta,eta,theta,iota,kappa\n";
    let csv: Vec<u8> = csv_line.iter().copied().cycle().take(16384).collect();

    let mut padded: Vec<u8> = Vec::with_capacity(256);
    padded.extend_from_slice(b"  \t \t");
    padded.extend_from_slice(&csv[..200]);
    padded.extend_from_slice(b" \t  \t ");

    let words_line: &[u8] = b"  hello \t world!\nfoo bar   baz qux\n";
    let text: Vec<u8> = words_line.iter().copied().cycle().take(16384).collect();

    bench_find_first(c, &csv);
    bench_split(c, &csv);
    bench_trim(c, &padded);
    bench_words(c, &text);
}

criterion_group!(benches, bench_scan);
criterion_main!(benches);
