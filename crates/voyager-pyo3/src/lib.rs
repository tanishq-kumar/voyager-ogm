//! Python PyO3 bindings and Apache Arrow bridge for Voyager OGM.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use voyager_core::ast::{AggregationFunc, BinaryOp, LiteralValue};
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_core::visitor::AstVisitor;

fn py_to_literal(val: &Bound<'_, PyAny>) -> PyResult<LiteralValue> {
    if val.is_none() {
        Ok(LiteralValue::Null)
    } else if let Ok(b) = val.extract::<bool>() {
        Ok(LiteralValue::Bool(b))
    } else if let Ok(i) = val.extract::<i64>() {
        Ok(LiteralValue::Int64(i))
    } else if let Ok(f) = val.extract::<f64>() {
        Ok(LiteralValue::Float64(f))
    } else if let Ok(s) = val.extract::<String>() {
        Ok(LiteralValue::String(s))
    } else if let Ok(list) = val.downcast::<PyList>() {
        let mut items = Vec::with_capacity(list.len());
        for item in list {
            items.push(py_to_literal(&item)?);
        }
        Ok(LiteralValue::List(items))
    } else {
        Err(PyValueError::new_err(format!(
            "Unsupported parameter type: {}",
            val.get_type()
        )))
    }
}

fn literal_to_py<'py>(lit: &LiteralValue, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    match lit {
        LiteralValue::Null => Ok(py.None().into_bound(py)),
        LiteralValue::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any()),
        LiteralValue::Int64(i) => Ok(i.into_pyobject(py)?.to_owned().into_any()),
        LiteralValue::Float64(f) => Ok(f.into_pyobject(py)?.to_owned().into_any()),
        LiteralValue::String(s) => Ok(s.into_pyobject(py)?.to_owned().into_any()),
        LiteralValue::ParameterRef(p) => Ok(p.into_pyobject(py)?.to_owned().into_any()),
        LiteralValue::List(l) => {
            let list = PyList::empty(py);
            for item in l {
                list.append(literal_to_py(item, py)?)?;
            }
            Ok(list.into_any())
        }
    }
}

/// Native Rust Query Builder exposed to Python.
#[pyclass(name = "NativeQueryBuilder")]
#[derive(Default, Clone)]
pub struct PyQueryBuilder {
    inner: QueryBuilder,
}

#[pymethods]
impl PyQueryBuilder {
    #[new]
    fn new() -> Self {
        Self {
            inner: QueryBuilder::new(),
        }
    }

