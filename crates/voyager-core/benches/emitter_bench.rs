use criterion::{Criterion, black_box, criterion_group, criterion_main};
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_core::visitor::AstVisitor;

fn benchmark_emitters(c: &mut Criterion) {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .to(vec!["ACTED_IN"], Some("r"))
        .hops(1, 2)
        .node(Some("m"), vec!["Movie"])
        .from(vec!["DIRECTED"], Some("d_rel"))
        .node(Some("d"), vec!["Director"])
        .where_gt("p", "age", 21)
        .where_eq("m", "released", 1999)
        .r#return()
        .field("p", "name", Some("actor"))
        .field("m", "title", Some("movie"))
        .field("d", "name", Some("director"))
        .order_by_asc("p", "name")
        .limit(10);

    let (arena, root) = builder.build();

    c.bench_function("emit_cypher_traversal", |b| {
        let mut emitter = CypherEmitter::new();
        b.iter(|| {
            let res = emitter
                .visit_query(black_box(&arena), black_box(root))
                .unwrap();
            black_box(res);
        });
    });

    c.bench_function("emit_sql_pgq_traversal", |b| {
        let mut emitter = SqlPgqEmitter::new("movies_graph");
        b.iter(|| {
            let res = emitter
                .visit_query(black_box(&arena), black_box(root))
                .unwrap();
            black_box(res);
        });
    });

    c.bench_function("emit_iso_gql_traversal", |b| {
        let mut emitter = IsoGqlEmitter::new();
        b.iter(|| {
            let res = emitter
                .visit_query(black_box(&arena), black_box(root))
                .unwrap();
            black_box(res);
        });
    });
}

criterion_group!(benches, benchmark_emitters);
criterion_main!(benches);
