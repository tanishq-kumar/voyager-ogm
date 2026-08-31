//! Tests for Two-Layer Rollback Transaction and Unit-of-Work Management.

use std::collections::BTreeMap;
use voyager_core::ast::{AstNode, LiteralValue, QueryAstArena};
use voyager_core::transaction::{EntityMutation, Transaction, TransactionState, UnitOfWork};

#[test]
fn test_unit_of_work_entity_mutations_recording() {
    let mut uow = UnitOfWork::new();

    let mut props = BTreeMap::new();
    props.insert("name".into(), LiteralValue::String("Alice".into()));
    props.insert("age".into(), LiteralValue::Int64(30));

    let node_id = uow.register_new_node(vec!["Person".into()], props.clone());
    assert_eq!(node_id, 1);
    assert_eq!(uow.len(), 1);

    let mut new_props = props.clone();
    new_props.insert("age".into(), LiteralValue::Int64(31));
    uow.register_dirty_node(node_id, props.clone(), new_props);
    assert_eq!(uow.len(), 2);

    let edge_id = uow.register_new_edge(1, 2, "KNOWS", BTreeMap::new());
    assert_eq!(edge_id, 2);
    assert_eq!(uow.len(), 3);

    let mutations = uow.pending_mutations();
    match &mutations[0] {
        EntityMutation::InsertNode {
            temp_id, labels, ..
        } => {
            assert_eq!(*temp_id, 1);
            assert_eq!(labels, &["Person"]);
        }
        _ => panic!("Expected InsertNode mutation"),
    }
}

#[test]
fn test_transaction_commit_lifecycle() {
    let mut arena = QueryAstArena::new();
    let mut uow = UnitOfWork::new();

    let mut tx = Transaction::new(101, &uow, &arena);
    assert_eq!(tx.id(), 101);
    assert!(tx.is_active());
    assert_eq!(tx.state(), TransactionState::Active);

    // Perform allocations and mutations during transaction
    let var_handle = arena.alloc(AstNode::Identifier("p".into()));
    let _h1 = arena.alloc(AstNode::PropertyAccess {
        target: var_handle,
        property: "age".into(),
    });
    uow.register_new_node(vec!["User".into()], BTreeMap::new());

    assert_eq!(arena.len(), 2);
    assert_eq!(uow.len(), 1);

    // Commit transaction
    assert!(tx.commit(&mut uow).is_ok());
    assert_eq!(tx.state(), TransactionState::Committed);
    assert!(!tx.is_active());
    assert_eq!(uow.len(), 0); // Unit of work cleared on commit
    assert_eq!(arena.len(), 2); // Arena allocations persisted

    // Cannot commit or mutate committed transaction
    assert!(tx.commit(&mut uow).is_err());
    assert!(tx.rollback(&mut uow, &mut arena).is_err());
}

#[test]
fn test_transaction_rollback_and_arena_restoration() {
    let mut arena = QueryAstArena::new();
    let mut uow = UnitOfWork::new();

    // Baseline allocation before transaction
    let var_handle = arena.alloc(AstNode::Identifier("base".into()));
    let baseline_handle = arena.alloc(AstNode::PropertyAccess {
        target: var_handle,
        property: "id".into(),
    });
    assert_eq!(arena.len(), 2);

    let mut tx = Transaction::new(102, &uow, &arena);

    // Mutate state within transaction
    let _dirty_handle = arena.alloc(AstNode::Literal(LiteralValue::String("temp".into())));
    let _dirty_node = uow.register_new_node(vec!["TempNode".into()], BTreeMap::new());

    assert_eq!(arena.len(), 3);
    assert_eq!(uow.len(), 1);

    // Rollback transaction (e.g. database error occurred)
    assert!(tx.rollback(&mut uow, &mut arena).is_ok());
    assert_eq!(tx.state(), TransactionState::RolledBack);
    assert!(!tx.is_active());

    // In-memory arena and unit of work are rolled back to baseline!
    assert_eq!(arena.len(), 2);
    assert_eq!(uow.len(), 0);
    assert!(arena.get(baseline_handle).is_ok());
}

#[test]
fn test_transaction_savepoints_and_partial_rollback() {
    let mut arena = QueryAstArena::new();
    let mut uow = UnitOfWork::new();

    let mut tx = Transaction::new(103, &uow, &arena);

    // Step 1: Initial mutation
    let _n1 = uow.register_new_node(vec!["Node1".into()], BTreeMap::new());
    let _h1 = arena.alloc(AstNode::Literal(LiteralValue::Int64(100)));
    assert_eq!(uow.len(), 1);
    assert_eq!(arena.len(), 1);

    // Step 2: Create Savepoint "sp1"
    assert!(tx.savepoint("sp1", &uow, &arena).is_ok());

    // Step 3: Mutate further after savepoint
    let _n2 = uow.register_new_node(vec!["Node2".into()], BTreeMap::new());
    let _h2 = arena.alloc(AstNode::Literal(LiteralValue::Int64(200)));
    assert_eq!(uow.len(), 2);
    assert_eq!(arena.len(), 2);

    // Step 4: Rollback to savepoint "sp1"
    assert!(
        tx.rollback_to_savepoint("sp1", &mut uow, &mut arena)
            .is_ok()
    );

    // State after sp1 is discarded; state before sp1 is preserved!
    assert_eq!(uow.len(), 1);
    assert_eq!(arena.len(), 1);
    assert!(tx.is_active()); // Transaction remains active

    // Step 5: Successful commit of remaining transaction
    assert!(tx.commit(&mut uow).is_ok());
    assert_eq!(tx.state(), TransactionState::Committed);
    assert_eq!(arena.len(), 1);
}

#[test]
fn test_chaos_repeated_aborted_transactions_zero_leak() {
    let mut arena = QueryAstArena::new();
    let mut uow = UnitOfWork::new();

    // Run 500 consecutive failed transactions with allocations and savepoints
    for i in 0..500 {
        let mut tx = Transaction::new(i, &uow, &arena);
        for j in 0..10 {
            arena.alloc(AstNode::Literal(LiteralValue::Int64(j)));
            uow.register_new_node(vec!["LeakTest".into()], BTreeMap::new());
        }

        tx.savepoint("temp_sp", &uow, &arena).unwrap();
        arena.alloc(AstNode::Literal(LiteralValue::String("temp".into())));

        // Abort transaction
        tx.rollback(&mut uow, &mut arena).unwrap();
        assert_eq!(tx.state(), TransactionState::RolledBack);
    }

    // Zero memory leak / arena size strictly preserved at 0
    assert_eq!(arena.len(), 0);
    assert_eq!(uow.len(), 0);
}
