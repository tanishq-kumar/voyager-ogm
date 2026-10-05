use std::mem::size_of;
use voyager_core::Error;
use voyager_core::ast::*;
use voyager_core::builder::QueryBuilder;

#[test]
fn test_node_handle_memory_size() {
    // Verify that NodeHandle is strictly 4 bytes (32-bit u32)
    assert_eq!(size_of::<NodeHandle>(), 4);
    assert_eq!(size_of::<Option<NodeHandle>>(), 8); // or 4 with niche if optimized
}

#[test]
fn test_arena_basic_allocation_and_retrieval() {
    let mut arena = QueryAstArena::new();
    assert!(arena.is_empty());
    assert_eq!(arena.len(), 0);

    let lit_handle = arena.alloc(AstNode::Literal(LiteralValue::Int64(42)));
    assert_eq!(lit_handle.index(), 0);
    assert_eq!(arena.len(), 1);
    assert!(!arena.is_empty());

    let node = arena.get(lit_handle).expect("Node should exist");
    match node {
        AstNode::Literal(LiteralValue::Int64(val)) => assert_eq!(*val, 42),
        _ => panic!("Expected Literal Int64"),
    }
}

#[test]
fn test_arena_invalid_handle_error() {
    let arena = QueryAstArena::new();
    let invalid_handle = NodeHandle(999);
    let result = arena.get(invalid_handle);

    assert!(result.is_err());
    match result.unwrap_err() {
        Error::InvalidNodeHandle(idx) => assert_eq!(idx, 999),
        other => panic!("Unexpected error type: {other:?}"),
    }
}

#[test]
fn test_arena_mutable_update() {
    let mut arena = QueryAstArena::new();
    let handle = arena.alloc(AstNode::Literal(LiteralValue::String("initial".into())));

    if let AstNode::Literal(LiteralValue::String(s)) = arena.get_mut(handle).unwrap() {
        *s = "updated".into();
    }

    match arena.get(handle).unwrap() {
        AstNode::Literal(LiteralValue::String(s)) => assert_eq!(s, "updated"),
        _ => panic!("Expected updated string literal"),
    }
}

#[test]
fn test_arena_clear_and_reuse() {
    let mut arena = QueryAstArena::with_capacity(64);
    arena.alloc(AstNode::Literal(LiteralValue::Bool(true)));
    arena.alloc(AstNode::Literal(LiteralValue::Bool(false)));
    assert_eq!(arena.len(), 2);

    arena.clear();
    assert_eq!(arena.len(), 0);
    assert!(arena.is_empty());

    let new_handle = arena.alloc(AstNode::Literal(LiteralValue::Int64(100)));
    assert_eq!(new_handle.index(), 0);
}

#[test]
fn test_fluent_query_builder_simple_match() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("p"), vec!["Person"])
        .where_property("p", "age", BinaryOp::Gt, 21)
        .select_property("p", "name", Some("person_name"))
        .select_property("p", "age", Some("person_age"))
        .order_by_property("p", "age", true)
        .limit(10)
        .skip(5);

    let (arena, root) = builder.build();
    assert!(!arena.is_empty());

    let root_node = arena.get(root).expect("Root statement must exist");
    if let AstNode::QueryStatement {
        matches,
        return_clause,
        ..
    } = root_node
    {
        assert_eq!(matches.len(), 1);
        let ret_handle = return_clause.expect("Return clause should exist");
        let ret_node = arena.get(ret_handle).unwrap();

        if let AstNode::ReturnClause {
            distinct,
            projections,
            order_by,
            skip,
            limit,
        } = ret_node
        {
            assert!(!distinct);
            assert_eq!(projections.len(), 2);
            assert_eq!(order_by.len(), 1);
            assert_eq!(*limit, Some(10));
            assert_eq!(*skip, Some(5));
        } else {
            panic!("Expected ReturnClause");
        }
    } else {
        panic!("Expected QueryStatement");
    }
}

