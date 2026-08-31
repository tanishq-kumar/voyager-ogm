use criterion::{Criterion, black_box, criterion_group, criterion_main};
use voyager_core::ast::*;
use voyager_core::builder::QueryBuilder;

fn benchmark_arena_allocation(c: &mut Criterion) {
    c.bench_function("arena_alloc_single_node", |b| {
        let mut arena = QueryAstArena::with_capacity(1000);
        b.iter(|| {
            arena.clear();
            for _ in 0..100 {
                let handle = arena.alloc(AstNode::Literal(LiteralValue::Int64(black_box(42))));
                black_box(handle);
            }
        });
    });
}

fn benchmark_fluent_ast_construction(c: &mut Criterion) {
    c.bench_function("fluent_query_builder_10_hop", |b| {
        b.iter(|| {
            let mut builder = QueryBuilder::new();
            builder
                .match_node(Some("u0"), vec!["Person"])
                .where_property("u0", "age", BinaryOp::Gt, 21);

            for i in 1..=10 {
                let next_var = format!("u{i}");
                builder
                    .to(vec!["KNOWS"], Some(format!("r{i}")))
                    .node(Some(next_var.clone()), vec!["Person"])
                    .where_property(next_var, "age", BinaryOp::Gte, 18);
            }

            builder
                .select_property("u0", "name", Some("start_name"))
                .select_property("u10", "name", Some("end_name"))
                .distinct(true)
                .limit(50);

            let (arena, root) = builder.build();
            black_box((arena, root));
        });
    });
}

criterion_group!(
    benches,
    benchmark_arena_allocation,
    benchmark_fluent_ast_construction
);
criterion_main!(benches);
