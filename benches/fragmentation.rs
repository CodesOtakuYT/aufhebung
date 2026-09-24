//! Streaming-fragmentation benchmark.
//!
//! The same payloads parsed as one contiguous buffer (strategy A: copy every
//! byte into a single `Vec`, then parse) vs parsed directly from the list of
//! chunks (strategy B: zero-copy [`ChunkedCursor`]).
//!
//! Both strategies run the *same* chunked parser, so the only difference is
//! the copy and the chunk bookkeeping. This answers the crate's core question:
//! at what fragmentation level does maintaining contiguity cost more than
//! accepting fragmentation?

mod support;

use support::{
    build_dns_payload, build_http_payload, build_json_payload, build_xml_payload, scan_dns_records,
    scan_http, scan_json_tokens, scan_xml_tokens, split_by_size,
};

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

type Scan = fn(&[&[u8]]) -> usize;

fn bench_format(c: &mut Criterion, name: &str, payload: &[u8], scan: Scan) {
    let levels: [(&str, usize); 4] = [
        ("contiguous", payload.len()),
        ("4KiB", 4096),
        ("64B", 64),
        ("8B", 8),
    ];

    let mut group = c.benchmark_group(format!("fragmentation/{name}"));
    group.throughput(Throughput::Bytes(payload.len() as u64));

    for (level, size) in levels {
        let fragments = split_by_size(payload, size);

        // the token count must not depend on fragmentation
        let joined: Vec<u8> = fragments.iter().flat_map(|c| c.iter().copied()).collect();
        let joined_slice: &[u8] = &joined;
        let single: &[&[u8]] = std::slice::from_ref(&joined_slice);
        assert_eq!(scan(&fragments), scan(single));

        // strategy A: keep contiguity — copy every byte, then parse
        group.bench_with_input(
            BenchmarkId::new("strategyA-concat", level),
            &fragments,
            |b, input| {
                b.iter(|| {
                    let joined: Vec<u8> = input.iter().flat_map(|c| c.iter().copied()).collect();
                    let joined_slice: &[u8] = &joined;
                    let single: &[&[u8]] = std::slice::from_ref(&joined_slice);
                    black_box(scan(single))
                });
            },
        );

        // strategy B: parse the chunks directly — zero copies
        group.bench_with_input(
            BenchmarkId::new("strategyB-chunked", level),
            &fragments,
            |b, input| {
                b.iter(|| black_box(scan(input)));
            },
        );
    }
    group.finish();
}

fn bench_fragmentation(c: &mut Criterion) {
    let http = build_http_payload();
    let json = build_json_payload();
    let xml = build_xml_payload();
    let dns = build_dns_payload();

    bench_format(c, "http", &http, scan_http);
    bench_format(c, "json", &json, scan_json_tokens);
    bench_format(c, "xml", &xml, scan_xml_tokens);
    bench_format(c, "dns", &dns, scan_dns_records);
}

criterion_group!(benches, bench_fragmentation);
criterion_main!(benches);
