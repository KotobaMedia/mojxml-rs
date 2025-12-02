use criterion::{Criterion, criterion_group, criterion_main};
use std::time::Duration;

const XML_SAMPLE: &str = include_str!("../../../testdata/46505-3411-56.xml");

fn roxmltree_parse() {
    let _ = mojxml_parser::parse_xml_content("46505-3411-56.xml", XML_SAMPLE, &Default::default())
        .unwrap();
}

fn bench_main(c: &mut Criterion) {
    let mut group = c.benchmark_group("XML Parsing");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(15));
    group.warm_up_time(Duration::from_secs(5));

    group.bench_function("roxmltree", |b| b.iter(roxmltree_parse));

    group.finish();
}

criterion_group!(benches, bench_main);
criterion_main!(benches);
