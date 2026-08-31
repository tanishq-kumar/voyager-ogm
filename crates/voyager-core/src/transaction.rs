//! Two-Layer Rollback Transaction and In-Memory Unit-of-Work.
//!
//! Provides two-phase transaction management ensuring in-memory AST arena handles,
//! entity mutations, and dirty property snapshots automatically revert upon database
//! transaction failures, aborts, or savepoint rollbacks.

use crate::ast::{LiteralValue, QueryAstArena};
use crate::error::{Error, Result};
use std::collections::BTreeMap;

/// State of an in-flight or completed transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransactionState {
    /// Active transaction accepting mutations and savepoints.
    Active,
    /// Successfully committed transaction.
    Committed,
    /// Rolled back transaction with reverted in-memory state.
    RolledBack,
}

/// Recorded entity mutation for in-memory Unit-of-Work tracking and undo operations.
#[derive(Debug, Clone, PartialEq)]
pub enum EntityMutation {
    /// Newly created graph node entity.
    InsertNode {
        /// Generated client-side temporary identifier.
        temp_id: u64,
        /// Assigned graph node labels.
        labels: Vec<String>,
        /// Initial entity property map.
        properties: BTreeMap<String, LiteralValue>,
    },
    /// Modified existing graph node entity.
    UpdateNode {
        /// Unique node identifier.
        node_id: u64,
        /// Original property values prior to mutation (for undo rollback).
        old_properties: BTreeMap<String, LiteralValue>,
        /// Updated property values.
        new_properties: BTreeMap<String, LiteralValue>,
    },
    /// Deleted graph node entity.
    DeleteNode {
        /// Identifier of the deleted node.
        node_id: u64,
        /// Previous labels of the node (for restoration).
        old_labels: Vec<String>,
        /// Previous properties of the node (for restoration).
        old_properties: BTreeMap<String, LiteralValue>,
    },
    /// Newly created graph relationship edge.
    InsertEdge {
        /// Generated client-side temporary edge identifier.
        temp_id: u64,
        /// Source node identifier.
        source_id: u64,
        /// Target node identifier.
        target_id: u64,
        /// Relationship edge type name (e.g. "FOLLOWS").
        edge_type: String,
        /// Relationship property map.
        properties: BTreeMap<String, LiteralValue>,
    },
    /// Deleted graph relationship edge.
    DeleteEdge {
        /// Edge identifier.
        edge_id: u64,
        /// Source node identifier.
        source_id: u64,
        /// Target node identifier.
        target_id: u64,
        /// Relationship type name.
        edge_type: String,
        /// Previous properties prior to deletion.
        old_properties: BTreeMap<String, LiteralValue>,
    },
}

/// Snapshot of the memory arena and mutation log at a specific point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointState {
    /// Length of the AST arena at checkpoint creation.
    pub arena_len: usize,
    /// Total count of recorded mutations in the Unit-of-Work at checkpoint creation.
    pub mutation_count: usize,
}

/// Named savepoint within an active transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Savepoint {
    /// User-defined savepoint name.
    pub name: String,
    /// Captured checkpoint state.
    pub checkpoint: CheckpointState,
}

/// In-memory Unit of Work tracking entity lifecycle, dirty states, and pending mutations.
#[derive(Debug, Default, Clone)]
pub struct UnitOfWork {
    mutations: Vec<EntityMutation>,
    next_temp_id: u64,
}

impl UnitOfWork {
    /// Creates a new, empty Unit of Work.
    pub fn new() -> Self {
        Self {
            mutations: Vec::new(),
            next_temp_id: 1,
        }
    }

    /// Registers a newly created node in the unit of work.
    pub fn register_new_node(
        &mut self,
        labels: Vec<String>,
        properties: BTreeMap<String, LiteralValue>,
    ) -> u64 {
        let temp_id = self.next_temp_id;
        self.next_temp_id += 1;
        self.mutations.push(EntityMutation::InsertNode {
            temp_id,
            labels,
            properties,
        });
        temp_id
    }

    /// Registers an update to an existing node with original and updated properties.
    pub fn register_dirty_node(
        &mut self,
        node_id: u64,
        old_properties: BTreeMap<String, LiteralValue>,
        new_properties: BTreeMap<String, LiteralValue>,
    ) {
        self.mutations.push(EntityMutation::UpdateNode {
            node_id,
            old_properties,
            new_properties,
        });
    }

    /// Registers a node deletion with its prior labels and properties.
    pub fn register_deleted_node(
        &mut self,
        node_id: u64,
        old_labels: Vec<String>,
        old_properties: BTreeMap<String, LiteralValue>,
    ) {
        self.mutations.push(EntityMutation::DeleteNode {
            node_id,
            old_labels,
            old_properties,
        });
    }

    /// Registers a newly created relationship edge.
    pub fn register_new_edge(
        &mut self,
        source_id: u64,
        target_id: u64,
        edge_type: impl Into<String>,
        properties: BTreeMap<String, LiteralValue>,
    ) -> u64 {
        let temp_id = self.next_temp_id;
        self.next_temp_id += 1;
        self.mutations.push(EntityMutation::InsertEdge {
            temp_id,
            source_id,
            target_id,
            edge_type: edge_type.into(),
            properties,
        });
        temp_id
    }

    /// Registers a deleted relationship edge.
    pub fn register_deleted_edge(
        &mut self,
        edge_id: u64,
        source_id: u64,
        target_id: u64,
        edge_type: impl Into<String>,
        old_properties: BTreeMap<String, LiteralValue>,
    ) {
        self.mutations.push(EntityMutation::DeleteEdge {
            edge_id,
            source_id,
            target_id,
            edge_type: edge_type.into(),
            old_properties,
        });
    }