#[test]
fn test_fluent_query_builder_multi_hop_traversal() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("a"), vec!["User"])
        .where_property("a", "name", BinaryOp::Eq, "Alice")
        .to_edge(
            Direction::Outgoing,
            vec!["KNOWS"],
            Some("r"),
            Some("b"),
            vec!["User"],
        )
        .hops(1, 3)
        .distinct(true)
        .select_property("a", "name", Some("user_name"))
        .select_property("b", "name", Some("friend_name"))
        .select_property_aggregate("b", "id", AggregationFunc::Count, Some("friend_count"));

    let (arena, root) = builder.build();
    assert!(arena.len() >= 6);

    let root_node = arena.get(root).unwrap();
    if let AstNode::QueryStatement { matches, .. } = root_node {
        assert_eq!(matches.len(), 1);
        let match_node = arena.get(matches[0]).unwrap();
        if let AstNode::MatchClause { paths, .. } = match_node {
            assert_eq!(paths.len(), 1);
            let path_chain = arena.get(paths[0]).unwrap();
            if let AstNode::PathChain { edges, .. } = path_chain {
                assert_eq!(edges.len(), 1);
                let edge = arena.get(edges[0]).unwrap();
                if let AstNode::EdgePattern {
                    min_hops,
                    max_hops,
                    direction,
                    ..
                } = edge
                {
                    assert_eq!(*direction, Direction::Outgoing);
                    assert_eq!(*min_hops, Some(1));
                    assert_eq!(*max_hops, Some(3));
                } else {
                    panic!("Expected EdgePattern");
                }
            } else {
                panic!("Expected PathChain");
            }
        }
    }
}

#[test]
fn test_procedure_call_ast_node() {
    let mut arena = QueryAstArena::new();
    let arg1 = arena.alloc(AstNode::Literal(LiteralValue::String("param1".into())));

    let proc_handle = arena.alloc(AstNode::ProcedureCall {
        execution_mode: ExecutionMode::Normal,
        namespace: Some("apoc.path".into()),
        procedure: "subgraphNodes".into(),
        arguments: vec![arg1],
        yield_items: vec!["node".into()],
    });

    let node = arena.get(proc_handle).unwrap();
    if let AstNode::ProcedureCall {
        namespace,
        procedure,
        arguments,
        yield_items,
        ..
    } = node
    {
        assert_eq!(namespace.as_deref(), Some("apoc.path"));
        assert_eq!(procedure, "subgraphNodes");
        assert_eq!(arguments.len(), 1);
        assert_eq!(yield_items, &["node"]);
    } else {
        panic!("Expected ProcedureCall");
    }
}

#[test]
fn test_intuitive_builder_aliases() {
    let mut builder = QueryBuilder::new();
    builder
        .node(Some("p"), vec!["Person"])
        .where_gt("p", "age", 18)
        .where_contains("p", "name", "Smith")
        .out_edge(vec!["ACTED_IN"], Some("r"))
        .node(Some("m"), vec!["Movie"])
        .hops(1, 2)
        .field("p", "name", Some("actor_name"))
        .field("m", "title", Some("movie_title"))
        .order_by_desc("m", "released")
        .limit(25)
        .skip(10);

    let (arena, root) = builder.build();
    assert!(!arena.is_empty());

    let root_node = arena.get(root).unwrap();
    if let AstNode::QueryStatement {
        matches,
        return_clause,
        ..
    } = root_node
    {
        assert_eq!(matches.len(), 1);
        let ret = arena.get(return_clause.unwrap()).unwrap();
        if let AstNode::ReturnClause {
            projections,
            limit,
            skip,
            order_by,
            ..
        } = ret
        {
            assert_eq!(projections.len(), 2);
            assert_eq!(*limit, Some(25));
            assert_eq!(*skip, Some(10));
            assert_eq!(order_by.len(), 1);
            assert!(!order_by[0].1); // descending = false
        }
    }
}