    fn r#match(&mut self) {
        self.inner.r#match();
    }

    fn optional_match(&mut self) {
        self.inner.optional_match();
    }

    #[pyo3(signature = (variable=None, labels=vec![]))]
    fn node(&mut self, variable: Option<String>, labels: Vec<String>) {
        self.inner.node(variable, labels);
    }

    #[pyo3(signature = (edge_types=vec![], variable=None))]
    fn to(&mut self, edge_types: Vec<String>, variable: Option<String>) {
        self.inner.to(edge_types, variable);
    }

    #[allow(clippy::wrong_self_convention)]
    #[pyo3(signature = (edge_types=vec![], variable=None))]
    fn from_edge(&mut self, edge_types: Vec<String>, variable: Option<String>) {
        self.inner.from(edge_types, variable);
    }

    #[pyo3(signature = (edge_types=vec![], variable=None))]
    fn edge(&mut self, edge_types: Vec<String>, variable: Option<String>) {
        self.inner.edge(edge_types, variable);
    }

    fn hops(&mut self, min: u32, max: u32) {
        self.inner.hops(min, max);
    }

    fn where_eq(&mut self, var: String, prop: String, val: &Bound<'_, PyAny>) -> PyResult<()> {
        let lit = py_to_literal(val)?;
        self.inner.where_property(var, prop, BinaryOp::Eq, lit);
        Ok(())
    }

    fn where_gt(&mut self, var: String, prop: String, val: &Bound<'_, PyAny>) -> PyResult<()> {
        let lit = py_to_literal(val)?;
        self.inner.where_property(var, prop, BinaryOp::Gt, lit);
        Ok(())
    }

    fn where_gte(&mut self, var: String, prop: String, val: &Bound<'_, PyAny>) -> PyResult<()> {
        let lit = py_to_literal(val)?;
        self.inner.where_property(var, prop, BinaryOp::Gte, lit);
        Ok(())
    }

    fn where_lt(&mut self, var: String, prop: String, val: &Bound<'_, PyAny>) -> PyResult<()> {
        let lit = py_to_literal(val)?;
        self.inner.where_property(var, prop, BinaryOp::Lt, lit);
        Ok(())
    }

    fn where_lte(&mut self, var: String, prop: String, val: &Bound<'_, PyAny>) -> PyResult<()> {
        let lit = py_to_literal(val)?;
        self.inner.where_property(var, prop, BinaryOp::Lte, lit);
        Ok(())
    }

    fn where_contains(&mut self, var: String, prop: String, val: String) {
        self.inner.where_contains(var, prop, val);
    }

    fn r#return(&mut self) {
        self.inner.r#return();
    }

    #[pyo3(name = "return_")]
    fn return_py(&mut self) {
        self.inner.r#return();
    }

    fn create(&mut self) {
        self.inner.create();
    }

    fn merge(&mut self) {
        self.inner.merge();
    }

    fn on_create_set(&mut self, var: String, prop: String, val: &Bound<'_, PyAny>) -> PyResult<()> {
        let lit = py_to_literal(val)?;
        self.inner.on_create_set(var, prop, lit);
        Ok(())
    }

    fn on_match_set(&mut self, var: String, prop: String, val: &Bound<'_, PyAny>) -> PyResult<()> {
        let lit = py_to_literal(val)?;
        self.inner.on_match_set(var, prop, lit);
        Ok(())
    }

    fn set_property(&mut self, var: String, prop: String, val: &Bound<'_, PyAny>) -> PyResult<()> {
        let lit = py_to_literal(val)?;
        self.inner.set_property(var, prop, lit);
        Ok(())
    }

    fn delete(&mut self, targets: Vec<String>) {
        self.inner.delete(targets);
    }

    fn detach_delete(&mut self, targets: Vec<String>) {
        self.inner.detach_delete(targets);
    }

    fn remove_property(&mut self, var: String, prop: String) {
        self.inner.remove_property(var, prop);
    }

    #[pyo3(signature = (var, prop, alias=None))]
    fn field(&mut self, var: String, prop: String, alias: Option<String>) {
        self.inner.field(var, prop, alias);
    }

    #[pyo3(signature = (var, prop, func, alias=None))]
    fn aggregate(
        &mut self,
        var: String,
        prop: String,
        func: String,
        alias: Option<String>,
    ) -> PyResult<()> {
        let agg = match func.to_lowercase().as_str() {
            "count" => AggregationFunc::Count,
            "count_distinct" => AggregationFunc::CountDistinct,
            "sum" => AggregationFunc::Sum,
            "avg" => AggregationFunc::Avg,
            "min" => AggregationFunc::Min,
            "max" => AggregationFunc::Max,
            "collect" => AggregationFunc::Collect,
            other => {
                return Err(PyValueError::new_err(format!(
                    "Unknown aggregation function: {other}"
                )));
            }
        };
        self.inner.select_property_aggregate(var, prop, agg, alias);
        Ok(())
    }

    fn order_by(&mut self, var: String, prop: String, ascending: bool) {
        if ascending {
            self.inner.order_by_asc(var, prop);
        } else {
            self.inner.order_by_desc(var, prop);
        }
    }

    fn distinct(&mut self) {
        self.inner.distinct(true);
    }

    fn limit(&mut self, limit: u64) {
        self.inner.limit(limit);
    }

    fn skip(&mut self, skip: u64) {
        self.inner.skip(skip);
    }

    #[pyo3(signature = (dialect="cypher", graph_name=None))]
    fn compile<'py>(
        &self,
        dialect: &str,
        graph_name: Option<String>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let (arena, root) = self.inner.clone().build();
        let compiled = match dialect.to_lowercase().as_str() {
            "cypher" | "opencypher" | "neo4j" | "memgraph" => {
                let mut emitter = CypherEmitter::new();
                emitter
                    .visit_query(&arena, root)
                    .map_err(|e| PyValueError::new_err(e.to_string()))?
            }
            "sql_pgq" | "pgq" | "duckpgq" | "sql" => {
                let name = graph_name.unwrap_or_else(|| "graph_table".into());
                let mut emitter = SqlPgqEmitter::new(name);
                emitter
                    .visit_query(&arena, root)
                    .map_err(|e| PyValueError::new_err(e.to_string()))?
            }
            "iso_gql" | "gql" => {
                let mut emitter = IsoGqlEmitter::new();
                emitter
                    .visit_query(&arena, root)
                    .map_err(|e| PyValueError::new_err(e.to_string()))?
            }
            other => {
                return Err(PyValueError::new_err(format!(
                    "Unsupported query dialect: '{other}'. Choose from 'cypher', 'sql_pgq', or 'iso_gql'."
                )));
            }
        };

        let dict = PyDict::new(py);
        dict.set_item("statement", compiled.statement)?;

        let params_dict = PyDict::new(py);
        for (k, v) in compiled.parameters {
            params_dict.set_item(k, literal_to_py(&v, py)?)?;
        }
        dict.set_item("parameters", params_dict)?;

        Ok(dict)
    }
}

