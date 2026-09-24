//! HTTP/1 request-line + header parsing on a contiguous buffer:
//! aufhebung's demo parser vs `httparse` vs a naive (no-memchr) baseline.

mod support;

use support::{
    LARGE_REQUEST, SMALL_REQUEST, parse_request, parse_request_naive, parse_request_noalloc,
};

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

fn bench_http(c: &mut Criterion) {
    let mut group = c.benchmark_group("http/request");
    for (name, wire) in [("small", SMALL_REQUEST), ("large", LARGE_REQUEST)] {
        group.throughput(Throughput::Bytes(wire.len() as u64));

        // the demo parser, verbatim (allocates the headers Vec)
        group.bench_with_input(BenchmarkId::new("aufhebung", name), wire, |b, input| {
            b.iter(|| black_box(parse_request(input)));
        });

        // same parser, fixed-size array: separates scanning from allocator cost
        group.bench_with_input(
            BenchmarkId::new("aufhebung-noalloc", name),
            wire,
            |b, input| {
                b.iter(|| black_box(parse_request_noalloc(input)));
            },
        );

        // the zero-copy HTTP/1 parser hyper is built on
        group.bench_with_input(BenchmarkId::new("httparse", name), wire, |b, input| {
            b.iter(|| {
                let mut headers = [httparse::EMPTY_HEADER; 96];
                let mut req = httparse::Request::new(&mut headers);
                black_box(req.parse(input).is_ok())
            });
        });

        // plain linear scans, no memchr
        group.bench_with_input(BenchmarkId::new("naive", name), wire, |b, input| {
            b.iter(|| black_box(parse_request_naive(input)));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_http);
criterion_main!(benches);