    /// Returns the sequence of all pending entity mutations.
    pub fn pending_mutations(&self) -> &[EntityMutation] {
        &self.mutations
    }

    /// Returns the total number of pending entity mutations.
    pub fn len(&self) -> usize {
        self.mutations.len()
    }

    /// Checks if there are no pending entity mutations.
    pub fn is_empty(&self) -> bool {
        self.mutations.is_empty()
    }

    /// Captures a snapshot checkpoint of the current unit of work and arena state.
    pub fn checkpoint(&self, arena: &QueryAstArena) -> CheckpointState {
        CheckpointState {
            arena_len: arena.len(),
            mutation_count: self.mutations.len(),
        }
    }

    /// Rolls back pending mutations and the AST arena to a prior checkpoint.
    pub fn rollback_to(&mut self, arena: &mut QueryAstArena, checkpoint: &CheckpointState) {
        arena.rollback_to(checkpoint.arena_len);
        if checkpoint.mutation_count < self.mutations.len() {
            self.mutations.truncate(checkpoint.mutation_count);
        }
    }

    /// Clears all pending mutations upon successful commit or transaction completion.
    pub fn clear(&mut self) {
        self.mutations.clear();
    }
}

/// Two-Layer Transaction Manager with in-memory savepoints and dirty rollback.
#[derive(Debug)]
pub struct Transaction {
    id: u64,
    state: TransactionState,
    initial_checkpoint: CheckpointState,
    savepoints: Vec<Savepoint>,
}

impl Transaction {
    /// Starts a new transaction capturing the initial unit of work and arena checkpoint.
    pub fn new(id: u64, uow: &UnitOfWork, arena: &QueryAstArena) -> Self {
        let initial_checkpoint = uow.checkpoint(arena);
        Self {
            id,
            state: TransactionState::Active,
            initial_checkpoint,
            savepoints: Vec::new(),
        }
    }

    /// Returns the unique transaction identifier.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Returns the current lifecycle state of the transaction.
    pub fn state(&self) -> TransactionState {
        self.state
    }

    /// Checks if the transaction is actively accepting operations.
    pub fn is_active(&self) -> bool {
        self.state == TransactionState::Active
    }

    /// Creates a named savepoint within the active transaction.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TransactionError`] if the transaction is no longer active.
    pub fn savepoint(
        &mut self,
        name: impl Into<String>,
        uow: &UnitOfWork,
        arena: &QueryAstArena,
    ) -> Result<()> {
        if !self.is_active() {
            return Err(Error::TransactionError(
                "Cannot create savepoint on inactive transaction".into(),
            ));
        }
        let sp_name = name.into();
        let checkpoint = uow.checkpoint(arena);
        self.savepoints.push(Savepoint {
            name: sp_name,
            checkpoint,
        });
        Ok(())
    }

    /// Rolls back in-memory entities and AST arena to a named savepoint.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TransactionError`] if savepoint does not exist or transaction is inactive.
    pub fn rollback_to_savepoint(
        &mut self,
        name: &str,
        uow: &mut UnitOfWork,
        arena: &mut QueryAstArena,
    ) -> Result<()> {
        if !self.is_active() {
            return Err(Error::TransactionError(
                "Cannot rollback savepoint on inactive transaction".into(),
            ));
        }

        let pos = self
            .savepoints
            .iter()
            .rposition(|sp| sp.name == name)
            .ok_or_else(|| {
                Error::TransactionError(format!("Savepoint '{name}' not found in transaction"))
            })?;

        let sp = self.savepoints[pos].clone();
        uow.rollback_to(arena, &sp.checkpoint);

        // Truncate savepoints created after this savepoint
        self.savepoints.truncate(pos + 1);
        Ok(())
    }

    /// Releases a named savepoint from the transaction.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TransactionError`] if savepoint does not exist or transaction is inactive.
    pub fn release_savepoint(&mut self, name: &str) -> Result<()> {
        if !self.is_active() {
            return Err(Error::TransactionError(
                "Cannot release savepoint on inactive transaction".into(),
            ));
        }

        let pos = self
            .savepoints
            .iter()
            .rposition(|sp| sp.name == name)
            .ok_or_else(|| {
                Error::TransactionError(format!("Savepoint '{name}' not found in transaction"))
            })?;

        self.savepoints.remove(pos);
        Ok(())
    }

    /// Commits the transaction and marks its state as [`TransactionState::Committed`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::TransactionError`] if transaction is already completed.
    pub fn commit(&mut self, uow: &mut UnitOfWork) -> Result<()> {
        if !self.is_active() {
            return Err(Error::TransactionError(
                "Cannot commit inactive transaction".into(),
            ));
        }
        self.state = TransactionState::Committed;
        uow.clear();
        self.savepoints.clear();
        Ok(())
    }

    /// Rolls back the entire transaction to its initial state, discarding all pending mutations.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TransactionError`] if transaction is already completed.
    pub fn rollback(&mut self, uow: &mut UnitOfWork, arena: &mut QueryAstArena) -> Result<()> {
        if !self.is_active() {
            return Err(Error::TransactionError(
                "Cannot rollback inactive transaction".into(),
            ));
        }
        self.state = TransactionState::RolledBack;
        uow.rollback_to(arena, &self.initial_checkpoint);
        self.savepoints.clear();
        Ok(())
    }
}
