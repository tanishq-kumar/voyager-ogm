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
            if let AstNode::PathChain { start_node, edges } = path {
                let start = arena.get(*start_node).unwrap();
                if let AstNode::NodePattern {
                    variable, labels, ..
                } = start
                {
                    assert_eq!(variable.as_deref(), Some("p"));
                    assert_eq!(labels, &["Person"]);
                }

                assert_eq!(edges.len(), 2);

                // Edge 1: (p)-[r:ACTED_IN*1..2]->(m:Movie)
                let edge1 = arena.get(edges[0]).unwrap();
                if let AstNode::EdgePattern {
                    direction,
                    edge_types,
                    min_hops,
                    max_hops,
                    target_node,
                    ..
                } = edge1
                {
                    assert_eq!(*direction, Direction::Outgoing);
                    assert_eq!(edge_types, &["ACTED_IN"]);
                    assert_eq!(*min_hops, Some(1));
                    assert_eq!(*max_hops, Some(2));
                    let target1 = arena.get(*target_node).unwrap();
                    if let AstNode::NodePattern {
                        variable, labels, ..
                    } = target1
                    {
                        assert_eq!(variable.as_deref(), Some("m"));
                        assert_eq!(labels, &["Movie"]);
                    }
                }

                // Edge 2: <-[d_rel:DIRECTED]-(d:Director)
                let edge2 = arena.get(edges[1]).unwrap();
                if let AstNode::EdgePattern {
                    direction,
                    edge_types,
                    target_node,
                    ..
                } = edge2
                {
                    assert_eq!(*direction, Direction::Incoming);
                    assert_eq!(edge_types, &["DIRECTED"]);
                    let target2 = arena.get(*target_node).unwrap();
                    if let AstNode::NodePattern {
                        variable, labels, ..
                    } = target2
                    {
                        assert_eq!(variable.as_deref(), Some("d"));
                        assert_eq!(labels, &["Director"]);
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
