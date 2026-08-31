//! Zero-Copy Apache Arrow Columnar Graph Streaming & Ingestion.

use crate::error::{Error, Result};
use arrow::array::{ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::ffi_stream::FFI_ArrowArrayStream;
use arrow::record_batch::RecordBatchReader;
use std::sync::Arc;

/// In-memory stream reader over a collection of Arrow `RecordBatch` chunks.
pub struct MemoryRecordBatchReader {
    schema: Arc<Schema>,
    batches: Vec<RecordBatch>,
    index: usize,
}

impl MemoryRecordBatchReader {
    /// Creates a new in-memory reader over the given schema and batch slices.
    pub fn new(schema: Arc<Schema>, batches: Vec<RecordBatch>) -> Self {
        Self {
            schema,
            batches,
            index: 0,
        }
    }
}

impl Iterator for MemoryRecordBatchReader {
    type Item = std::result::Result<RecordBatch, arrow::error::ArrowError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.batches.len() {
            let batch = self.batches[self.index].clone();
            self.index += 1;
            Some(Ok(batch))
        } else {
            None
        }
    }
}

impl RecordBatchReader for MemoryRecordBatchReader {
    fn schema(&self) -> Arc<Schema> {
        Arc::clone(&self.schema)
    }
}

/// Columnar Graph Batch Builder for zero-copy Arrow RecordBatch generation.
#[derive(Debug, Clone)]
pub struct GraphBatchBuilder {
    node_ids: Vec<i64>,
    labels: Vec<String>,
    names: Vec<String>,
    ages: Vec<i64>,
    scores: Vec<f64>,
    active: Vec<bool>,
}

impl Default for GraphBatchBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphBatchBuilder {
    /// Creates a new graph batch builder with pre-allocated vectors.
    pub fn new() -> Self {
        Self {
            node_ids: Vec::new(),
            labels: Vec::new(),
            names: Vec::new(),
            ages: Vec::new(),
            scores: Vec::new(),
            active: Vec::new(),
        }
    }

    /// Creates a new builder with reserved capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            node_ids: Vec::with_capacity(capacity),
            labels: Vec::with_capacity(capacity),
            names: Vec::with_capacity(capacity),
            ages: Vec::with_capacity(capacity),
            scores: Vec::with_capacity(capacity),
            active: Vec::with_capacity(capacity),
        }
    }

    /// Appends a single graph node record to the columnar buffer.
    #[inline(always)]
    pub fn push_node(
        &mut self,
        id: i64,
        label: impl Into<String>,
        name: impl Into<String>,
        age: i64,
        score: f64,
        is_active: bool,
    ) {
        self.node_ids.push(id);
        self.labels.push(label.into());
        self.names.push(name.into());
        self.ages.push(age);
        self.scores.push(score);
        self.active.push(is_active);
    }

    /// Generates a synthetic dataset of `count` graph node records for microbenchmarking.
    pub fn generate_synthetic_nodes(count: usize) -> RecordBatch {
        let mut builder = Self::with_capacity(count);
        for i in 0..count {
            builder.push_node(
                i as i64,
                "Person",
                format!("Person_{i}"),
                20 + (i % 60) as i64,
                75.5 + ((i % 25) as f64),
                i % 2 == 0,
            );
        }
        builder
            .finish()
            .expect("Failed to build synthetic RecordBatch")
    }

    /// Compiles the columnar arrays into an Arrow `RecordBatch`.
    pub fn finish(self) -> Result<RecordBatch> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("label", DataType::Utf8, false),
            Field::new("name", DataType::Utf8, false),
            Field::new("age", DataType::Int64, false),
            Field::new("score", DataType::Float64, false),
            Field::new("active", DataType::Boolean, false),
        ]));

        let columns: Vec<ArrayRef> = vec![
            Arc::new(Int64Array::from(self.node_ids)),
            Arc::new(StringArray::from(self.labels)),
            Arc::new(StringArray::from(self.names)),
            Arc::new(Int64Array::from(self.ages)),
            Arc::new(Float64Array::from(self.scores)),
            Arc::new(BooleanArray::from(self.active)),
        ];

        RecordBatch::try_new(schema, columns).map_err(|e| Error::ArrowError(e.to_string()))
    }
}

/// Exports an Arrow `RecordBatch` into a C-ABI compatible `FFI_ArrowArrayStream`.
pub fn export_batch_to_c_stream(batch: RecordBatch) -> FFI_ArrowArrayStream {
    let schema = batch.schema();
    let reader = Box::new(MemoryRecordBatchReader::new(schema, vec![batch]));
    FFI_ArrowArrayStream::new(reader)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arrow_synthetic_batch_generation() {
        let batch = GraphBatchBuilder::generate_synthetic_nodes(1000);
        assert_eq!(batch.num_rows(), 1000);
        assert_eq!(batch.num_columns(), 6);
        assert_eq!(batch.column(0).len(), 1000);
    }

    #[test]
    fn test_arrow_c_stream_export() {
        let batch = GraphBatchBuilder::generate_synthetic_nodes(500);
        let stream = export_batch_to_c_stream(batch);
        assert!(stream.get_schema.is_some());
        assert!(stream.get_next.is_some());
    }
}
