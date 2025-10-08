use criterion::{Criterion, criterion_group, criterion_main};
use std::time::Duration;

fn roxmltree_parse() {
    let data = include_str!("../../../testdata/46505-3411-56.xml").to_string();
    let _ = mojxml_parser::parse_xml_content(
        "46505-3411-56.xml",
        &data,
        &Default::default(),
    )
    .unwrap();
}

fn bench_main(c: &mut Criterion) {
    let mut group = c.benchmark_group("XML Parsing");
    group.warm_up_time(Duration::from_secs(20));

    group.bench_function("roxmltree", |b| b.iter(roxmltree_parse));

    group.finish();
}

criterion_group!(benches, bench_main);
criterion_main!(benches);