#[test]
fn test_memgraph_gqlalchemy_chaining_style() {
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
    let root_node = arena.get(root).unwrap();

    if let AstNode::QueryStatement {
        matches,
        return_clause,
        ..
    } = root_node
    {
        assert_eq!(matches.len(), 1);
        let match_node = arena.get(matches[0]).unwrap();
        if let AstNode::MatchClause {
            paths,
            where_clause,
            ..
        } = match_node
        {
            assert_eq!(paths.len(), 1);
            assert!(where_clause.is_some());

            let path = arena.get(paths[0]).unwrap();
            if let AstNode::PathChain {
                start_node, edges, ..
            } = path
            {
                let start = arena.get(*start_node).unwrap();
                if let AstNode::NodePattern {
                    variable,
                    label_expression,
                    ..
                } = start
                {
                    assert_eq!(variable.as_deref(), Some("p"));
                    assert_eq!(start.node_labels(), vec!["Person".to_string()]);
                    assert_eq!(
                        label_expression.as_ref(),
                        Some(&LabelExpression::Label("Person".into()))
                    );
                }

                assert_eq!(edges.len(), 2);

                // Edge 1: (p)-[r:ACTED_IN*1..2]->(m:Movie)
                let edge1 = arena.get(edges[0]).unwrap();
                if let AstNode::EdgePattern {
                    direction,
                    label_expression,
                    min_hops,
                    max_hops,
                    target_node,
                    ..
                } = edge1
                {
                    assert_eq!(*direction, Direction::Outgoing);
                    assert_eq!(edge1.edge_types(), vec!["ACTED_IN".to_string()]);
                    assert_eq!(
                        label_expression.as_ref(),
                        Some(&LabelExpression::Label("ACTED_IN".into()))
                    );
                    assert_eq!(*min_hops, Some(1));
                    assert_eq!(*max_hops, Some(2));
                    let target1 = arena.get(*target_node).unwrap();
                    if let AstNode::NodePattern {
                        variable,
                        label_expression,
                        ..
                    } = target1
                    {
                        assert_eq!(variable.as_deref(), Some("m"));
                        assert_eq!(target1.node_labels(), vec!["Movie".to_string()]);
                        assert_eq!(
                            label_expression.as_ref(),
                            Some(&LabelExpression::Label("Movie".into()))
                        );
                    }
                }

                // Edge 2: <-[d_rel:DIRECTED]-(d:Director)
                let edge2 = arena.get(edges[1]).unwrap();
                if let AstNode::EdgePattern {
                    direction,
                    label_expression,
                    target_node,
                    ..
                } = edge2
                {
                    assert_eq!(*direction, Direction::Incoming);
                    assert_eq!(edge2.edge_types(), vec!["DIRECTED".to_string()]);
                    assert_eq!(
                        label_expression.as_ref(),
                        Some(&LabelExpression::Label("DIRECTED".into()))
                    );
                    let target2 = arena.get(*target_node).unwrap();
                    if let AstNode::NodePattern {
                        variable,
                        label_expression,
                        ..
                    } = target2
                    {
                        assert_eq!(variable.as_deref(), Some("d"));
                        assert_eq!(target2.node_labels(), vec!["Director".to_string()]);
                        assert_eq!(
                            label_expression.as_ref(),
                            Some(&LabelExpression::Label("Director".into()))
                        );
                    }
                }
            }
        }

        let ret = arena.get(return_clause.unwrap()).unwrap();
        if let AstNode::ReturnClause {
            projections, limit, ..
        } = ret
        {
            assert_eq!(projections.len(), 3);
            assert_eq!(*limit, Some(10));
        }
    }
}

