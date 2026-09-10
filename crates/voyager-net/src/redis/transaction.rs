//! Redis / FalkorDB transaction implementation using `MULTI`, `EXEC`, and `DISCARD`.

use async_trait::async_trait;
use std::collections::HashMap;

use crate::engine::{AsyncTransaction, QueryResult, QuerySummary};
use crate::error::{NetError, Result};
use crate::redis::connection::RedisConnection;
use crate::redis::graph::FalkorQueryResult;
use crate::redis::resp::RespValue;

/// An active transaction over a Redis connection using `MULTI` / `EXEC` / `DISCARD`.
pub struct RedisTransaction<'a> {
    conn: &'a mut RedisConnection,
    is_active: bool,
    #[allow(dead_code)]
    queued_count: usize,
    accumulated_summary: QuerySummary,
}

impl<'a> RedisTransaction<'a> {
    /// Begins a new transaction on the provided Redis connection by executing `MULTI`.
    pub async fn begin(conn: &'a mut RedisConnection) -> Result<Self> {
        let resp = conn.execute_raw(&["MULTI"]).await?;
        match resp {
            RespValue::SimpleString(s) if s == "OK" => {
                conn.set_in_transaction(true);
                Ok(Self {
                    conn,
                    is_active: true,
                    queued_count: 0,
                    accumulated_summary: QuerySummary::default(),
                })
            }
            RespValue::Error(err) => Err(NetError::TransactionError(format!(
                "Failed to begin Redis transaction (MULTI): {}",
                err
            ))),
            other => Err(NetError::ProtocolError(format!(
                "Unexpected response to MULTI: {:?}",
                other
            ))),
        }
    }

    /// Returns `true` if this transaction is active and has not been committed or rolled back.
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Executes a parameterized query within the transaction boundary.
    /// In Redis `MULTI` blocks, commands are queued until `EXEC`.
    pub async fn execute(
        &mut self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult> {
        if !self.is_active {
            return Err(NetError::TransactionError(
                "Cannot execute query on inactive transaction".to_string(),
            ));
        }

        let formatted_query = format_cypher_with_params(query, params);
        let graph_name = self.conn.default_graph().to_string();

        let resp = self
            .conn
            .execute_raw(&["GRAPH.QUERY", &graph_name, &formatted_query])
            .await?;

        match resp {
            RespValue::SimpleString(s) if s == "QUEUED" => {
                self.queued_count += 1;
                // Since commands are queued until EXEC, return an empty QueryResult for intermediate steps
                Ok(QueryResult::empty(QuerySummary::default()))
            }
            RespValue::Error(err) => Err(NetError::ExecutionError(format!(
                "Failed to queue query in transaction: {}",
                err
            ))),
            other => Err(NetError::ProtocolError(format!(
                "Expected +QUEUED in transaction, got {:?}",
                other
            ))),
        }
    }

    /// Commits the active transaction using `EXEC` and aggregates summary statistics.
    pub async fn commit(mut self) -> Result<QuerySummary> {
        self.commit_internal().await
    }

    /// Aborts and rolls back the active transaction using `DISCARD`.
    pub async fn rollback(mut self) -> Result<()> {
        self.rollback_internal().await
    }

    async fn commit_internal(&mut self) -> Result<QuerySummary> {
        if !self.is_active {
            return Err(NetError::TransactionError(
                "Transaction is already completed".to_string(),
            ));
        }

        let resp = self.conn.execute_raw(&["EXEC"]).await?;
        self.conn.set_in_transaction(false);
        self.is_active = false;

        match resp {
            RespValue::Array(Some(results)) => {
                let mut total_summary = self.accumulated_summary.clone();

                for item in results {
                    if let Ok(res) = FalkorQueryResult::parse(item) {
                        let summary = res.statistics.to_query_summary();
                        total_summary.nodes_created += summary.nodes_created;
                        total_summary.nodes_deleted += summary.nodes_deleted;
                        total_summary.relationships_created += summary.relationships_created;
                        total_summary.relationships_deleted += summary.relationships_deleted;
                        total_summary.properties_set += summary.properties_set;
                        total_summary.labels_added += summary.labels_added;
                        total_summary.labels_removed += summary.labels_removed;
                        total_summary.indexes_added += summary.indexes_added;
                        total_summary.execution_time_ms += summary.execution_time_ms;
                    }
                }

                Ok(total_summary)
            }
            RespValue::Array(None) => Err(NetError::TransactionError(
                "Transaction was aborted (EXEC returned null)".to_string(),
            )),
            RespValue::Error(err) => Err(NetError::TransactionError(format!(
                "Transaction EXEC failed: {}",
                err
            ))),
            other => Err(NetError::ProtocolError(format!(
                "Unexpected response to EXEC: {:?}",
                other
            ))),
        }
    }

    async fn rollback_internal(&mut self) -> Result<()> {
        if !self.is_active {
            return Ok(());
        }

        let resp = self.conn.execute_raw(&["DISCARD"]).await?;
        self.conn.set_in_transaction(false);
        self.is_active = false;

        match resp {
            RespValue::SimpleString(s) if s == "OK" => Ok(()),
            RespValue::Error(err) => Err(NetError::TransactionError(format!(
                "Failed to discard transaction: {}",
                err
            ))),
            other => Err(NetError::ProtocolError(format!(
                "Unexpected response to DISCARD: {:?}",
                other
            ))),
        }
    }
}

#[async_trait]
impl<'a> AsyncTransaction for RedisTransaction<'a> {
    async fn execute(
        &mut self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult> {
        self.execute(query, params).await
    }

    async fn commit(mut self: Box<Self>) -> Result<QuerySummary> {
        self.commit_internal().await
    }

    async fn rollback(mut self: Box<Self>) -> Result<()> {
        self.rollback_internal().await
    }
}

/// Helper function to prefix a Cypher query with parameters in FalkorDB's native syntax.
pub fn format_cypher_with_params(
    query: &str,
    params: &HashMap<String, serde_json::Value>,
) -> String {
    if params.is_empty() {
        return query.to_string();
    }

    let mut parts = Vec::with_capacity(params.len());
    for (k, v) in params {
        match v {
            serde_json::Value::String(s) => {
                parts.push(format!("{}=\"{}\"", k, s.replace('\"', "\\\"")));
            }
            serde_json::Value::Number(n) => {
                parts.push(format!("{}={}", k, n));
            }
            serde_json::Value::Bool(b) => {
                parts.push(format!("{}={}", k, b));
            }
            serde_json::Value::Null => {
                parts.push(format!("{}=null", k));
            }
            other => {
                parts.push(format!("{}={}", k, other));
            }
        }
    }

    format!("CYPHER {} {}", parts.join(" "), query)
}