/// Zero-copy Arrow Stream implementing the Python Arrow PyCapsule Protocol (`__arrow_c_stream__`).
#[pyclass(name = "ArrowStream")]
pub struct PyArrowStream {
    batch: arrow::record_batch::RecordBatch,
}

#[pymethods]
impl PyArrowStream {
    /// Number of rows in the batch.
    #[getter]
    fn num_rows(&self) -> usize {
        self.batch.num_rows()
    }

    /// Number of columns in the batch.
    #[getter]
    fn num_columns(&self) -> usize {
        self.batch.num_columns()
    }

    /// Official Python Arrow PyCapsule Protocol for zero-copy streaming into Polars / PyArrow.
    #[pyo3(signature = (requested_schema=None))]
    fn __arrow_c_stream__<'py>(
        &self,
        py: Python<'py>,
        requested_schema: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, pyo3::types::PyCapsule>> {
        let _ = requested_schema;
        let ffi_stream = voyager_core::arrow::export_batch_to_c_stream(self.batch.clone());
        let name = std::ffi::CString::new("arrow_array_stream").unwrap();
        pyo3::types::PyCapsule::new(py, ffi_stream, Some(name))
    }
}

/// Generates a synthetic Arrow RecordBatch stream for microbenchmarks.
#[pyfunction]
fn generate_synthetic_stream(count: usize) -> PyResult<PyArrowStream> {
    let batch = voyager_core::arrow::GraphBatchBuilder::generate_synthetic_nodes(count);
    Ok(PyArrowStream { batch })
}

/// Returns the native Voyager OGM engine version string.
#[pyfunction]
fn version() -> &'static str {
    voyager_core::VERSION
}

/// Native In-Memory Unit of Work for dirty entity state management.
#[pyclass(name = "NativeUnitOfWork")]
pub struct PyUnitOfWork {
    inner: voyager_core::transaction::UnitOfWork,
}

#[pymethods]
impl PyUnitOfWork {
    #[new]
    fn new() -> Self {
        Self {
            inner: voyager_core::transaction::UnitOfWork::new(),
        }
    }

    #[getter]
    fn len(&self) -> usize {
        self.inner.len()
    }

    #[getter]
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn clear(&mut self) {
        self.inner.clear();
    }
}

/// Native Two-Layer Transaction with in-memory savepoints and dirty rollback.
#[pyclass(name = "NativeTransaction")]
pub struct PyTransaction {
    inner: voyager_core::transaction::Transaction,
    uow: voyager_core::transaction::UnitOfWork,
    arena: voyager_core::ast::QueryAstArena,
}

#[pymethods]
impl PyTransaction {
    #[new]
    fn new(id: u64) -> Self {
        let uow = voyager_core::transaction::UnitOfWork::new();
        let arena = voyager_core::ast::QueryAstArena::new();
        let inner = voyager_core::transaction::Transaction::new(id, &uow, &arena);
        Self { inner, uow, arena }
    }

    #[getter]
    fn id(&self) -> u64 {
        self.inner.id()
    }

    #[getter]
    fn state(&self) -> String {
        match self.inner.state() {
            voyager_core::transaction::TransactionState::Active => "ACTIVE".into(),
            voyager_core::transaction::TransactionState::Committed => "COMMITTED".into(),
            voyager_core::transaction::TransactionState::RolledBack => "ROLLED_BACK".into(),
        }
    }

    #[getter]
    fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    fn savepoint(&mut self, name: String) -> PyResult<()> {
        self.inner
            .savepoint(name, &self.uow, &self.arena)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn rollback_to_savepoint(&mut self, name: String) -> PyResult<()> {
        self.inner
            .rollback_to_savepoint(&name, &mut self.uow, &mut self.arena)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn release_savepoint(&mut self, name: String) -> PyResult<()> {
        self.inner
            .release_savepoint(&name)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn commit(&mut self) -> PyResult<()> {
        self.inner
            .commit(&mut self.uow)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn rollback(&mut self) -> PyResult<()> {
        self.inner
            .rollback(&mut self.uow, &mut self.arena)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

/// Native Python module definition for `_voyager_rs`.
#[pymodule]
fn _voyager_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(generate_synthetic_stream, m)?)?;
    m.add_class::<PyQueryBuilder>()?;
    m.add_class::<PyArrowStream>()?;
    m.add_class::<PyUnitOfWork>()?;
    m.add_class::<PyTransaction>()?;
    m.add("__version__", voyager_core::VERSION)?;
    Ok(())
}