#[test]
fn test_query_builder_has_mutations() {
    // Pure read query
    let mut read_b = QueryBuilder::new();
    read_b
        .match_node(Some("p"), vec!["Person"])
        .where_eq("p", "age", 30)
        .select_property("p", "name", None::<String>);
    assert!(!read_b.has_mutations());

    // Create clause
    let mut create_b = QueryBuilder::new();
    create_b.create().node(Some("n"), vec!["Node"]);
    assert!(create_b.has_mutations());

    // Merge clause
    let mut merge_b = QueryBuilder::new();
    merge_b.merge().node(Some("n"), vec!["Node"]);
    assert!(merge_b.has_mutations());

    // Set clause
    let mut set_b = QueryBuilder::new();
    set_b
        .r#match()
        .node(Some("n"), vec!["Node"])
        .set_property("n", "active", true);
    assert!(set_b.has_mutations());

    // Delete clause
    let mut del_b = QueryBuilder::new();
    del_b
        .r#match()
        .node(Some("n"), vec!["Node"])
        .delete(vec!["n"]);
    assert!(del_b.has_mutations());

    // Detach delete clause
    let mut detach_b = QueryBuilder::new();
    detach_b
        .r#match()
        .node(Some("n"), vec!["Node"])
        .detach_delete(vec!["n"]);
    assert!(detach_b.has_mutations());

    // Remove property clause
    let mut rem_b = QueryBuilder::new();
    rem_b
        .r#match()
        .node(Some("n"), vec!["Node"])
        .remove_property("n", "score");
    assert!(rem_b.has_mutations());
}

