//! The same HTTP/1 request parsed across chunk boundaries: the chunked
//! iterator style (3 and 16 chunks) vs the flat parsers over the same bytes as
//! one buffer, which quantifies the cost of chunk bookkeeping.

mod support;

use support::{
    SMALL_REQUEST, parse_request, parse_request_chunked, parse_request_naive, split_chunks,
};

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

fn bench_chunked(c: &mut Criterion) {
    let mut group = c.benchmark_group("http/chunked");
    group.throughput(Throughput::Bytes(SMALL_REQUEST.len() as u64));

    for n in [3usize, 16] {
        let chunks = split_chunks(SMALL_REQUEST, n);
        group.bench_with_input(BenchmarkId::new("aufhebung", n), &chunks, |b, chunks| {
            b.iter(|| black_box(parse_request_chunked(chunks)));
        });
    }

    // the same bytes as a single buffer, for reference
    let joined: Vec<u8> = SMALL_REQUEST.to_vec();
    group.bench_with_input(
        BenchmarkId::new("flat-aufhebung", "joined"),
        &joined[..],
        |b, input| {
            b.iter(|| black_box(parse_request(input)));
        },
    );
    group.bench_with_input(
        BenchmarkId::new("flat-httparse", "joined"),
        &joined[..],
        |b, input| {
            b.iter(|| {
                let mut headers = [httparse::EMPTY_HEADER; 96];
                let mut req = httparse::Request::new(&mut headers);
                black_box(req.parse(input).is_ok())
            });
        },
    );
    group.bench_with_input(
        BenchmarkId::new("flat-naive", "joined"),
        &joined[..],
        |b, input| {
            b.iter(|| black_box(parse_request_naive(input)));
        },
    );

    group.finish();
}

criterion_group!(benches, bench_chunked);
criterion_main!(benches);
