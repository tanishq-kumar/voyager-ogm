//! Voyager OGM: Transactions, Savepoints & In-Memory Unit of Work
//!
//! Teaches:
//! - Transaction lifecycle management (`Transaction`, `TransactionState`)
//! - In-memory Unit of Work tracking entity insertions, updates, and deletions
//! - Capturing checkpoints across AST arenas and mutation logs
//! - Named savepoints (`tx.savepoint()`) for fine-grained sub-operation rollbacks
//! - Rolling back to savepoints while keeping the parent transaction active
//! - Clean transaction commit (`tx.commit()`) and full rollback (`tx.rollback()`)
//!
//! Prerequisites:
//! - Engine: None (offline / in-memory unit of work)
//! - Crates: voyager-ogm
//!
//! Run with:
//! `cargo run -p voyager-ogm --example 11_transactions_and_unit_of_work`

use std::collections::BTreeMap;
use voyager_ogm::ast::{LiteralValue, QueryAstArena};
use voyager_ogm::transaction::{Transaction, TransactionState, UnitOfWork};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Voyager OGM (Rust) - Transactions & Unit of Work ===\n");

    let mut arena = QueryAstArena::new();
    let mut uow = UnitOfWork::new();

    // -----------------------------------------------------------------------
    // 1. Transaction Initialization
    // -----------------------------------------------------------------------
    println!("--- [1] Transaction Start ---");
    let mut tx = Transaction::new(1001, &uow, &arena);
    println!(
        "Transaction #{} started: state = {:?}, is_active = {}",
        tx.id(),
        tx.state(),
        tx.is_active()
    );
    assert_eq!(tx.state(), TransactionState::Active);

    // -----------------------------------------------------------------------
    // 2. Unit of Work: Registering Entity Mutations
    // -----------------------------------------------------------------------
    println!("\n--- [2] Unit of Work Entity Registration ---");
    let mut alice_props = BTreeMap::new();
    alice_props.insert(
        "name".to_string(),
        LiteralValue::String("Alice".to_string()),
    );
    alice_props.insert("balance".to_string(), LiteralValue::Float64(1500.0));
    alice_props.insert(
        "tier".to_string(),
        LiteralValue::String("Standard".to_string()),
    );
    uow.register_new_node(vec!["Customer".to_string()], alice_props);

    let mut bob_props = BTreeMap::new();
    bob_props.insert("name".to_string(), LiteralValue::String("Bob".to_string()));
    bob_props.insert("balance".to_string(), LiteralValue::Float64(500.0));
    uow.register_new_node(vec!["Customer".to_string()], bob_props);

    println!(
        "Registered {} initial pending mutations in Unit of Work.",
        uow.len()
    );
    assert_eq!(uow.len(), 2);

    // -----------------------------------------------------------------------
    // 3. Savepoint Creation & Nested Sub-Operation Rollback
    // -----------------------------------------------------------------------
    println!("\n--- [3] Savepoint & Partial Rollback ---");
    tx.savepoint("before_risky_op", &uow, &arena)?;
    println!("Created savepoint 'before_risky_op'.");

    // Perform speculative mutation inside savepoint scope
    let mut old_alice = BTreeMap::new();
    old_alice.insert("balance".to_string(), LiteralValue::Float64(1500.0));
    let mut new_alice = BTreeMap::new();
    new_alice.insert("balance".to_string(), LiteralValue::Float64(0.0));
    uow.register_dirty_node(1, old_alice, new_alice);

    println!("Speculative mutation added. UoW mutations: {}", uow.len());
    assert_eq!(uow.len(), 3);

    // Rollback speculative mutation to savepoint
    println!("Simulating sub-operation failure: rolling back to 'before_risky_op'...");
    tx.rollback_to_savepoint("before_risky_op", &mut uow, &mut arena)?;
    println!(
        "Savepoint rolled back. UoW mutations restored to: {}",
        uow.len()
    );
    assert_eq!(uow.len(), 2);
    assert_eq!(tx.state(), TransactionState::Active);

    // -----------------------------------------------------------------------
    // 4. Successful Operations & Transaction Commit
    // -----------------------------------------------------------------------
    println!("\n--- [4] Clean Transaction Commit ---");
    let mut old_bob = BTreeMap::new();
    old_bob.insert("balance".to_string(), LiteralValue::Float64(500.0));
    let mut new_bob = BTreeMap::new();
    new_bob.insert("balance".to_string(), LiteralValue::Float64(1200.0));
    uow.register_dirty_node(2, old_bob, new_bob);

    println!("Total pending mutations prior to commit: {}", uow.len());
    tx.commit(&mut uow)?;
    println!(
        "Transaction committed successfully! State: {:?}, Pending UoW mutations: {}",
        tx.state(),
        uow.len()
    );
    assert_eq!(tx.state(), TransactionState::Committed);
    assert_eq!(uow.len(), 0);

    // -----------------------------------------------------------------------
    // 5. Full Transaction Auto-Rollback Lifecycle
    // -----------------------------------------------------------------------
    println!("\n--- [5] Full Transaction Rollback Lifecycle ---");
    let mut tx2 = Transaction::new(1002, &uow, &arena);
    let mut eve_props = BTreeMap::new();
    eve_props.insert("name".to_string(), LiteralValue::String("Eve".to_string()));
    uow.register_new_node(vec!["Customer".to_string()], eve_props);
    assert_eq!(uow.len(), 1);

    println!("Transaction #1002 encountered an unrecoverable network error. Rolling back...");
    tx2.rollback(&mut uow, &mut arena)?;
    println!(
        "Transaction #1002 rolled back! State: {:?}, UoW pending mutations: {}",
        tx2.state(),
        uow.len()
    );
    assert_eq!(tx2.state(), TransactionState::RolledBack);
    assert_eq!(uow.len(), 0);

    println!("\n[SUCCESS] Transactions and Unit of Work demo completed successfully.");
    Ok(())
}