#[test]
fn test_query_builder_import_match_patterns_deduplication() {
    use std::collections::HashSet;

    let mut p1 = QueryBuilder::new();
    p1.r#match()
        .node(Some("p"), vec!["Person"])
        .to(vec!["WORKS_AT"], None::<String>)
        .node(Some("c"), vec!["Company"]);

    let mut p2 = QueryBuilder::new();
    p2.r#match()
        .node(Some("p"), vec!["Person"])
        .to(vec!["MANAGES"], None::<String>)
        .node(Some("c"), vec!["Company"]);

    let mut main_b = QueryBuilder::new();
    let mut seen_vars = HashSet::new();

    main_b.import_match_patterns(&mut p1, &mut seen_vars);
    assert!(seen_vars.contains("p"));
    assert!(seen_vars.contains("c"));

    main_b.import_match_patterns(&mut p2, &mut seen_vars);

    let (arena, root) = main_b.build();
    let root_node = arena.get(root).unwrap();
    if let AstNode::QueryStatement { matches, .. } = root_node {
        assert_eq!(matches.len(), 1);
        let match_node = arena.get(matches[0]).unwrap();
        if let AstNode::MatchClause { paths, .. } = match_node {
            assert_eq!(paths.len(), 2);

            // Path 1 should have labels
            if let AstNode::PathChain {
                start_node, edges, ..
            } = arena.get(paths[0]).unwrap()
            {
                let start = arena.get(*start_node).unwrap();
                if let AstNode::NodePattern { variable, .. } = start {
                    assert_eq!(variable.as_deref(), Some("p"));
                    assert_eq!(start.node_labels(), vec!["Person".to_string()]);
                }
                let edge = arena.get(edges[0]).unwrap();
                if let AstNode::EdgePattern { target_node, .. } = edge {
                    let target = arena.get(*target_node).unwrap();
                    if let AstNode::NodePattern { variable, .. } = target {
                        assert_eq!(variable.as_deref(), Some("c"));
                        assert_eq!(target.node_labels(), vec!["Company".to_string()]);
                    }
                }
            }

            // Path 2 should have NO labels because p and c were in seen_vars
            if let AstNode::PathChain {
                start_node, edges, ..
            } = arena.get(paths[1]).unwrap()
            {
                let start = arena.get(*start_node).unwrap();
                if let AstNode::NodePattern {
                    variable,
                    label_expression,
                    ..
                } = start
                {
                    assert_eq!(variable.as_deref(), Some("p"));
                    assert!(
                        label_expression.is_none(),
                        "Expected empty label_expression for reused variable p, got {label_expression:?}"
                    );
                    assert!(
                        start.node_labels().is_empty(),
                        "Expected empty labels for reused variable p"
                    );
                }
                let edge = arena.get(edges[0]).unwrap();
                if let AstNode::EdgePattern { target_node, .. } = edge {
                    let target = arena.get(*target_node).unwrap();
                    if let AstNode::NodePattern {
                        variable,
                        label_expression,
                        ..
                    } = target
                    {
                        assert_eq!(variable.as_deref(), Some("c"));
                        assert!(
                            label_expression.is_none(),
                            "Expected empty label_expression for reused variable c, got {label_expression:?}"
                        );
                        assert!(
                            target.node_labels().is_empty(),
                            "Expected empty labels for reused variable c"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn test_label_expression_parsing_and_combinators() {
    // 1. Single label
    let expr = LabelExpression::parse("Person").unwrap();
    assert_eq!(expr, LabelExpression::Label("Person".into()));
    assert_eq!(expr.collect_labels(), vec!["Person".to_string()]);
    assert!(expr.is_simple_conjunction());
    assert_eq!(expr.to_conjunction_labels(), Some(vec!["Person".into()]));

    // 2. Wildcard
    assert_eq!(
        LabelExpression::parse("%").unwrap(),
        LabelExpression::Wildcard
    );
    assert_eq!(
        LabelExpression::parse("*").unwrap(),
        LabelExpression::Wildcard
    );

    // 3. Negation
    let not_expr = LabelExpression::parse("!Bot").unwrap();
    assert_eq!(
        not_expr,
        LabelExpression::Not(Box::new(LabelExpression::Label("Bot".into())))
    );
    assert_eq!(not_expr.collect_labels(), vec!["Bot".to_string()]);
    assert!(!not_expr.is_simple_conjunction());

    // 4. Conjunction
    let and_expr = LabelExpression::parse("Person & Developer").unwrap();
    assert_eq!(
        and_expr,
        LabelExpression::And(
            Box::new(LabelExpression::Label("Person".into())),
            Box::new(LabelExpression::Label("Developer".into()))
        )
    );
    assert_eq!(
        and_expr.to_conjunction_labels(),
        Some(vec!["Person".into(), "Developer".into()])
    );

    // 5. Disjunction
    let or_expr = LabelExpression::parse("Teacher | Student").unwrap();
    assert_eq!(
        or_expr,
        LabelExpression::Or(
            Box::new(LabelExpression::Label("Teacher".into())),
            Box::new(LabelExpression::Label("Student".into()))
        )
    );
    assert_eq!(
        or_expr.to_disjunction_labels(),
        Some(vec!["Teacher".into(), "Student".into()])
    );

    // 6. Deeply nested: (A & (B | !C))
    let nested = LabelExpression::parse("(A & (B | !C))").unwrap();
    let expected = LabelExpression::and(
        LabelExpression::label("A"),
        LabelExpression::or(
            LabelExpression::label("B"),
            LabelExpression::not(LabelExpression::label("C")),
        ),
    );
    assert_eq!(nested, expected);
    assert_eq!(nested.collect_labels(), vec!["A", "B", "C"]);

    // 7. Heuristic parsing & helpers
    let from_labels = LabelExpression::from_labels(["Person", "Developer"]).unwrap();
    assert_eq!(from_labels, and_expr);

    let from_edges = LabelExpression::from_edge_types(["Teacher", "Student"]).unwrap();
    assert_eq!(from_edges, or_expr);
}

#[test]
fn test_label_expression_ast_dialects_emission() {
    use voyager_core::emitters::{CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
    use voyager_core::visitor::AstVisitor;

    // Test 1: Complex nested node expression: (Person | Company) & !Inactive
    let complex_expr = LabelExpression::parse("(Person | Company) & !Inactive").unwrap();
    let mut b = QueryBuilder::new();
    let n_id = b.ident("n");
    b.r#match()
        .node_expr(Some("n"), complex_expr)
        .r#return()
        .select_expr(n_id, None::<String>);

    let (arena, root) = b.build();

    let mut cypher = CypherEmitter::new();
    assert_eq!(
        cypher.visit_query(&arena, root).unwrap().statement,
        "MATCH (n:(Person|Company)&!Inactive) RETURN n"
    );

    let mut gql = IsoGqlEmitter::new();
    assert_eq!(
        gql.visit_query(&arena, root).unwrap().statement,
        "MATCH (n:(Person|Company)&!Inactive) RETURN n"
    );

    let mut pgq = SqlPgqEmitter::new("voyager_graph");
    assert_eq!(
        pgq.visit_query(&arena, root).unwrap().statement,
        "SELECT * FROM GRAPH_TABLE (voyager_graph MATCH (n IS (Person | Company) & !Inactive) COLUMNS (n))"
    );

    // Test 2: Edge traversal with disjunction: -[r:KNOWS|FOLLOWS]->
    let mut b2 = QueryBuilder::new();
    let a_id = b2.ident("a");
    b2.r#match()
        .node(Some("a"), vec!["User"])
        .to(vec!["KNOWS", "FOLLOWS"], Some("r"))
        .node(Some("b"), vec!["User"])
        .r#return()
        .select_expr(a_id, None::<String>);

    let (arena2, root2) = b2.build();

    let mut cypher2 = CypherEmitter::new();
    assert_eq!(
        cypher2.visit_query(&arena2, root2).unwrap().statement,
        "MATCH (a:User)-[r:KNOWS|FOLLOWS]->(b:User) RETURN a"
    );

    let mut gql2 = IsoGqlEmitter::new();
    assert_eq!(
        gql2.visit_query(&arena2, root2).unwrap().statement,
        "MATCH (a:User)-[r:KNOWS|FOLLOWS]->(b:User) RETURN a"
    );

    let mut pgq2 = SqlPgqEmitter::new("voyager_graph");
    assert_eq!(
        pgq2.visit_query(&arena2, root2).unwrap().statement,
        "SELECT * FROM GRAPH_TABLE (voyager_graph MATCH (a IS User) -[r IS KNOWS | FOLLOWS]-> (b IS User) COLUMNS (a))"
    );
}

#[test]
fn test_label_expression_topology_extraction() {
    use voyager_core::topology::GraphTopology;

    let query = "MATCH (a:Person)-[r:KNOWS|FOLLOWS]->(b:Company) RETURN a";
    let topo = GraphTopology::from_query_str(query);

    let a_node = topo.nodes.iter().find(|n| n.id == "a").unwrap();
    assert_eq!(a_node.labels, vec!["Person"]);
    assert_eq!(
        a_node.label_expression,
        Some(LabelExpression::Label("Person".into()))
    );

    let b_node = topo.nodes.iter().find(|n| n.id == "b").unwrap();
    assert_eq!(b_node.labels, vec!["Company"]);
    assert_eq!(
        b_node.label_expression,
        Some(LabelExpression::Label("Company".into()))
    );

    let edge = &topo.edges[0];
    assert_eq!(edge.types, vec!["KNOWS", "FOLLOWS"]);
    assert_eq!(
        edge.label_expression,
        Some(LabelExpression::or(
            LabelExpression::label("KNOWS"),
            LabelExpression::label("FOLLOWS")
        ))
    );
}

#[test]
fn test_label_expression_serde_roundtrip() {
    let expr = LabelExpression::parse("(A & (B | !C))").unwrap();
    let json = serde_json::to_string(&expr).expect("Serialization failed");
    let deserialized: LabelExpression =
        serde_json::from_str(&json).expect("Deserialization failed");
    assert_eq!(expr, deserialized);
}
