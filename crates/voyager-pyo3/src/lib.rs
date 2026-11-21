//! Python PyO3 bindings and Apache Arrow bridge for Voyager OGM.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use pyo3::exceptions::{
    PyNotImplementedError, PyRuntimeError, PyTypeError, PyUserWarning, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};

pyo3::create_exception!(
    voyager_ogm._voyager_rs,
    VoyagerError,
    pyo3::exceptions::PyValueError
);

fn make_voyager_err(code: voyager_ogm::ErrorCode, msg: impl Into<String>) -> PyErr {
    Python::attach(|py| {
        let py_err = VoyagerError::new_err(msg.into());
        let _ = py_err.value(py).setattr("code", code.as_str());
        py_err
    })
}

/// Whether a [`SpecPath`] belongs to the Query Spec v1 consumer or to the
/// fluent PyO3 builder API. Spec paths produce `INVALID_SPEC` errors that carry
/// the JSON Pointer; fluent calls keep their historical error codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpecPathMode {
    Spec,
    Fluent,
}

/// JSON Pointer to an element of a Query Spec v1 document.
///
/// Paths are built while descending into the document so an error does not have
/// to reconstruct its location: [`SpecPath::key`] appends an object member and
/// [`SpecPath::index`] appends an array index. `/` and `~` are escaped as `~1`
/// and `~0`, as JSON Pointer requires. The document root is the empty string.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SpecPath {
    pointer: String,
    mode: SpecPathMode,
}

impl SpecPath {
    fn spec_root() -> Self {
        Self {
            pointer: String::new(),
            mode: SpecPathMode::Spec,
        }
    }

    fn fluent() -> Self {
        Self {
            pointer: String::new(),
            mode: SpecPathMode::Fluent,
        }
    }

    fn as_spec(&self) -> Self {
        Self {
            pointer: self.pointer.clone(),
            mode: SpecPathMode::Spec,
        }
    }

    fn is_spec(&self) -> bool {
        self.mode == SpecPathMode::Spec
    }

    fn key(&self, key: &str) -> Self {
        let mut pointer = self.pointer.clone();
        pointer.push('/');
        for ch in key.chars() {
            match ch {
                '~' => pointer.push_str("~0"),
                '/' => pointer.push_str("~1"),
                other => pointer.push(other),
            }
        }
        Self {
            pointer,
            mode: self.mode,
        }
    }

    fn index(&self, index: usize) -> Self {
        let mut pointer = self.pointer.clone();
        pointer.push('/');
        pointer.push_str(&index.to_string());
        Self {
            pointer,
            mode: self.mode,
        }
    }

    fn pointer(&self) -> &str {
        &self.pointer
    }
}

/// JSON type name for a Python value, used in spec error messages.
///
/// Spec authors write JSON, so messages say `number`, `string`, `array`, and so
/// on instead of leaking a Python type repr such as `<class 'int'>`.
fn json_type_name(val: &Bound<'_, PyAny>) -> &'static str {
    if val.is_none() {
        "null"
    } else if val.is_instance_of::<PyBool>() {
        "boolean"
    } else if val.is_instance_of::<PyInt>() || val.is_instance_of::<PyFloat>() {
        "number"
    } else if val.is_instance_of::<PyString>() {
        "string"
    } else if val.is_instance_of::<PyList>() || val.is_instance_of::<PyTuple>() {
        "array"
    } else if val.is_instance_of::<PyDict>() {
        "object"
    } else {
        "value"
    }
}

/// Renders a scalar Python value the way JSON spells it, for error messages.
///
/// Containers and arbitrary objects fall back to their JSON type name, so a
/// Python `repr` (single quotes, tuple parentheses) never leaks into a spec
/// message.
fn render_json_scalar(val: &Bound<'_, PyAny>) -> Option<String> {
    if val.is_none() {
        Some("null".to_string())
    } else if let Ok(boolean) = val.extract::<bool>() {
        Some(boolean.to_string())
    } else if let Ok(integer) = val.extract::<i64>() {
        Some(integer.to_string())
    } else if let Ok(float) = val.extract::<f64>() {
        Some(float.to_string())
    } else if val.is_instance_of::<PyString>() {
        val.extract::<String>()
            .ok()
            .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
    } else {
        None
    }
}

/// Builds a [`VoyagerError`] with code `INVALID_SPEC`, the JSON Pointer on
/// `err.path`, and the same pointer in the message.
fn make_spec_err(path: &SpecPath, msg: impl Into<String>) -> PyErr {
    Python::attach(|py| {
        let pointer = path.pointer();
        let msg = msg.into();
        let message = if pointer.is_empty() {
            format!("Invalid query spec at the document root: {msg}")
        } else {
            format!("Invalid query spec at '{pointer}': {msg}")
        };
        let py_err = VoyagerError::new_err(message);
        let _ = py_err.value(py).setattr("code", "INVALID_SPEC");
        let _ = py_err.value(py).setattr("path", pointer);
        py_err
    })
}

/// Structural error for a spec path, or the historical `INVALID_CLAUSE` for a
/// fluent builder call.
fn path_error(path: &SpecPath, msg: impl Into<String>) -> PyErr {
    let msg = msg.into();
    if path.is_spec() {
        make_spec_err(path, msg)
    } else {
        make_voyager_err(voyager_ogm::ErrorCode::InvalidClause, msg)
    }
}

/// Value error for a spec path, or the historical `ValueError` for a fluent
/// builder call.
fn path_value_error(path: &SpecPath, msg: impl Into<String>) -> PyErr {
    let msg = msg.into();
    if path.is_spec() {
        make_spec_err(path, msg)
    } else {
        PyValueError::new_err(msg)
    }
}

/// Extracts a Python value for a spec element, turning a conversion failure
/// into `INVALID_SPEC` with the element path (or the historical `TypeError` for
/// fluent calls).
fn extract_required<'a, 'py, T>(
    val: &'a Bound<'py, PyAny>,
    path: &SpecPath,
    expected: &str,
) -> PyResult<T>
where
    T: pyo3::FromPyObject<'a, 'py>,
{
    val.extract::<T>().map_err(|_| {
        let msg = format!("expected {expected}, got {}", json_type_name(val));
        if path.is_spec() {
            make_spec_err(path, msg)
        } else {
            PyTypeError::new_err(msg)
        }
    })
}

fn to_py_schema_err(err: voyager_ogm::Error) -> PyErr {
    Python::attach(|py| match err.code() {
        voyager_ogm::ErrorCode::UnsupportedDialect | voyager_ogm::ErrorCode::UnsupportedFeature => {
            let py_err = PyNotImplementedError::new_err(err.to_string());
            let _ = py_err.value(py).setattr("code", err.code().as_str());
            py_err
        }
        _ => make_voyager_err(err.code(), err.to_string()),
    })
}
use voyager_ogm::ast::{
    AggregationFunc, BinaryOp, Direction, LiteralValue, NodeHandle, QueryAstArena, UnaryOp,
};
use voyager_ogm::builder::{MergeRelationshipProperties, QueryBuilder};
use voyager_ogm::emitters::{AgeEmitter, CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_ogm::optimizer::{AstOptimizer, OptimizationLevel};
use voyager_ogm::schema::{
    ConformanceReport, DEFAULT_VECTOR_DIMENSIONS, DdlDialect, FieldDescriptor, FieldType,
    FullTextIndexConfig, IndexType, NodeSchema, RelationshipSchema, SchemaRegistry,
    VectorIndexConfig, VectorSimilarity, adapt_tantivy_query as core_adapt_tantivy_query,
    canonical_fulltext_index_name as core_canonical_fulltext_index_name,
    canonical_vector_index_name as core_canonical_vector_index_name, global_schema_registry,
    resolve_fulltext_search as core_resolve_fulltext_search,
};
use voyager_ogm::topology::GraphTopology;
use voyager_ogm::visitor::{AstVisitor, CompiledQuery};

fn compiled_query_to_py_dict<'py>(
    py: Python<'py>,
    compiled: &CompiledQuery,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("statement", &compiled.statement)?;

    let params_dict = PyDict::new(py);
    for (k, v) in &compiled.parameters {
        params_dict.set_item(k, literal_to_py(v, py)?)?;
    }
    dict.set_item("parameters", params_dict)?;
    dict.set_item("execution_mode", compiled.execution_mode.as_str())?;

    let cols_list = PyList::empty(py);
    for col in &compiled.columns {
        let col_dict = PyDict::new(py);
        col_dict.set_item("name", &col.name)?;
        col_dict.set_item("alias", &col.alias)?;
        cols_list.append(col_dict)?;
    }
    dict.set_item("columns", cols_list)?;

    Ok(dict)
}

fn py_to_literal(val: &Bound<'_, PyAny>) -> PyResult<LiteralValue> {
    if val.is_none() {
        Ok(LiteralValue::Null)
    } else if let Ok(b) = val.cast::<PyBool>() {
        Ok(LiteralValue::Bool(b.is_true()))
    } else if let Ok(i) = val.cast::<PyInt>() {
        Ok(LiteralValue::Int64(i.extract()?))
    } else if let Ok(f) = val.cast::<PyFloat>() {
        Ok(LiteralValue::Float64(f.value()))
    } else if let Ok(s) = val.cast::<PyString>() {
        Ok(LiteralValue::String(s.to_string_lossy().into_owned()))
    } else if let Ok(list) = val.cast::<PyList>() {
        let mut items = Vec::with_capacity(list.len());
        for item in list {
            items.push(py_to_literal(&item)?);
        }
        Ok(LiteralValue::List(items))
    } else if let Ok(tuple) = val.cast::<PyTuple>() {
        let mut items = Vec::with_capacity(tuple.len());
        for item in tuple {
            items.push(py_to_literal(&item)?);
        }
        Ok(LiteralValue::List(items))
    } else if let Ok(dict) = val.cast::<PyDict>() {
        let mut entries = Vec::with_capacity(dict.len());
        for (k, v) in dict {
            let key_str: String = if let Ok(s) = k.cast::<PyString>() {
                s.to_string_lossy().into_owned()
            } else {
                k.extract()?
            };
            let val_lit = py_to_literal(&v)?;
            entries.push((key_str, val_lit));
        }
        Ok(LiteralValue::Map(entries))
    } else if val.hasattr("isoformat")? {
        let iso: String = val.call_method0("isoformat")?.extract()?;
        Ok(LiteralValue::String(iso))
    } else if val.hasattr("hex")? && val.hasattr("urn")? {
        let s: String = val.str()?.extract()?;
        Ok(LiteralValue::String(s))
    } else if val.get_type().name()? == "Decimal" {
        if let Ok(f) = val
            .call_method0("__float__")
            .and_then(|v| v.extract::<f64>())
        {
            Ok(LiteralValue::Float64(f))
        } else {
            let s: String = val.str()?.extract()?;
            Ok(LiteralValue::String(s))
        }
    } else {
        Err(PyValueError::new_err(format!(
            "Unsupported parameter type: {}",
            json_type_name(val)
        )))
    }
}

/// Converts the right-hand side of a fluent `IN` / `NOT IN` comparison. A
/// Python list is the membership collection, so it is always a literal list;
/// anything else goes through the normal expression converter.
fn fluent_membership_operand(
    builder: &mut QueryBuilder,
    other: &Bound<'_, PyAny>,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    if other.cast::<PyList>().is_ok() {
        let lit = py_to_literal(other).map_err(|e| path_value_error(path, e.to_string()))?;
        return Ok(builder.literal(lit));
    }
    py_to_node_handle_depth(builder, other, 0, path)
}

fn property_list_to_literals(val: &Bound<'_, PyAny>) -> PyResult<Vec<(String, LiteralValue)>> {
    let list = val.cast::<PyList>().map_err(|_| {
        PyTypeError::new_err("relationship properties must be a list of (name, value) pairs")
    })?;
    let mut properties = Vec::with_capacity(list.len());
    for item in list {
        let (name, value): (String, Bound<'_, PyAny>) = item.extract()?;
        properties.push((name, py_to_literal(&value)?));
    }
    Ok(properties)
}

fn optional_property_list_to_literals(
    properties: Option<Vec<(String, Bound<'_, PyAny>)>>,
) -> PyResult<Vec<(String, LiteralValue)>> {
    match properties {
        Some(items) => {
            let mut literals = Vec::with_capacity(items.len());
            for (name, value) in items {
                literals.push((name, py_to_literal(&value)?));
            }
            Ok(literals)
        }
        None => Ok(Vec::new()),
    }
}

fn parse_binary_op(s: &str) -> PyResult<BinaryOp> {
    match s.to_ascii_lowercase().as_str() {
        "=" | "eq" => Ok(BinaryOp::Eq),
        "!=" | "ne" | "neq" | "<>" => Ok(BinaryOp::Neq),
        "<" | "lt" => Ok(BinaryOp::Lt),
        "<=" | "lte" | "le" => Ok(BinaryOp::Lte),
        ">" | "gt" => Ok(BinaryOp::Gt),
        ">=" | "gte" | "ge" => Ok(BinaryOp::Gte),
        "in" => Ok(BinaryOp::In),
        "not_in" | "not in" => Ok(BinaryOp::NotIn),
        "contains" => Ok(BinaryOp::Contains),
        "starts_with" | "startswith" => Ok(BinaryOp::StartsWith),
        "ends_with" | "endswith" => Ok(BinaryOp::EndsWith),
        "=~" | "regex" => Ok(BinaryOp::RegexMatch),
        "and" | "&" => Ok(BinaryOp::And),
        "or" | "|" => Ok(BinaryOp::Or),
        "xor" | "^" => Ok(BinaryOp::Xor),
        "+" | "add" => Ok(BinaryOp::Add),
        "||" | "concat" => Ok(BinaryOp::Concat),
        "-" | "sub" => Ok(BinaryOp::Sub),
        "*" | "mul" => Ok(BinaryOp::Mul),
        "/" | "div" => Ok(BinaryOp::Div),
        "%" | "mod" => Ok(BinaryOp::Mod),
        other => Err(PyValueError::new_err(format!(
            "Unknown binary operator: '{other}'"
        ))),
    }
}

fn parse_unary_op(s: &str) -> PyResult<UnaryOp> {
    match s.to_ascii_lowercase().as_str() {
        "not" | "~" => Ok(UnaryOp::Not),
        "-" | "neg" => Ok(UnaryOp::Neg),
        "is_null" | "is null" => Ok(UnaryOp::IsNull),
        "is_not_null" | "is not null" => Ok(UnaryOp::IsNotNull),
        other => Err(PyValueError::new_err(format!(
            "Unknown unary operator: '{other}'"
        ))),
    }
}

/// Parses the direction of an `order_by` entry. The schema defines it as a
/// boolean (`true` = ascending) or the uppercase strings `"ASC"`/`"DESC"`, and
/// this function accepts exactly those values so consumer and schema agree.
/// Any other spelling (including lowercase `"asc"`) is rejected with an
/// `INVALID_SPEC` error at the direction element.
fn parse_order_asc(val: &Bound<'_, PyAny>, path: &SpecPath, field: &str) -> PyResult<bool> {
    if let Ok(asc) = val.extract::<bool>() {
        return Ok(asc);
    }
    if let Ok(s) = val.extract::<String>() {
        match s.as_str() {
            "ASC" => return Ok(true),
            "DESC" => return Ok(false),
            _ => {}
        }
    }
    Err(make_spec_err(
        path,
        format!(
            "Invalid '{field}': expected a boolean or 'ASC'/'DESC', got {}.",
            json_type_name(val)
        ),
    ))
}

/// Native AST Expression node handle wrapping a lightweight AST arena.
#[pyclass(name = "AstExpr", from_py_object)]
#[derive(Clone)]
pub struct PyAstExpr {
    pub(crate) arena: QueryAstArena,
    pub(crate) handle: NodeHandle,
}

#[pymethods]
impl PyAstExpr {
    #[staticmethod]
    fn prop(var: String, prop: String) -> Self {
        let mut b = QueryBuilder::new();
        let h = b.prop(var, prop);
        Self {
            arena: b.into_arena(),
            handle: h,
        }
    }

    #[staticmethod]
    fn ident(name: String) -> Self {
        let mut b = QueryBuilder::new();
        let h = b.ident(name);
        Self {
            arena: b.into_arena(),
            handle: h,
        }
    }

    #[staticmethod]
    fn literal(val: &Bound<'_, PyAny>) -> PyResult<Self> {
        let lit = py_to_literal(val)?;
        let mut b = QueryBuilder::new();
        let h = b.literal(lit);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    #[staticmethod]
    fn binary(left: &Bound<'_, PyAny>, op: String, right: &Bound<'_, PyAny>) -> PyResult<Self> {
        let op_parsed = parse_binary_op(&op)?;
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, left, 0, &SpecPath::fluent())?;
        let right_h = py_to_node_handle_depth(&mut b, right, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, op_parsed, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    #[staticmethod]
    fn unary(op: String, operand: &Bound<'_, PyAny>) -> PyResult<Self> {
        let op_parsed = parse_unary_op(&op)?;
        let mut b = QueryBuilder::new();
        let op_h = py_to_node_handle_depth(&mut b, operand, 0, &SpecPath::fluent())?;
        let h = b.unary_expr(op_parsed, op_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    #[staticmethod]
    fn function(name: String, args: Vec<Bound<'_, PyAny>>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let mut arg_handles = Vec::with_capacity(args.len());
        for arg in &args {
            arg_handles.push(py_to_node_handle_depth(
                &mut b,
                arg,
                0,
                &SpecPath::fluent(),
            )?);
        }
        let h = b.function(name, arg_handles);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn in_(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = fluent_membership_operand(&mut b, other, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::In, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn not_in(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = fluent_membership_operand(&mut b, other, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::NotIn, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn contains(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::Contains, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn startswith(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::StartsWith, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn endswith(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::EndsWith, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn is_null(&self) -> Self {
        let mut b = QueryBuilder::new();
        let op_h = b.import_subarena(self.arena.clone(), self.handle);
        let h = b.unary_expr(UnaryOp::IsNull, op_h);
        Self {
            arena: b.into_arena(),
            handle: h,
        }
    }

    fn is_not_null(&self) -> Self {
        let mut b = QueryBuilder::new();
        let op_h = b.import_subarena(self.arena.clone(), self.handle);
        let h = b.unary_expr(UnaryOp::IsNotNull, op_h);
        Self {
            arena: b.into_arena(),
            handle: h,
        }
    }

    fn __add__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::Add, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __radd__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let right_h = b.import_subarena(self.arena.clone(), self.handle);
        let h = b.binary_expr(left_h, BinaryOp::Add, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __sub__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::Sub, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __rsub__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let right_h = b.import_subarena(self.arena.clone(), self.handle);
        let h = b.binary_expr(left_h, BinaryOp::Sub, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __mul__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::Mul, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __rmul__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let right_h = b.import_subarena(self.arena.clone(), self.handle);
        let h = b.binary_expr(left_h, BinaryOp::Mul, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __truediv__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::Div, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __rtruediv__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let right_h = b.import_subarena(self.arena.clone(), self.handle);
        let h = b.binary_expr(left_h, BinaryOp::Div, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __mod__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::Mod, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __rmod__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let right_h = b.import_subarena(self.arena.clone(), self.handle);
        let h = b.binary_expr(left_h, BinaryOp::Mod, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __neg__(&self) -> Self {
        let mut b = QueryBuilder::new();
        let op_h = b.import_subarena(self.arena.clone(), self.handle);
        let h = b.unary_expr(UnaryOp::Neg, op_h);
        Self {
            arena: b.into_arena(),
            handle: h,
        }
    }

    fn __and__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::And, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __rand__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let right_h = b.import_subarena(self.arena.clone(), self.handle);
        let h = b.binary_expr(left_h, BinaryOp::And, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __or__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::Or, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __ror__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let right_h = b.import_subarena(self.arena.clone(), self.handle);
        let h = b.binary_expr(left_h, BinaryOp::Or, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __xor__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, BinaryOp::Xor, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __rxor__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let right_h = b.import_subarena(self.arena.clone(), self.handle);
        let h = b.binary_expr(left_h, BinaryOp::Xor, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __invert__(&self) -> Self {
        let mut b = QueryBuilder::new();
        let op_h = b.import_subarena(self.arena.clone(), self.handle);
        let h = b.unary_expr(UnaryOp::Not, op_h);
        Self {
            arena: b.into_arena(),
            handle: h,
        }
    }

    fn __bool__(&self) -> PyResult<bool> {
        Err(PyTypeError::new_err(
            "Evaluating a Voyager AstExpr in a boolean context is not supported.",
        ))
    }

    fn __richcmp__(
        &self,
        other: &Bound<'_, PyAny>,
        op: pyo3::pyclass::CompareOp,
    ) -> PyResult<Self> {
        let bin_op = match op {
            pyo3::pyclass::CompareOp::Eq => BinaryOp::Eq,
            pyo3::pyclass::CompareOp::Ne => BinaryOp::Neq,
            pyo3::pyclass::CompareOp::Lt => BinaryOp::Lt,
            pyo3::pyclass::CompareOp::Le => BinaryOp::Lte,
            pyo3::pyclass::CompareOp::Gt => BinaryOp::Gt,
            pyo3::pyclass::CompareOp::Ge => BinaryOp::Gte,
        };
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0, &SpecPath::fluent())?;
        let h = b.binary_expr(left_h, bin_op, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __repr__(&self) -> String {
        format!("<AstExpr handle={:?}>", self.handle)
    }
}

/// Maximum recursion depth allowed when converting Python expression trees to AST node handles.
const MAX_AST_DEPTH: usize = 256;

fn is_known_expr_tag(tag: &str) -> bool {
    matches!(
        tag,
        "prop"
            | "ident"
            | "param"
            | "lit"
            | "literal"
            | "bin"
            | "unary"
            | "fn"
            | "func"
            | "call"
            | "case"
            | "list_comp"
            | "pattern_comp"
            | "exists"
            | "count"
            | "list"
            | "alias"
    )
}

fn py_to_node_handle(builder: &mut QueryBuilder, val: &Bound<'_, PyAny>) -> PyResult<NodeHandle> {
    py_to_node_handle_depth(builder, val, 0, &SpecPath::fluent())
}

#[inline(never)]
fn try_import_native_ast(builder: &mut QueryBuilder, val: &Bound<'_, PyAny>) -> Option<NodeHandle> {
    if let Ok(py_expr) = val.cast::<PyAstExpr>() {
        let borrowed = py_expr.borrow();
        return Some(builder.import_subarena(borrowed.arena.clone(), borrowed.handle));
    }
    if let Ok(native_attr) = val.getattr("_native_expr")
        && let Ok(py_expr) = native_attr.cast::<PyAstExpr>()
    {
        let borrowed = py_expr.borrow();
        return Some(builder.import_subarena(borrowed.arena.clone(), borrowed.handle));
    }
    None
}

#[inline(never)]
fn handle_dict_expr(
    builder: &mut QueryBuilder,
    dict: &Bound<'_, PyDict>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    if let Some(ver_item) = dict.get_item("spec_version")? {
        let ver_path = path.key("spec_version");
        let ver: u32 = extract_required(&ver_item, &ver_path, "an integer major version")?;
        if ver != 1 {
            return Err(make_spec_err(
                &ver_path,
                format!("Unsupported expression spec_version: {ver}. Supported major versions: 1."),
            ));
        }
    }
    if let Some(inner_expr) = dict.get_item("expr")? {
        if path.is_spec() {
            reject_unknown_keys(dict, path, &["spec_version", "expr"], "expression object")?;
        }
        return py_to_node_handle_depth(builder, &inner_expr, depth + 1, &path.key("expr"));
    }
    if path.is_spec() {
        // A bare object in an expression position is not a query spec: nested
        // query specs are only accepted by the dedicated `exists`/`count`
        // payload and `pattern_comp` path readers, which call the subquery
        // builder directly. Everywhere else an object must wrap its expression
        // under an "expr" key.
        return Err(make_spec_err(
            path,
            "expected an object-wrapped expression with an 'expr' key; a nested query spec is only accepted by the 'exists', 'count', and 'pattern_comp' payloads",
        ));
    }
    let dict_cloned = dict.clone();
    let mut subquery_err: Option<PyErr> = None;
    let sub_h = builder.subquery(|q| {
        if let Err(e) = build_query_from_spec_internal(q, &dict_cloned, depth + 1, path) {
            subquery_err = Some(e);
        }
    });
    if let Some(e) = subquery_err {
        return Err(e);
    }
    Ok(sub_h)
}

/// Reads a Query Spec v1 array position as a sequence of items, accepting both
/// spellings the protocol allows: a Python `list` (the JSON array form, e.g.
/// after `json.loads`/`json.dumps`) and a Python `tuple` (the form the Python
/// SDK emits). The two are interchangeable, so every array position goes
/// through here instead of ad-hoc `PyTuple`/`PyList` casts.
fn as_spec_sequence<'a>(val: &Bound<'a, PyAny>) -> Option<Bound<'a, PyTuple>> {
    if let Ok(tuple) = val.cast::<PyTuple>() {
        Some(tuple.clone())
    } else if let Ok(list) = val.cast::<PyList>() {
        Some(list.to_tuple())
    } else {
        None
    }
}

/// Reads a spec array position whose shape is part of the v1 contract. A
/// non-sequence value fails with `INVALID_SPEC` at `path` (or `INVALID_CLAUSE`
/// for a fluent caller) instead of a bare PyO3 `TypeError`.
fn expect_spec_array<'a>(
    val: &Bound<'a, PyAny>,
    path: &SpecPath,
    field: &str,
) -> PyResult<Bound<'a, PyTuple>> {
    as_spec_sequence(val).ok_or_else(|| {
        path_error(
            path,
            format!(
                "Invalid '{field}': expected a JSON array (list or tuple), got {}.",
                json_type_name(val)
            ),
        )
    })
}

/// Validates the item count of a fixed-arity spec array so a too-short
/// sequence fails with a clear, field-named error instead of the bare
/// `IndexError` a later `get_item` would raise.
fn check_spec_arity(
    tuple: &Bound<'_, PyTuple>,
    path: &SpecPath,
    field: &str,
    min_items: usize,
    shape: &str,
) -> PyResult<()> {
    let len = tuple.len();
    if len < min_items {
        return Err(path_error(
            path,
            format!("Invalid '{field}': expected at least {min_items} item(s) {shape}, got {len}."),
        ));
    }
    Ok(())
}

/// Validates the item count of a fixed-arity spec array against both bounds.
///
/// Every fixed shape in the v1 schema carries `maxItems`; an item past the
/// documented shape is a spec error at the extra item's own path instead of
/// being read and silently dropped. Path steps use the narrower
/// [`reject_extra_step_items`] tolerance, which still accepts trailing `null`
/// placeholders.
fn check_spec_arity_bounded(
    tuple: &Bound<'_, PyTuple>,
    path: &SpecPath,
    field: &str,
    min_items: usize,
    max_items: usize,
    shape: &str,
) -> PyResult<()> {
    check_spec_arity(tuple, path, field, min_items, shape)?;
    if tuple.len() > max_items {
        let extra = tuple.get_item(max_items)?;
        return Err(path_error(
            &path.index(max_items),
            format!(
                "Invalid '{field}': unexpected {} after the documented shape {shape}. This position accepts at most {max_items} item(s).",
                json_type_name(&extra)
            ),
        ));
    }
    Ok(())
}

/// Strips one leading `$` from a parameter name, matching the Python SDK's
/// `param()` helper (`param("$batch")` and `param("batch")` are the same
/// parameter). Exactly one `$` is removed; `"$$x"` still names `"$x"`.
fn strip_leading_dollar(name: String) -> String {
    match name.strip_prefix('$') {
        Some(stripped) => stripped.to_string(),
        None => name,
    }
}

/// Rejects items past a path step's documented shape unless they are `null`.
///
/// A node step documents `[node, variable, labels, properties]` and an edge
/// step documents `[edge, direction, types, variable, min_hops, max_hops,
/// properties]`. Trailing `null` placeholders stay accepted; any other extra
/// item is a spec error rather than being dropped.
fn reject_extra_step_items(
    tuple: &Bound<'_, PyTuple>,
    path: &SpecPath,
    field: &str,
    documented_items: usize,
    shape: &str,
) -> PyResult<()> {
    for idx in documented_items..tuple.len() {
        let item = tuple.get_item(idx)?;
        if !item.is_none() {
            return Err(path_error(
                &path.index(idx),
                format!(
                    "Invalid {field}: unexpected {} after the documented shape {shape}. This path step accepts no further items.",
                    json_type_name(&item)
                ),
            ));
        }
    }
    Ok(())
}

/// Parses a path step's optional properties item into `property -> expression`
/// pairs.
///
/// Omitted and `null` both mean "no properties"; an empty object is treated the
/// same way. A non-object is rejected at the properties item's path, and each
/// value is parsed with the strict tagged-expression rule, so a bare scalar
/// fails at `/.../properties/<name>` with the `["lit", value]` hint.
fn parse_step_properties(
    builder: &mut QueryBuilder,
    tuple: &Bound<'_, PyTuple>,
    idx: usize,
    depth: usize,
    step_path: &SpecPath,
    field: &str,
) -> PyResult<Vec<(String, NodeHandle)>> {
    let Some(item) = optional_step_item(tuple, idx)? else {
        return Ok(Vec::new());
    };
    if item.is_none() {
        return Ok(Vec::new());
    }
    let properties_path = step_path.index(idx);
    let dict = item.cast::<PyDict>().map_err(|_| {
        path_error(
            &properties_path,
            format!(
                "Invalid {field} properties: expected a JSON object mapping property names to expressions, or null, got {}.",
                json_type_name(&item)
            ),
        )
    })?;
    let mut properties = Vec::with_capacity(dict.len());
    for (key, value) in dict.iter() {
        let name: String = key.extract().map_err(|_| {
            path_error(
                &properties_path,
                format!(
                    "Invalid {field} properties: property names must be strings, got {}.",
                    json_type_name(&key)
                ),
            )
        })?;
        let value_path = properties_path.key(&name);
        let value_handle = py_to_node_handle_depth(builder, &value, depth, &value_path)?;
        properties.push((name, value_handle));
    }
    Ok(properties)
}

/// Returns item `idx` of a path step, or `None` when the step is shorter.
fn optional_step_item<'py>(
    tuple: &Bound<'py, PyTuple>,
    idx: usize,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    if idx < tuple.len() {
        Ok(Some(tuple.get_item(idx)?))
    } else {
        Ok(None)
    }
}

/// Recovers the alias of an `['alias', inner_expression, name]` expression spec
/// when a projection carries no explicit alias. Sequence-agnostic: works for
/// the tuple spelling and the JSON list spelling alike, so a round-tripped
/// spec keeps its alias.
fn extract_alias_from_expr_spec(
    expr_spec: &Bound<'_, PyAny>,
    path: &SpecPath,
) -> PyResult<Option<String>> {
    if let Some(seq) = as_spec_sequence(expr_spec)
        && !seq.is_empty()
        && seq
            .get_item(0)?
            .extract::<String>()
            .map(|t| t == "alias")
            .unwrap_or(false)
    {
        check_spec_arity_bounded(
            &seq,
            path,
            "expression 'alias'",
            3,
            3,
            "[alias, expression, name]",
        )?;
        let name: String =
            extract_required(&seq.get_item(2)?, &path.index(2), "a string alias name")?;
        return Ok(Some(name));
    }
    Ok(None)
}

#[inline(never)]
fn handle_prop_tuple(
    builder: &mut QueryBuilder,
    tuple: &Bound<'_, PyTuple>,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    check_spec_arity_bounded(
        tuple,
        path,
        "expression 'prop'",
        3,
        3,
        "[prop, variable, property]",
    )?;
    let var: String = extract_required(&tuple.get_item(1)?, &path.index(1), "a string variable")?;
    if var.is_empty() {
        return Err(path_error(
            &path.index(1),
            "Invalid 'prop': the variable must be a non-empty string.",
        ));
    }
    let prop: String = extract_required(&tuple.get_item(2)?, &path.index(2), "a string property")?;
    Ok(builder.prop(var, prop))
}

#[inline(never)]
fn handle_ident_tuple(
    builder: &mut QueryBuilder,
    tuple: &Bound<'_, PyTuple>,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    check_spec_arity_bounded(tuple, path, "expression 'ident'", 2, 2, "[ident, name]")?;
    let name: String = extract_required(&tuple.get_item(1)?, &path.index(1), "a string name")?;
    if name.is_empty() {
        return Err(path_error(
            &path.index(1),
            "Invalid 'ident': the identifier must be a non-empty string.",
        ));
    }
    Ok(builder.ident(name))
}

#[inline(never)]
fn handle_param_tuple(
    builder: &mut QueryBuilder,
    tuple: &Bound<'_, PyTuple>,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    check_spec_arity_bounded(tuple, path, "expression 'param'", 2, 2, "[param, name]")?;
    let name: String = extract_required(&tuple.get_item(1)?, &path.index(1), "a string name")?;
    let name = strip_leading_dollar(name);
    if name.is_empty() {
        return Err(path_error(
            &path.index(1),
            "Invalid 'param': the parameter name must be a non-empty string.",
        ));
    }
    Ok(builder.param(name))
}

#[inline(never)]
fn handle_lit_tuple(
    builder: &mut QueryBuilder,
    tuple: &Bound<'_, PyTuple>,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    check_spec_arity_bounded(tuple, path, "expression 'lit'", 2, 2, "[lit, value]")?;
    let value_path = path.index(1);
    let lit = py_to_literal(&tuple.get_item(1)?)
        .map_err(|e| path_value_error(&value_path, e.to_string()))?;
    Ok(builder.literal(lit))
}

#[inline(never)]
fn handle_fn_tuple(
    builder: &mut QueryBuilder,
    tuple: &Bound<'_, PyTuple>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    check_spec_arity_bounded(tuple, path, "expression 'fn'", 3, 3, "[fn, name, args]")?;
    let name: String = extract_required(&tuple.get_item(1)?, &path.index(1), "a string name")?;
    let args_path = path.index(2);
    let args_seq = expect_spec_array(&tuple.get_item(2)?, &args_path, "fn args")?;
    let mut arg_handles = Vec::with_capacity(args_seq.len());
    for (idx, arg) in args_seq.iter().enumerate() {
        arg_handles.push(py_to_node_handle_depth(
            builder,
            &arg,
            depth + 1,
            &args_path.index(idx),
        )?);
    }
    Ok(builder.function(name, arg_handles))
}

#[inline(never)]
fn handle_case_tuple(
    builder: &mut QueryBuilder,
    tuple: &Bound<'_, PyTuple>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    check_spec_arity_bounded(
        tuple,
        path,
        "expression 'case'",
        4,
        4,
        "[case, operand, branches, else_branch]",
    )?;
    let op_item = tuple.get_item(1)?;
    let operand = if op_item.is_none() {
        None
    } else {
        Some(py_to_node_handle_depth(
            builder,
            &op_item,
            depth + 1,
            &path.index(1),
        )?)
    };
    let branches_path = path.index(2);
    let branches_seq = expect_spec_array(&tuple.get_item(2)?, &branches_path, "case branches")?;
    let mut branches = Vec::with_capacity(branches_seq.len());
    for (idx, item) in branches_seq.iter().enumerate() {
        let branch_path = branches_path.index(idx);
        let branch = expect_spec_array(&item, &branch_path, "case branch")?;
        check_spec_arity_bounded(&branch, &branch_path, "case branch", 2, 2, "[when, then]")?;
        let w = py_to_node_handle_depth(
            builder,
            &branch.get_item(0)?,
            depth + 1,
            &branch_path.index(0),
        )?;
        let t = py_to_node_handle_depth(
            builder,
            &branch.get_item(1)?,
            depth + 1,
            &branch_path.index(1),
        )?;
        branches.push((w, t));
    }
    let else_item = tuple.get_item(3)?;
    let else_branch = if else_item.is_none() {
        None
    } else {
        Some(py_to_node_handle_depth(
            builder,
            &else_item,
            depth + 1,
            &path.index(3),
        )?)
    };
    Ok(builder.case_when(operand, branches, else_branch))
}

#[inline(never)]
fn handle_list_comp_tuple(
    builder: &mut QueryBuilder,
    tuple: &Bound<'_, PyTuple>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    check_spec_arity_bounded(
        tuple,
        path,
        "expression 'list_comp'",
        5,
        5,
        "[list_comp, variable, list_expr, where_filter, map_expr]",
    )?;
    let var: String = extract_required(&tuple.get_item(1)?, &path.index(1), "a string variable")?;
    let list_h = py_to_node_handle_depth(builder, &tuple.get_item(2)?, depth + 1, &path.index(2))?;
    let wh_item = tuple.get_item(3)?;
    let where_filter = if wh_item.is_none() {
        None
    } else {
        Some(py_to_node_handle_depth(
            builder,
            &wh_item,
            depth + 1,
            &path.index(3),
        )?)
    };
    let map_item = tuple.get_item(4)?;
    let map_expr = if map_item.is_none() {
        None
    } else {
        Some(py_to_node_handle_depth(
            builder,
            &map_item,
            depth + 1,
            &path.index(4),
        )?)
    };
    Ok(builder.list_comprehension(var, list_h, where_filter, map_expr))
}

#[inline(never)]
fn handle_pattern_comp_tuple(
    builder: &mut QueryBuilder,
    tuple: &Bound<'_, PyTuple>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    check_spec_arity_bounded(
        tuple,
        path,
        "expression 'pattern_comp'",
        4,
        4,
        "[pattern_comp, path_spec, where_filter, proj_expr]",
    )?;
    let path_spec = tuple.get_item(1)?;
    let path_h = build_path_from_py(builder, &path_spec, depth + 1, &path.index(1))?;
    let wh_item = tuple.get_item(2)?;
    let where_filter = if wh_item.is_none() {
        None
    } else {
        Some(py_to_node_handle_depth(
            builder,
            &wh_item,
            depth + 1,
            &path.index(2),
        )?)
    };
    let proj_h = py_to_node_handle_depth(builder, &tuple.get_item(3)?, depth + 1, &path.index(3))?;
    Ok(builder.pattern_comprehension(path_h, where_filter, proj_h))
}

#[inline(never)]
fn handle_exists_tuple(
    builder: &mut QueryBuilder,
    tuple: &Bound<'_, PyTuple>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    check_spec_arity_bounded(
        tuple,
        path,
        "expression 'exists'",
        2,
        2,
        "[exists, subquery_spec]",
    )?;
    let sub_spec = tuple.get_item(1)?;
    let sub_h = build_subquery_from_py(builder, &sub_spec, depth + 1, &path.index(1))?;
    Ok(builder.exists_subquery(sub_h))
}

#[inline(never)]
fn handle_count_tuple(
    builder: &mut QueryBuilder,
    tuple: &Bound<'_, PyTuple>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    check_spec_arity_bounded(
        tuple,
        path,
        "expression 'count'",
        2,
        2,
        "[count, subquery_spec]",
    )?;
    let sub_spec = tuple.get_item(1)?;
    let sub_h = build_subquery_from_py(builder, &sub_spec, depth + 1, &path.index(1))?;
    Ok(builder.count_subquery(sub_h))
}

#[inline(never)]
fn handle_list_literal_tuple(
    builder: &mut QueryBuilder,
    tuple: &Bound<'_, PyTuple>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    check_spec_arity_bounded(tuple, path, "expression 'list'", 2, 2, "[list, items]")?;
    let items_path = path.index(1);
    let items_seq = expect_spec_array(&tuple.get_item(1)?, &items_path, "list items")?;
    let mut item_handles = Vec::with_capacity(items_seq.len());
    for (idx, item) in items_seq.iter().enumerate() {
        item_handles.push(py_to_node_handle_depth(
            builder,
            &item,
            depth + 1,
            &items_path.index(idx),
        )?);
    }
    Ok(builder.list_literal(item_handles))
}

#[inline(never)]
fn handle_literal_fallback(
    builder: &mut QueryBuilder,
    val: &Bound<'_, PyAny>,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    if let Ok(lit) = py_to_literal(val) {
        Ok(builder.literal(lit))
    } else {
        let msg = format!(
            "Cannot convert Python object to AST node handle: {}",
            json_type_name(val)
        );
        if path.is_spec() {
            Err(make_spec_err(path, msg))
        } else {
            Err(make_voyager_err(voyager_ogm::ErrorCode::InvalidSpec, msg))
        }
    }
}

fn py_to_node_handle_depth(
    builder: &mut QueryBuilder,
    val: &Bound<'_, PyAny>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    if depth > MAX_AST_DEPTH {
        return Err(path_value_error(
            path,
            format!(
                "Maximum AST expression recursion depth exceeded ({MAX_AST_DEPTH}). Query is too deeply nested.",
            ),
        ));
    }
    // Direct Native AST Expression fast-path:
    if let Some(h) = try_import_native_ast(builder, val) {
        return Ok(h);
    }
    if let Ok(dict) = val.cast::<PyDict>() {
        return handle_dict_expr(builder, dict, depth, path);
    }
    // In an expression position a sequence is always a tagged expression: there
    // is no implicit literal-list spelling. Lists and tuples are accepted
    // interchangeably, but the first item must be a known expression tag.
    if let Some(tuple) = as_spec_sequence(val) {
        return handle_expr_sequence(builder, tuple, depth, path);
    }
    if path.is_spec() {
        return Err(make_spec_err(
            path,
            format!(
                "expected an expression (a tagged JSON array or an object-wrapped expression), got {}; write a scalar literal as [\"lit\", value]",
                json_type_name(val)
            ),
        ));
    }
    handle_literal_fallback(builder, val, path)
}

/// Suggestion appended to expression-array errors, naming the two explicit
/// spellings for a list value.
const EXPRESSION_ARRAY_HINT: &str =
    "Use [\"lit\", [...]] for a literal list or [\"list\", [...]] for a list of expressions.";

/// Validates the first item of an expression sequence and returns its tag.
///
/// A JSON array in an expression position is never a literal list: it must be a
/// tagged expression descriptor, so the first item must be a known expression
/// tag. Empty sequences, non-string first items and unknown tags are rejected at
/// the sequence's own JSON Pointer.
fn expression_tag(tuple: &Bound<'_, PyTuple>, path: &SpecPath) -> PyResult<String> {
    if tuple.is_empty() {
        return Err(path_error(
            path,
            format!(
                "Empty expression array: an expression array must start with a known expression tag. {}",
                EXPRESSION_ARRAY_HINT
            ),
        ));
    }
    let first = tuple.get_item(0)?;
    let tag = match first.extract::<String>() {
        Ok(tag) => tag,
        Err(_) => {
            let type_name = json_type_name(&first);
            let rendered = match render_json_scalar(&first) {
                Some(value) => format!("got {value} ({type_name})"),
                None => format!("got {type_name}"),
            };
            return Err(path_error(
                path,
                format!(
                    "Invalid expression array: the first element must be a known expression tag string, {rendered}. {}",
                    EXPRESSION_ARRAY_HINT
                ),
            ));
        }
    };
    if !is_known_expr_tag(&tag) {
        return Err(path_error(
            path,
            format!(
                "Unknown expression tag '{tag}': the first element of an expression array must be a known expression tag. {}",
                EXPRESSION_ARRAY_HINT
            ),
        ));
    }
    Ok(tag)
}

#[inline(never)]
fn handle_expr_sequence(
    builder: &mut QueryBuilder,
    tuple: Bound<'_, PyTuple>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    let tag = expression_tag(&tuple, path)?;
    match tag.as_str() {
        "prop" => handle_prop_tuple(builder, &tuple, path),
        "ident" => handle_ident_tuple(builder, &tuple, path),
        "param" => handle_param_tuple(builder, &tuple, path),
        "lit" | "literal" => handle_lit_tuple(builder, &tuple, path),
        "bin" => {
            check_spec_arity_bounded(
                &tuple,
                path,
                "expression 'bin'",
                4,
                4,
                "[bin, operator, left, right]",
            )?;
            let op_path = path.index(1);
            let op_str: String =
                extract_required(&tuple.get_item(1)?, &op_path, "a string operator")?;
            let op =
                parse_binary_op(&op_str).map_err(|e| path_value_error(&op_path, e.to_string()))?;
            let left =
                py_to_node_handle_depth(builder, &tuple.get_item(2)?, depth + 1, &path.index(2))?;
            let right =
                py_to_node_handle_depth(builder, &tuple.get_item(3)?, depth + 1, &path.index(3))?;
            Ok(builder.binary_expr(left, op, right))
        }
        "unary" => {
            check_spec_arity_bounded(
                &tuple,
                path,
                "expression 'unary'",
                3,
                3,
                "[unary, operator, operand]",
            )?;
            let op_path = path.index(1);
            let op_str: String =
                extract_required(&tuple.get_item(1)?, &op_path, "a string operator")?;
            let op =
                parse_unary_op(&op_str).map_err(|e| path_value_error(&op_path, e.to_string()))?;
            let operand =
                py_to_node_handle_depth(builder, &tuple.get_item(2)?, depth + 1, &path.index(2))?;
            Ok(builder.unary_expr(op, operand))
        }
        "fn" | "func" | "call" => handle_fn_tuple(builder, &tuple, depth, path),
        "case" => handle_case_tuple(builder, &tuple, depth, path),
        "list_comp" => handle_list_comp_tuple(builder, &tuple, depth, path),
        "pattern_comp" => handle_pattern_comp_tuple(builder, &tuple, depth, path),
        "exists" => handle_exists_tuple(builder, &tuple, depth, path),
        "count" => handle_count_tuple(builder, &tuple, depth, path),
        "list" => handle_list_literal_tuple(builder, &tuple, depth, path),
        "alias" => {
            check_spec_arity_bounded(
                &tuple,
                path,
                "expression 'alias'",
                3,
                3,
                "[alias, expression, name]",
            )?;
            let _: String =
                extract_required(&tuple.get_item(2)?, &path.index(2), "a string alias name")?;
            py_to_node_handle_depth(builder, &tuple.get_item(1)?, depth + 1, &path.index(1))
        }
        other => Err(path_error(
            path,
            format!(
                "Unknown expression tag '{other}': the first element of an expression array must be a known expression tag. {}",
                EXPRESSION_ARRAY_HINT
            ),
        )),
    }
}

fn parse_direction(dir_str: &str) -> PyResult<Direction> {
    match dir_str.to_ascii_lowercase().as_str() {
        "out" | "outgoing" | "->" => Ok(Direction::Outgoing),
        "in" | "incoming" | "<-" => Ok(Direction::Incoming),
        "undirected" | "both" | "-" | "--" => Ok(Direction::Undirected),
        other => Err(make_voyager_err(
            voyager_ogm::ErrorCode::InvalidClause,
            format!(
                "Invalid edge direction '{other}'. Expected 'outgoing' ('out', '->'), 'incoming' ('in', '<-'), or 'undirected' ('both', '-', '--')."
            ),
        )),
    }
}

fn build_path_steps_into_builder(
    builder: &mut QueryBuilder,
    steps: &Bound<'_, PyTuple>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<()> {
    for (idx, step) in steps.iter().enumerate() {
        let step_path = path.index(idx);
        let tuple = expect_spec_array(&step, &step_path, "path step")?;
        let tag_path = step_path.index(0);
        let tag: String =
            extract_required(&tuple.get_item(0)?, &tag_path, "a string path step tag")?;
        match tag.as_str() {
            "node" => {
                check_spec_arity(
                    &tuple,
                    &step_path,
                    "path step 'node'",
                    3,
                    "[node, variable, labels, properties?]",
                )?;
                reject_extra_step_items(
                    &tuple,
                    &step_path,
                    "path step 'node'",
                    4,
                    "[node, variable, labels, properties]",
                )?;
                let var: Option<String> = extract_required(
                    &tuple.get_item(1)?,
                    &step_path.index(1),
                    "a string or null variable",
                )?;
                let labels: Vec<String> = extract_required(
                    &tuple.get_item(2)?,
                    &step_path.index(2),
                    "an array of string labels",
                )?;
                let properties = parse_step_properties(
                    builder,
                    &tuple,
                    3,
                    depth,
                    &step_path,
                    "path step 'node'",
                )?;
                if properties.is_empty() {
                    builder.node(var, labels);
                } else {
                    builder.node_with_prop_exprs(var, labels, properties);
                }
            }
            "edge" => {
                check_spec_arity(
                    &tuple,
                    &step_path,
                    "path step 'edge'",
                    3,
                    "[edge, direction, types, variable?, min_hops?, max_hops?, properties?]",
                )?;
                reject_extra_step_items(
                    &tuple,
                    &step_path,
                    "path step 'edge'",
                    7,
                    "[edge, direction, types, variable, min_hops, max_hops, properties]",
                )?;
                let dir_path = step_path.index(1);
                let dir_str: String =
                    extract_required(&tuple.get_item(1)?, &dir_path, "a string edge direction")?;
                let dir =
                    parse_direction(&dir_str).map_err(|e| path_error(&dir_path, e.to_string()))?;
                let edge_types: Vec<String> = extract_required(
                    &tuple.get_item(2)?,
                    &step_path.index(2),
                    "an array of string edge types",
                )?;
                let var: Option<String> = match optional_step_item(&tuple, 3)? {
                    Some(item) => {
                        extract_required(&item, &step_path.index(3), "a string or null variable")?
                    }
                    None => None,
                };
                let min_hops: Option<u32> = match optional_step_item(&tuple, 4)? {
                    Some(item) => extract_required(
                        &item,
                        &step_path.index(4),
                        "a non-negative integer or null",
                    )?,
                    None => None,
                };
                let max_hops: Option<u32> = match optional_step_item(&tuple, 5)? {
                    Some(item) => extract_required(
                        &item,
                        &step_path.index(5),
                        "a non-negative integer or null",
                    )?,
                    None => None,
                };
                let properties = parse_step_properties(
                    builder,
                    &tuple,
                    6,
                    depth,
                    &step_path,
                    "path step 'edge'",
                )?;
                match (dir, properties.is_empty()) {
                    (Direction::Outgoing, true) => builder.to(edge_types, var),
                    (Direction::Incoming, true) => builder.from(edge_types, var),
                    (Direction::Undirected, true) => builder.edge(edge_types, var),
                    (Direction::Outgoing, false) => {
                        builder.to_with_prop_exprs(edge_types, var, properties)
                    }
                    (Direction::Incoming, false) => {
                        builder.from_with_prop_exprs(edge_types, var, properties)
                    }
                    (Direction::Undirected, false) => {
                        builder.edge_with_prop_exprs(edge_types, var, properties)
                    }
                };
                if min_hops.is_some() || max_hops.is_some() {
                    let min = min_hops.unwrap_or(1);
                    let max = max_hops.unwrap_or(min);
                    builder.hops(min, max);
                }
            }
            other => {
                return Err(path_error(
                    &tag_path,
                    format!("Unknown path step tag: '{other}'. Expected 'node' or 'edge'."),
                ));
            }
        }
    }
    Ok(())
}

/// Parses one `order_by` entry: `[variable, property, direction]` or
/// `[expression, direction]`, where `direction` is a boolean or `"ASC"`/
/// `"DESC"`. Accepts the tuple spelling and the JSON list spelling alike, and
/// fails with a clear, field-named error on wrong shapes or directions.
fn build_order_by_entry(
    builder: &mut QueryBuilder,
    item: &Bound<'_, PyAny>,
    depth: usize,
    path: &SpecPath,
    field: &str,
) -> PyResult<()> {
    let tuple = expect_spec_array(item, path, field)?;
    check_spec_arity(
        &tuple,
        path,
        field,
        2,
        "[variable, property, direction] or [expression, direction]",
    )?;
    let first = tuple.get_item(0)?;
    if let Ok(var) = first.extract::<String>() {
        check_spec_arity_bounded(&tuple, path, field, 3, 3, "[variable, property, direction]")?;
        let prop: String =
            extract_required(&tuple.get_item(1)?, &path.index(1), "a string property")?;
        let asc = parse_order_asc(&tuple.get_item(2)?, &path.index(2), field)?;
        builder.order_by_property(var, prop, asc);
    } else {
        check_spec_arity_bounded(&tuple, path, field, 2, 2, "[expression, direction]")?;
        let expr_h = py_to_node_handle_depth(builder, &first, depth, &path.index(0))?;
        let asc = parse_order_asc(&tuple.get_item(1)?, &path.index(1), field)?;
        builder.order_by(expr_h, asc);
    }
    Ok(())
}

/// Builds the predicates of a `where` position, shared by match and with
/// clauses.
///
/// The schema defines `where` as one `Expression` or an array of `Expression`s.
/// A single `Expression` is itself a JSON array that starts with a tag string,
/// so the two forms are told apart deterministically by the first element:
///
/// - an empty array is an empty predicate list (no predicates are emitted);
/// - an array whose first element is a string is one expression;
/// - an array whose first element is an array, tuple, or object is a list of
///   expressions.
///
/// A non-array value is one expression (the object-wrapped form, or a value the
/// expression parser rejects). A `where` array whose first element is any other
/// scalar matches neither schema branch and is rejected at the `where` path.
fn build_where_predicates(
    builder: &mut QueryBuilder,
    where_item: &Bound<'_, PyAny>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<()> {
    if let Some(seq) = as_spec_sequence(where_item) {
        if seq.is_empty() {
            return Ok(());
        }
        let first = seq.get_item(0)?;
        if first.extract::<String>().is_ok() {
            let h = py_to_node_handle_depth(builder, where_item, depth, path)?;
            builder.where_expr(h);
            return Ok(());
        }
        if first.cast::<PyDict>().is_ok() || as_spec_sequence(&first).is_some() {
            for (idx, predicate) in seq.iter().enumerate() {
                let pred_path = path.index(idx);
                let h = py_to_node_handle_depth(builder, &predicate, depth, &pred_path)?;
                builder.where_expr(h);
            }
            return Ok(());
        }
        let type_name = json_type_name(&first);
        let rendered = match render_json_scalar(&first) {
            Some(value) => format!("first element {value} ({type_name})"),
            None => format!("first element of type {type_name}"),
        };
        return Err(make_spec_err(
            path,
            format!(
                "Invalid 'where': a 'where' array must be one expression (starting with a string tag) or an array of expressions (starting with an array or object), got {rendered}. {}",
                EXPRESSION_ARRAY_HINT
            ),
        ));
    }
    let h = py_to_node_handle_depth(builder, where_item, depth, path)?;
    builder.where_expr(h);
    Ok(())
}

fn build_path_from_py(
    builder: &mut QueryBuilder,
    spec: &Bound<'_, PyAny>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    let target = if let Ok(native) = spec.getattr("_native") {
        native
    } else {
        spec.clone()
    };
    if let Ok(py_builder) = target.extract::<PyRef<PyQueryBuilder>>() {
        let sub = py_builder.inner.clone();
        let (sub_arena, sub_handle) = sub.build();
        let sub_h = builder.import_subarena(sub_arena, sub_handle);
        return Ok(sub_h);
    }
    if let Some(steps) = as_spec_sequence(spec) {
        let mut err: Option<PyErr> = None;
        let sub_h = builder.subquery(|q| {
            if let Err(e) = build_path_steps_into_builder(q, &steps, depth, path) {
                err = Some(e);
            }
        });
        if let Some(e) = err {
            return Err(e);
        }
        Ok(sub_h)
    } else {
        py_to_node_handle_depth(builder, spec, depth, path)
    }
}

fn build_subquery_from_py(
    builder: &mut QueryBuilder,
    spec: &Bound<'_, PyAny>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<NodeHandle> {
    let target = if let Ok(native) = spec.getattr("_native") {
        native
    } else {
        spec.clone()
    };
    if let Ok(py_builder) = target.extract::<PyRef<PyQueryBuilder>>() {
        let sub = py_builder.inner.clone();
        let (sub_arena, sub_handle) = sub.build();
        let sub_h = builder.import_subarena(sub_arena, sub_handle);
        return Ok(sub_h);
    }
    if let Ok(dict) = spec.cast::<PyDict>() {
        let dict_cloned = dict.clone();
        let mut err: Option<PyErr> = None;
        let sub_h = builder.subquery(|q| {
            if let Err(e) = build_query_from_spec_internal(q, &dict_cloned, depth, path) {
                err = Some(e);
            }
        });
        if let Some(e) = err {
            return Err(e);
        }
        Ok(sub_h)
    } else if let Some(steps) = as_spec_sequence(spec) {
        let mut err: Option<PyErr> = None;
        let sub_h = builder.subquery(|q| {
            q.r#match();
            if let Err(e) = build_path_steps_into_builder(q, &steps, depth, path) {
                err = Some(e);
            }
        });
        if let Some(e) = err {
            return Err(e);
        }
        Ok(sub_h)
    } else {
        py_to_node_handle_depth(builder, spec, depth, path)
    }
}

const ROOT_SPEC_KEYS: &[&str] = &[
    "spec_version",
    "load_csv",
    "unwinds",
    "matches",
    "mutations",
    "with_clauses",
    "projections",
    "distinct",
    "order_by",
    "skip",
    "offset",
    "limit",
    "execution_mode",
    // Supported by the consumer and exercised by the round-trip suite, but not
    // yet listed in the v1 schema (schema completeness is tracked separately).
    "linear_clauses",
];

const MATCH_SPEC_KEYS: &[&str] = &[
    "spec_version",
    "optional",
    "path_mode",
    "path_variable",
    "paths",
    "where",
];

const WITH_SPEC_KEYS: &[&str] = &[
    "spec_version",
    "distinct",
    "projections",
    "where",
    "order_by",
    "skip",
    "limit",
];

/// Rejects any object key that is not in the closed v1 key set for that object.
fn reject_unknown_keys(
    dict: &Bound<'_, PyDict>,
    path: &SpecPath,
    allowed: &[&str],
    what: &str,
) -> PyResult<()> {
    for (key, _value) in dict.iter() {
        let key_str: String = match key.extract() {
            Ok(key_str) => key_str,
            Err(_) => {
                return Err(make_spec_err(
                    path,
                    format!(
                        "invalid {what}: object keys must be strings, got {}",
                        json_type_name(&key)
                    ),
                ));
            }
        };
        if !allowed.contains(&key_str.as_str()) {
            return Err(make_spec_err(
                &path.key(&key_str),
                format!(
                    "unknown {what} key '{key_str}'. Allowed keys: {}.",
                    allowed.join(", ")
                ),
            ));
        }
    }
    Ok(())
}

/// Parses a non-negative `skip` / `offset` / `limit` value.
fn parse_non_negative(val: &Bound<'_, PyAny>, path: &SpecPath, field: &str) -> PyResult<u64> {
    if val.is_instance_of::<PyBool>() {
        return Err(make_spec_err(
            path,
            format!(
                "Invalid '{field}': expected a non-negative integer, got {}.",
                json_type_name(val)
            ),
        ));
    }
    let value: i64 = extract_required(val, path, "a non-negative integer")?;
    if value < 0 {
        return Err(make_spec_err(
            path,
            format!("Invalid '{field}': expected a non-negative integer, got {value}."),
        ));
    }
    Ok(value as u64)
}

/// Builds one projection item (`field`, `agg`, or `expr`) shared by the root
/// `projections` list and `with_clauses[i].projections`.
fn build_projection_item(
    builder: &mut QueryBuilder,
    item: &Bound<'_, PyAny>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<()> {
    let tuple = expect_spec_array(item, path, "projection")?;
    check_spec_arity(&tuple, path, "projection", 1, "[tag, ...]")?;
    let tag_path = path.index(0);
    let tag: String = extract_required(&tuple.get_item(0)?, &tag_path, "a string projection tag")?;
    match tag.as_str() {
        "field" => {
            check_spec_arity_bounded(
                &tuple,
                path,
                "projection 'field'",
                4,
                4,
                "[field, variable, property, alias]",
            )?;
            let var: String =
                extract_required(&tuple.get_item(1)?, &path.index(1), "a string variable")?;
            let prop: String =
                extract_required(&tuple.get_item(2)?, &path.index(2), "a string property")?;
            let alias: Option<String> = extract_required(
                &tuple.get_item(3)?,
                &path.index(3),
                "a string or null alias",
            )?;
            builder.field(var, prop, alias);
        }
        "agg" => {
            check_spec_arity_bounded(
                &tuple,
                path,
                "projection 'agg'",
                5,
                5,
                "[agg, variable, property, func, alias]",
            )?;
            let var: String =
                extract_required(&tuple.get_item(1)?, &path.index(1), "a string variable")?;
            let prop: String =
                extract_required(&tuple.get_item(2)?, &path.index(2), "a string property")?;
            let func_path = path.index(3);
            let func_str: String = extract_required(
                &tuple.get_item(3)?,
                &func_path,
                "a string aggregation function",
            )?;
            let alias: Option<String> = extract_required(
                &tuple.get_item(4)?,
                &path.index(4),
                "a string or null alias",
            )?;
            let agg = match func_str.to_lowercase().as_str() {
                "count" => AggregationFunc::Count,
                "count_distinct" => AggregationFunc::CountDistinct,
                "sum" => AggregationFunc::Sum,
                "avg" => AggregationFunc::Avg,
                "min" => AggregationFunc::Min,
                "max" => AggregationFunc::Max,
                "collect" => AggregationFunc::Collect,
                other => {
                    return Err(make_spec_err(
                        &func_path,
                        format!(
                            "Unknown aggregation function '{other}'. Expected count, count_distinct, sum, avg, min, max, collect."
                        ),
                    ));
                }
            };
            if prop == "*" || prop.is_empty() {
                let expr = builder.ident(var);
                builder.select_aggregate(expr, agg, alias);
            } else {
                builder.select_property_aggregate(var, prop, agg, alias);
            }
        }
        "expr" => {
            check_spec_arity_bounded(
                &tuple,
                path,
                "projection 'expr'",
                3,
                3,
                "[expr, expression, alias]",
            )?;
            let expr_path = path.index(1);
            let expr_spec = tuple.get_item(1)?;
            let mut alias: Option<String> = extract_required(
                &tuple.get_item(2)?,
                &path.index(2),
                "a string or null alias",
            )?;
            if alias.is_none() {
                alias = extract_alias_from_expr_spec(&expr_spec, &expr_path)?;
            }
            let h = py_to_node_handle_depth(builder, &expr_spec, depth, &expr_path)?;
            builder.select_expr(h, alias);
        }
        other => {
            return Err(make_spec_err(
                &tag_path,
                format!("Unknown projection tag '{other}'. Expected 'field', 'agg', or 'expr'."),
            ));
        }
    }
    Ok(())
}

#[allow(clippy::collapsible_if)]
fn build_query_from_spec_internal(
    builder: &mut QueryBuilder,
    spec: &Bound<'_, PyDict>,
    depth: usize,
    path: &SpecPath,
) -> PyResult<()> {
    // Everything below this point is Query Spec v1 parsing, even when a dict
    // expression reached here through the fluent API.
    let spec_path = path.as_spec();
    let path = &spec_path;

    // Spec Versioning & Compatibility Check:
    // Requires top-level "spec_version": 1 in accordance with the Query Spec Interchange Protocol v1.
    let version_path = path.key("spec_version");
    match spec.get_item("spec_version")? {
        Some(ver_item) => {
            let ver: u32 =
                extract_required(&ver_item, &version_path, "an integer major version number")?;
            if ver != 1 {
                return Err(make_spec_err(
                    &version_path,
                    format!("Unsupported query spec_version: {ver}. Supported major versions: 1."),
                ));
            }
        }
        None => {
            return Err(make_spec_err(
                &version_path,
                "Missing required 'spec_version' in query spec dictionary. Expected 'spec_version': 1.",
            ));
        }
    }

    reject_unknown_keys(spec, path, ROOT_SPEC_KEYS, "top-level query spec")?;

    let mut produced_clause = false;
    let mut mutation_count = 0usize;
    let mut with_clause_count = 0usize;
    let mut has_root_projections = false;

    if let Some(load_csv_item) = spec.get_item("load_csv")? {
        if !load_csv_item.is_none() {
            let item_path = path.key("load_csv");
            let tuple = expect_spec_array(&load_csv_item, &item_path, "load_csv")?;
            check_spec_arity_bounded(
                &tuple,
                &item_path,
                "load_csv",
                3,
                3,
                "[url, with_headers, alias]",
            )?;
            let url: String =
                extract_required(&tuple.get_item(0)?, &item_path.index(0), "a string URL")?;
            let with_headers: bool =
                extract_required(&tuple.get_item(1)?, &item_path.index(1), "a boolean")?;
            let alias: String =
                extract_required(&tuple.get_item(2)?, &item_path.index(2), "a string alias")?;
            builder.load_csv(url, with_headers, alias);
            produced_clause = true;
        }
    }

    if let Some(unwinds_item) = spec.get_item("unwinds")? {
        let key_path = path.key("unwinds");
        let unwinds_seq = expect_spec_array(&unwinds_item, &key_path, "unwinds")?;
        for (idx, item) in unwinds_seq.iter().enumerate() {
            let item_path = key_path.index(idx);
            let tuple = expect_spec_array(&item, &item_path, "unwinds entry")?;
            check_spec_arity_bounded(&tuple, &item_path, "unwinds entry", 2, 2, "[param, alias]")?;
            let param_name: String = extract_required(
                &tuple.get_item(0)?,
                &item_path.index(0),
                "a string parameter name",
            )?;
            let param_name = strip_leading_dollar(param_name);
            if param_name.is_empty() {
                return Err(make_spec_err(
                    &item_path.index(0),
                    "Invalid 'unwinds entry': the parameter name must be a non-empty string.",
                ));
            }
            let alias: String =
                extract_required(&tuple.get_item(1)?, &item_path.index(1), "a string alias")?;
            builder.unwind_param(param_name, alias);
            produced_clause = true;
        }
    }

    if let Some(matches_item) = spec.get_item("matches")? {
        let key_path = path.key("matches");
        let matches_seq = expect_spec_array(&matches_item, &key_path, "matches")?;
        for (idx, m) in matches_seq.iter().enumerate() {
            let m_path = key_path.index(idx);
            let m_dict = m.cast::<PyDict>().map_err(|_| {
                make_spec_err(
                    &m_path,
                    format!("expected a match clause object, got {}", json_type_name(&m)),
                )
            })?;
            reject_unknown_keys(m_dict, &m_path, MATCH_SPEC_KEYS, "match clause")?;
            if let Some(m_ver_item) = m_dict.get_item("spec_version")? {
                let version_path = m_path.key("spec_version");
                let m_ver: u32 = extract_required(
                    &m_ver_item,
                    &version_path,
                    "an integer major version number",
                )?;
                if m_ver != 1 {
                    return Err(make_spec_err(
                        &version_path,
                        format!(
                            "Unsupported match clause spec_version: {m_ver}. Supported major versions: 1."
                        ),
                    ));
                }
            }
            let paths_item = m_dict.get_item("paths")?.ok_or_else(|| {
                make_spec_err(
                    &m_path.key("paths"),
                    "missing required 'paths' in match clause.",
                )
            })?;
            if paths_item.is_none() {
                return Err(make_spec_err(
                    &m_path.key("paths"),
                    "missing required 'paths' in match clause.",
                ));
            }
            let paths_path = m_path.key("paths");
            let paths_seq = expect_spec_array(&paths_item, &paths_path, "paths")?;

            let is_optional = if let Some(optional_item) = m_dict.get_item("optional")? {
                if optional_item.is_none() {
                    false
                } else {
                    extract_required(&optional_item, &m_path.key("optional"), "a boolean")?
                }
            } else {
                false
            };
            if is_optional {
                builder.optional_match();
            } else {
                builder.r#match();
            }

            if let Some(mode_item) = m_dict.get_item("path_mode")? {
                if !mode_item.is_none() {
                    let mode_path = m_path.key("path_mode");
                    let m_str: String =
                        extract_required(&mode_item, &mode_path, "a string path mode")?;
                    let m = match m_str.to_ascii_lowercase().as_str() {
                        "trail" => voyager_ogm::ast::PathMode::Trail,
                        "simple" => voyager_ogm::ast::PathMode::Simple,
                        "acyclic" => voyager_ogm::ast::PathMode::Acyclic,
                        "walk" => voyager_ogm::ast::PathMode::Walk,
                        "none" => voyager_ogm::ast::PathMode::None,
                        other => {
                            return Err(make_spec_err(
                                &mode_path,
                                format!(
                                    "unknown path_mode '{other}'. Expected 'trail', 'simple', 'acyclic', 'walk', or 'none'."
                                ),
                            ));
                        }
                    };
                    builder.path_mode(m);
                }
            }
            if let Some(var_item) = m_dict.get_item("path_variable")? {
                if !var_item.is_none() {
                    let var_path = m_path.key("path_variable");
                    let v_str: String =
                        extract_required(&var_item, &var_path, "a string path variable")?;
                    builder.path_variable(v_str);
                }
            }
            if paths_seq.is_empty() {
                return Err(make_spec_err(
                    &paths_path,
                    "a match clause needs at least one path; an empty 'paths' list has no pattern and would leave its predicates unattached",
                ));
            }

            for (p_idx, p_steps) in paths_seq.iter().enumerate() {
                if p_idx > 0 {
                    builder.pattern();
                }
                let p_path = paths_path.index(p_idx);
                let steps = expect_spec_array(&p_steps, &p_path, "path")?;
                if !steps.is_empty() {
                    produced_clause = true;
                }
                build_path_steps_into_builder(builder, &steps, depth, &p_path)?;
            }

            if let Some(where_item) = m_dict.get_item("where")? {
                if !where_item.is_none() {
                    build_where_predicates(builder, &where_item, depth, &m_path.key("where"))?;
                }
            }
        }
    }

    if let Some(mutations_item) = spec.get_item("mutations")? {
        let key_path = path.key("mutations");
        let mutations_seq = expect_spec_array(&mutations_item, &key_path, "mutations")?;
        mutation_count = mutations_seq.len();
        for (idx, mut_item) in mutations_seq.iter().enumerate() {
            let mut_path = key_path.index(idx);
            let tuple = expect_spec_array(&mut_item, &mut_path, "mutation")?;
            check_spec_arity(&tuple, &mut_path, "mutation", 1, "[tag, ...]")?;
            let tag_path = mut_path.index(0);
            let tag: String =
                extract_required(&tuple.get_item(0)?, &tag_path, "a string mutation tag")?;
            match tag.as_str() {
                "create" => {
                    check_spec_arity_bounded(
                        &tuple,
                        &mut_path,
                        "mutation 'create'",
                        2,
                        2,
                        "[create, paths]",
                    )?;
                    builder.create();
                    let paths_path = mut_path.index(1);
                    let paths = expect_spec_array(
                        &tuple.get_item(1)?,
                        &paths_path,
                        "mutation 'create' paths",
                    )?;
                    for (p_idx, p_steps) in paths.iter().enumerate() {
                        if p_idx > 0 {
                            builder.pattern();
                        }
                        let p_path = paths_path.index(p_idx);
                        let steps = expect_spec_array(&p_steps, &p_path, "mutation 'create' path")?;
                        build_path_steps_into_builder(builder, &steps, depth, &p_path)?;
                    }
                }
                "merge" => {
                    check_spec_arity_bounded(
                        &tuple,
                        &mut_path,
                        "mutation 'merge'",
                        4,
                        4,
                        "[merge, path, on_create_set, on_match_set]",
                    )?;
                    builder.merge();
                    let path_path = mut_path.index(1);
                    let path_steps = expect_spec_array(
                        &tuple.get_item(1)?,
                        &path_path,
                        "mutation 'merge' path",
                    )?;
                    build_path_steps_into_builder(builder, &path_steps, depth, &path_path)?;

                    let on_create_path = mut_path.index(2);
                    let on_creates = expect_spec_array(
                        &tuple.get_item(2)?,
                        &on_create_path,
                        "merge 'on_create_set' entries",
                    )?;
                    for (s_idx, item) in on_creates.iter().enumerate() {
                        let item_path = on_create_path.index(s_idx);
                        let s_tuple =
                            expect_spec_array(&item, &item_path, "merge 'on_create_set' entry")?;
                        check_spec_arity_bounded(
                            &s_tuple,
                            &item_path,
                            "merge 'on_create_set' entry",
                            3,
                            3,
                            "[variable, property, value]",
                        )?;
                        let var: String = extract_required(
                            &s_tuple.get_item(0)?,
                            &item_path.index(0),
                            "a string variable",
                        )?;
                        let prop: String = extract_required(
                            &s_tuple.get_item(1)?,
                            &item_path.index(1),
                            "a string property",
                        )?;
                        let val_h = py_to_node_handle_depth(
                            builder,
                            &s_tuple.get_item(2)?,
                            depth,
                            &item_path.index(2),
                        )?;
                        builder.on_create_set_expr(var, prop, val_h);
                    }

                    let on_match_path = mut_path.index(3);
                    let on_matches = expect_spec_array(
                        &tuple.get_item(3)?,
                        &on_match_path,
                        "merge 'on_match_set' entries",
                    )?;
                    for (s_idx, item) in on_matches.iter().enumerate() {
                        let item_path = on_match_path.index(s_idx);
                        let s_tuple =
                            expect_spec_array(&item, &item_path, "merge 'on_match_set' entry")?;
                        check_spec_arity_bounded(
                            &s_tuple,
                            &item_path,
                            "merge 'on_match_set' entry",
                            3,
                            3,
                            "[variable, property, value]",
                        )?;
                        let var: String = extract_required(
                            &s_tuple.get_item(0)?,
                            &item_path.index(0),
                            "a string variable",
                        )?;
                        let prop: String = extract_required(
                            &s_tuple.get_item(1)?,
                            &item_path.index(1),
                            "a string property",
                        )?;
                        let val_h = py_to_node_handle_depth(
                            builder,
                            &s_tuple.get_item(2)?,
                            depth,
                            &item_path.index(2),
                        )?;
                        builder.on_match_set_expr(var, prop, val_h);
                    }
                }
                "set" => {
                    check_spec_arity_bounded(
                        &tuple,
                        &mut_path,
                        "mutation 'set'",
                        4,
                        4,
                        "[set, variable, property, value]",
                    )?;
                    let var: String = extract_required(
                        &tuple.get_item(1)?,
                        &mut_path.index(1),
                        "a string variable",
                    )?;
                    let prop: String = extract_required(
                        &tuple.get_item(2)?,
                        &mut_path.index(2),
                        "a string property",
                    )?;
                    let val_h = py_to_node_handle_depth(
                        builder,
                        &tuple.get_item(3)?,
                        depth,
                        &mut_path.index(3),
                    )?;
                    builder.set_property_expr(var, prop, val_h);
                }
                "delete" => {
                    check_spec_arity_bounded(
                        &tuple,
                        &mut_path,
                        "mutation 'delete'",
                        3,
                        3,
                        "[delete, detach, targets]",
                    )?;
                    let detach: bool =
                        extract_required(&tuple.get_item(1)?, &mut_path.index(1), "a boolean")?;
                    let targets: Vec<String> = extract_required(
                        &tuple.get_item(2)?,
                        &mut_path.index(2),
                        "an array of string variables",
                    )?;
                    if detach {
                        builder.detach_delete(targets);
                    } else {
                        builder.delete(targets);
                    }
                }
                "remove" => {
                    check_spec_arity_bounded(
                        &tuple,
                        &mut_path,
                        "mutation 'remove'",
                        3,
                        3,
                        "[remove, variable, property]",
                    )?;
                    let var: String = extract_required(
                        &tuple.get_item(1)?,
                        &mut_path.index(1),
                        "a string variable",
                    )?;
                    let prop: String = extract_required(
                        &tuple.get_item(2)?,
                        &mut_path.index(2),
                        "a string property",
                    )?;
                    builder.remove_property(var, prop);
                }
                other => {
                    return Err(make_spec_err(
                        &tag_path,
                        format!(
                            "Unknown mutation tag: '{other}'. Expected 'create', 'merge', 'set', 'delete', or 'remove'."
                        ),
                    ));
                }
            }
            produced_clause = true;
        }
    }

    if let Some(with_item) = spec.get_item("with_clauses")? {
        let key_path = path.key("with_clauses");
        let with_seq = expect_spec_array(&with_item, &key_path, "with_clauses")?;
        with_clause_count = with_seq.len();
        for (idx, item) in with_seq.iter().enumerate() {
            let item_path = key_path.index(idx);
            let with_dict = item.cast::<PyDict>().map_err(|_| {
                make_spec_err(
                    &item_path,
                    format!(
                        "expected a with clause object, got {}",
                        json_type_name(&item)
                    ),
                )
            })?;
            reject_unknown_keys(with_dict, &item_path, WITH_SPEC_KEYS, "with clause")?;
            if let Some(w_ver_item) = with_dict.get_item("spec_version")? {
                let version_path = item_path.key("spec_version");
                let w_ver: u32 = extract_required(
                    &w_ver_item,
                    &version_path,
                    "an integer major version number",
                )?;
                if w_ver != 1 {
                    return Err(make_spec_err(
                        &version_path,
                        format!(
                            "Unsupported with clause spec_version: {w_ver}. Supported major versions: 1."
                        ),
                    ));
                }
            }
            let proj_item = with_dict.get_item("projections")?.ok_or_else(|| {
                make_spec_err(
                    &item_path.key("projections"),
                    "missing required 'projections' in with clause.",
                )
            })?;
            if proj_item.is_none() {
                return Err(make_spec_err(
                    &item_path.key("projections"),
                    "missing required 'projections' in with clause.",
                ));
            }
            let proj_path = item_path.key("projections");
            let proj_seq = expect_spec_array(&proj_item, &proj_path, "with_clauses 'projections'")?;
            if proj_seq.is_empty() {
                return Err(make_spec_err(
                    &proj_path,
                    "a with clause needs at least one projection; an empty projection list would drop its where, order_by, skip and limit",
                ));
            }
            builder.r#with();
            produced_clause = true;
            if let Some(distinct_item) = with_dict.get_item("distinct")? {
                if !distinct_item.is_none() {
                    let distinct: bool =
                        extract_required(&distinct_item, &item_path.key("distinct"), "a boolean")?;
                    if distinct {
                        builder.distinct(true);
                    }
                }
            }
            for (p_idx, p) in proj_seq.iter().enumerate() {
                build_projection_item(builder, &p, depth, &proj_path.index(p_idx))?;
            }
            if let Some(where_item) = with_dict.get_item("where")? {
                if !where_item.is_none() {
                    build_where_predicates(builder, &where_item, depth, &item_path.key("where"))?;
                }
            }
            if let Some(order_item) = with_dict.get_item("order_by")? {
                if !order_item.is_none() {
                    let order_path = item_path.key("order_by");
                    let order_seq =
                        expect_spec_array(&order_item, &order_path, "with_clauses 'order_by'")?;
                    for (o_idx, order_entry) in order_seq.iter().enumerate() {
                        build_order_by_entry(
                            builder,
                            &order_entry,
                            depth,
                            &order_path.index(o_idx),
                            "with_clauses 'order_by' entry",
                        )?;
                    }
                }
            }
            if let Some(skip_item) = with_dict.get_item("skip")? {
                if !skip_item.is_none() {
                    let skip = parse_non_negative(&skip_item, &item_path.key("skip"), "skip")?;
                    builder.skip(skip);
                }
            }
            if let Some(limit_item) = with_dict.get_item("limit")? {
                if !limit_item.is_none() {
                    let limit = parse_non_negative(&limit_item, &item_path.key("limit"), "limit")?;
                    builder.limit(limit);
                }
            }
        }
    }

    if let Some(proj_item) = spec.get_item("projections")? {
        if !proj_item.is_none() {
            let proj_path = path.key("projections");
            let proj_seq = expect_spec_array(&proj_item, &proj_path, "projections")?;
            if !proj_seq.is_empty() {
                builder.r#return();
                produced_clause = true;
                has_root_projections = true;
            }
            for (idx, item) in proj_seq.iter().enumerate() {
                build_projection_item(builder, &item, depth, &proj_path.index(idx))?;
            }
        }
    }

    // Root pagination and DISTINCT belong to the final RETURN. Without one they
    // would either be dropped or fold into the last pending WITH, silently
    // changing the query's meaning, so reject them at the first offending key.
    if !has_root_projections {
        for key in ["distinct", "order_by", "skip", "offset", "limit"] {
            if let Some(item) = spec.get_item(key)?
                && !item.is_none()
            {
                return Err(make_spec_err(
                    &path.key(key),
                    format!(
                        "root-level '{key}' requires 'projections'; add a non-empty root projection list (a RETURN clause) or move it into a with clause"
                    ),
                ));
            }
        }
        // A statement that ends in WITH is not a complete query. Mutations emit
        // after WITH, so a mutation-only tail is fine.
        if with_clause_count > 0 && mutation_count == 0 {
            return Err(make_spec_err(
                &path.key("projections"),
                "a query ending in WITH needs root projections; add a non-empty root 'projections' list to close the statement with RETURN",
            ));
        }
    }

    if let Some(distinct_item) = spec.get_item("distinct")? {
        if !distinct_item.is_none() {
            let distinct: bool =
                extract_required(&distinct_item, &path.key("distinct"), "a boolean")?;
            if distinct {
                builder.distinct(true);
            }
        }
    }

    if let Some(order_item) = spec.get_item("order_by")? {
        if !order_item.is_none() {
            let order_path = path.key("order_by");
            let order_seq = expect_spec_array(&order_item, &order_path, "order_by")?;
            for (idx, order_entry) in order_seq.iter().enumerate() {
                build_order_by_entry(
                    builder,
                    &order_entry,
                    depth,
                    &order_path.index(idx),
                    "order_by entry",
                )?;
            }
        }
    }

    if let Some(skip_item) = spec.get_item("skip")? {
        if !skip_item.is_none() {
            let skip = parse_non_negative(&skip_item, &path.key("skip"), "skip")?;
            builder.skip(skip);
        }
    }

    if let Some(offset_item) = spec.get_item("offset")? {
        if !offset_item.is_none() {
            let offset = parse_non_negative(&offset_item, &path.key("offset"), "offset")?;
            builder.offset(offset);
        }
    }

    if let Some(limit_item) = spec.get_item("limit")? {
        if !limit_item.is_none() {
            let limit = parse_non_negative(&limit_item, &path.key("limit"), "limit")?;
            builder.limit(limit);
        }
    }

    if let Some(linear_item) = spec.get_item("linear_clauses")? {
        let key_path = path.key("linear_clauses");
        let linear_seq = expect_spec_array(&linear_item, &key_path, "linear_clauses")?;
        for (idx, item) in linear_seq.iter().enumerate() {
            let item_path = key_path.index(idx);
            let tuple = expect_spec_array(&item, &item_path, "linear_clauses entry")?;
            check_spec_arity(&tuple, &item_path, "linear_clauses entry", 1, "[tag, ...]")?;
            let tag_path = item_path.index(0);
            let tag: String =
                extract_required(&tuple.get_item(0)?, &tag_path, "a string linear clause tag")?;
            match tag.as_str() {
                "let" => {
                    check_spec_arity_bounded(
                        &tuple,
                        &item_path,
                        "linear clause 'let'",
                        3,
                        3,
                        "[let, variable, expression]",
                    )?;
                    let var: String = extract_required(
                        &tuple.get_item(1)?,
                        &item_path.index(1),
                        "a string variable",
                    )?;
                    let expr_h = py_to_node_handle_depth(
                        builder,
                        &tuple.get_item(2)?,
                        depth,
                        &item_path.index(2),
                    )?;
                    builder.let_(var, expr_h);
                }
                "filter" => {
                    check_spec_arity_bounded(
                        &tuple,
                        &item_path,
                        "linear clause 'filter'",
                        2,
                        2,
                        "[filter, predicate]",
                    )?;
                    let pred_h = py_to_node_handle_depth(
                        builder,
                        &tuple.get_item(1)?,
                        depth,
                        &item_path.index(1),
                    )?;
                    builder.filter_(pred_h);
                }
                other => {
                    return Err(make_spec_err(
                        &tag_path,
                        format!(
                            "Unknown linear clause tag: '{other}'. Expected 'let' or 'filter'."
                        ),
                    ));
                }
            }
            produced_clause = true;
        }
    }

    if let Some(mode_item) = spec.get_item("execution_mode")? {
        if !mode_item.is_none() {
            let mode_path = path.key("execution_mode");
            let mode_str: String =
                extract_required(&mode_item, &mode_path, "a string execution mode")?;
            match mode_str.to_ascii_lowercase().as_str() {
                "normal" => {}
                "explain" => {
                    builder.explain();
                }
                "profile" => {
                    builder.profile();
                }
                "explain_and_profile" => {
                    builder.explain();
                    builder.profile();
                }
                other => {
                    return Err(make_spec_err(
                        &mode_path,
                        format!(
                            "unknown execution_mode '{other}'. Expected 'normal', 'explain', 'profile', or 'explain_and_profile'."
                        ),
                    ));
                }
            }
        }
    }

    if !produced_clause {
        return Err(make_spec_err(
            path,
            "empty query spec: expected at least one of 'load_csv', 'unwinds', 'matches', 'mutations', 'with_clauses', 'projections', or 'linear_clauses'.",
        ));
    }

    Ok(())
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
        LiteralValue::Map(m) => {
            let dict = PyDict::new(py);
            for (k, v) in m {
                dict.set_item(k, literal_to_py(v, py)?)?;
            }
            Ok(dict.into_any())
        }
    }
}

fn check_warn_cypher_path_mode(
    py: Python<'_>,
    dialect: &str,
    path_mode: Option<&str>,
) -> PyResult<()> {
    if matches!(
        dialect.trim().to_ascii_lowercase().as_str(),
        "cypher" | "opencypher" | "neo4j" | "memgraph"
    ) && let Some(mode) = path_mode
        && !mode.is_empty()
        && !mode.eq_ignore_ascii_case("none")
    {
        let msg = format!(
            "Dialect '{dialect}' does not support explicit path search modes ('{mode}'). Cypher default traversal semantics (trail) will be used and the search mode keyword was omitted."
        );
        let warnings = py.import("warnings")?;
        let user_warning = py.get_type::<PyUserWarning>();
        warnings.call_method1("warn", (msg, user_warning))?;
    }
    Ok(())
}

fn topology_to_py_dict<'py>(
    topology: &GraphTopology,
    py: Python<'py>,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("version", topology.version)?;
    let nodes_list = PyList::empty(py);
    let edges_list = PyList::empty(py);

    for node in &topology.nodes {
        let node_dict = PyDict::new(py);
        node_dict.set_item("id", &node.id)?;
        node_dict.set_item("label", &node.label)?;
        let labels_list = PyList::empty(py);
        for l in &node.labels {
            labels_list.append(l)?;
        }
        node_dict.set_item("labels", labels_list)?;
        node_dict.set_item("group", &node.group)?;
        node_dict.set_item("size", node.size)?;

        let props_dict = PyDict::new(py);
        for (k, v) in &node.properties {
            props_dict.set_item(k, literal_to_py(v, py)?)?;
        }
        node_dict.set_item("properties", props_dict)?;

        let data_dict = PyDict::new(py);
        for (k, v) in &node.data {
            data_dict.set_item(k, literal_to_py(v, py)?)?;
        }
        node_dict.set_item("data", data_dict)?;

        nodes_list.append(node_dict)?;
    }

    for edge in &topology.edges {
        let edge_dict = PyDict::new(py);
        edge_dict.set_item("id", &edge.id)?;
        edge_dict.set_item("source", &edge.source)?;
        edge_dict.set_item("target", &edge.target)?;
        edge_dict.set_item("label", &edge.label)?;
        let types_list = PyList::empty(py);
        for t in &edge.types {
            types_list.append(t)?;
        }
        edge_dict.set_item("types", types_list)?;
        let dir_str = match edge.direction {
            Direction::Outgoing => "outgoing",
            Direction::Incoming => "incoming",
            Direction::Undirected => "undirected",
        };
        edge_dict.set_item("direction", dir_str)?;
        edge_dict.set_item("color", &edge.color)?;
        edge_dict.set_item("min_hops", edge.min_hops)?;
        edge_dict.set_item("max_hops", edge.max_hops)?;

        let props_dict = PyDict::new(py);
        for (k, v) in &edge.properties {
            props_dict.set_item(k, literal_to_py(v, py)?)?;
        }
        edge_dict.set_item("properties", props_dict)?;

        let data_dict = PyDict::new(py);
        for (k, v) in &edge.data {
            data_dict.set_item(k, literal_to_py(v, py)?)?;
        }
        edge_dict.set_item("data", data_dict)?;

        edges_list.append(edge_dict)?;
    }

    result.set_item("nodes", nodes_list)?;
    result.set_item("edges", edges_list)?;
    Ok(result)
}

/// Extracts graph node and edge topology from a raw query string without external parsers.
#[pyfunction]
fn extract_topology_from_query<'py>(query: &str, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
    let topology = GraphTopology::from_query_str(query);
    topology_to_py_dict(&topology, py)
}

fn conformance_report_to_py_dict<'py>(
    report: &ConformanceReport,
    py: Python<'py>,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("is_valid", report.is_valid)?;

    let diags_list = PyList::empty(py);
    for diag in &report.diagnostics {
        let d = PyDict::new(py);
        d.set_item("severity", diag.severity.as_str())?;
        d.set_item("code", &diag.code)?;
        d.set_item("message", &diag.message)?;
        d.set_item("entity_id", &diag.entity_id)?;
        d.set_item("suggestion", &diag.suggestion)?;
        diags_list.append(d)?;
    }
    result.set_item("diagnostics", diags_list)?;

    let errors_list = PyList::empty(py);
    for err in report.error_messages() {
        errors_list.append(err)?;
    }
    result.set_item("errors", errors_list)?;

    let warnings_list = PyList::empty(py);
    for warn in report.warning_messages() {
        warnings_list.append(warn)?;
    }
    result.set_item("warnings", warnings_list)?;

    Ok(result)
}

/// Validates a raw query string against the schema registry.
#[pyfunction]
#[pyo3(signature = (query, registry=None))]
fn validate_query_against_schema<'py>(
    query: &str,
    registry: Option<&PyNativeSchemaRegistry>,
    py: Python<'py>,
) -> PyResult<Bound<'py, PyDict>> {
    let topology = GraphTopology::from_query_str(query);
    let reg = match registry {
        Some(r) => r.registry(),
        None => global_schema_registry(),
    };
    let report = reg.validate_topology(&topology);
    conformance_report_to_py_dict(&report, py)
}

/// Native Rust Query Builder exposed to Python.
#[pyclass(name = "NativeQueryBuilder", from_py_object)]
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

    fn pattern(&mut self) {
        self.inner.pattern();
    }

    fn has_mutations(&self) -> bool {
        self.inner.has_mutations()
    }

    fn explain(&mut self) {
        self.inner.explain();
    }

    fn profile(&mut self) {
        self.inner.profile();
    }

    fn execution_mode(&self) -> &'static str {
        self.inner.execution_mode().as_str()
    }

    fn clause_mode(&self) -> Option<&'static str> {
        self.inner.clause_mode()
    }

    fn last_merged_variable(&self) -> Option<String> {
        self.inner.last_merged_variable()
    }

    fn is_variable_declared(&self, var: &str) -> bool {
        self.inner.is_variable_declared(var)
    }

    fn find_variables_by_labels(&self, labels: Vec<String>) -> Vec<String> {
        self.inner.find_variables_by_labels(&labels)
    }

    fn clone_builder(&self) -> Self {
        self.clone()
    }

    fn __copy__(&self) -> Self {
        self.clone()
    }

    fn __deepcopy__(&self, _memo: &Bound<'_, PyAny>) -> Self {
        self.clone()
    }

    #[pyo3(signature = (other, seen_vars=None))]
    fn import_match_patterns(
        &mut self,
        other: &Bound<'_, PyQueryBuilder>,
        seen_vars: Option<Vec<String>>,
    ) -> PyResult<Vec<String>> {
        let mut other_ref = other.borrow_mut();
        let mut vars_set: std::collections::HashSet<String> =
            seen_vars.unwrap_or_default().into_iter().collect();
        self.inner
            .import_match_patterns(&mut other_ref.inner, &mut vars_set);
        Ok(vars_set.into_iter().collect())
    }

    #[pyo3(signature = (variable=None, labels=vec![], properties=None))]
    fn node(
        &mut self,
        variable: Option<String>,
        labels: Vec<String>,
        properties: Option<Vec<(String, Bound<'_, PyAny>)>>,
    ) -> PyResult<()> {
        if let Some(props) = properties {
            let mut props_lit = Vec::with_capacity(props.len());
            for (k, v) in props {
                let lit = py_to_literal(&v)?;
                props_lit.push((k, lit));
            }
            self.inner.node_with_props(variable, labels, props_lit);
        } else {
            self.inner.node(variable, labels);
        }
        Ok(())
    }

    #[pyo3(signature = (edge_types=vec![], variable=None, properties=None))]
    fn to(
        &mut self,
        edge_types: Vec<String>,
        variable: Option<String>,
        properties: Option<Vec<(String, Bound<'_, PyAny>)>>,
    ) -> PyResult<()> {
        if let Some(props) = properties {
            let mut props_lit = Vec::with_capacity(props.len());
            for (k, v) in props {
                let lit = py_to_literal(&v)?;
                props_lit.push((k, lit));
            }
            self.inner.to_with_props(edge_types, variable, props_lit);
        } else {
            self.inner.to(edge_types, variable);
        }
        Ok(())
    }

    #[allow(clippy::wrong_self_convention)]
    #[pyo3(signature = (edge_types=vec![], variable=None, properties=None))]
    fn from_edge(
        &mut self,
        edge_types: Vec<String>,
        variable: Option<String>,
        properties: Option<Vec<(String, Bound<'_, PyAny>)>>,
    ) -> PyResult<()> {
        if let Some(props) = properties {
            let mut props_lit = Vec::with_capacity(props.len());
            for (k, v) in props {
                let lit = py_to_literal(&v)?;
                props_lit.push((k, lit));
            }
            self.inner.from_with_props(edge_types, variable, props_lit);
        } else {
            self.inner.from(edge_types, variable);
        }
        Ok(())
    }

    #[pyo3(signature = (edge_types=vec![], variable=None, properties=None))]
    fn edge(
        &mut self,
        edge_types: Vec<String>,
        variable: Option<String>,
        properties: Option<Vec<(String, Bound<'_, PyAny>)>>,
    ) -> PyResult<()> {
        if let Some(props) = properties {
            let mut props_lit = Vec::with_capacity(props.len());
            for (k, v) in props {
                let lit = py_to_literal(&v)?;
                props_lit.push((k, lit));
            }
            self.inner.edge_with_props(edge_types, variable, props_lit);
        } else {
            self.inner.edge(edge_types, variable);
        }
        Ok(())
    }

    fn hops(&mut self, min: u32, max: u32) {
        self.inner.hops(min, max);
    }

    #[pyo3(signature = (variable=None))]
    fn trail(&mut self, variable: Option<String>) {
        self.inner.trail();
        if let Some(v) = variable {
            self.inner.path_variable(v);
        }
    }

    #[pyo3(signature = (variable=None))]
    fn simple(&mut self, variable: Option<String>) {
        self.inner.simple();
        if let Some(v) = variable {
            self.inner.path_variable(v);
        }
    }

    #[pyo3(signature = (variable=None))]
    fn acyclic(&mut self, variable: Option<String>) {
        self.inner.acyclic();
        if let Some(v) = variable {
            self.inner.path_variable(v);
        }
    }

    #[pyo3(signature = (variable=None))]
    fn walk(&mut self, variable: Option<String>) {
        self.inner.walk();
        if let Some(v) = variable {
            self.inner.path_variable(v);
        }
    }

    fn path_variable(&mut self, variable: String) {
        self.inner.path_variable(variable);
    }

    fn path_mode(&mut self, mode: String) -> PyResult<()> {
        let m = match mode.to_ascii_lowercase().as_str() {
            "trail" => voyager_ogm::ast::PathMode::Trail,
            "simple" => voyager_ogm::ast::PathMode::Simple,
            "acyclic" => voyager_ogm::ast::PathMode::Acyclic,
            "walk" => voyager_ogm::ast::PathMode::Walk,
            "none" | "" => voyager_ogm::ast::PathMode::None,
            other => {
                return Err(PyValueError::new_err(format!(
                    "Unknown path mode '{other}'. Expected trail, simple, acyclic, walk, or none."
                )));
            }
        };
        self.inner.path_mode(m);
        Ok(())
    }

    fn get_path_mode(&self) -> Option<String> {
        self.inner.get_path_mode()
    }

    fn where_expr(&mut self, expr: &Bound<'_, PyAny>) -> PyResult<()> {
        let h = py_to_node_handle(&mut self.inner, expr)?;
        self.inner.where_expr(h);
        Ok(())
    }

    #[pyo3(signature = (expr, alias=None))]
    fn select_expr(&mut self, expr: &Bound<'_, PyAny>, alias: Option<String>) -> PyResult<()> {
        let (h, resolved_alias) = if let Ok(tuple) = expr.cast::<PyTuple>() {
            if !tuple.is_empty()
                && tuple
                    .get_item(0)?
                    .extract::<String>()
                    .map(|t| t == "alias")
                    .unwrap_or(false)
            {
                let inner = tuple.get_item(1)?;
                let tuple_alias: Option<String> = tuple.get_item(2)?.extract().ok();
                let inner_h = py_to_node_handle(&mut self.inner, &inner)?;
                (inner_h, alias.or(tuple_alias))
            } else {
                (py_to_node_handle(&mut self.inner, expr)?, alias)
            }
        } else {
            (py_to_node_handle(&mut self.inner, expr)?, alias)
        };
        self.inner.select_expr(h, resolved_alias);
        Ok(())
    }

    #[pyo3(signature = (expr, alias=None))]
    fn custom_expr(&mut self, expr: &Bound<'_, PyAny>, alias: Option<String>) -> PyResult<()> {
        self.select_expr(expr, alias)
    }

    fn where_property(
        &mut self,
        var: String,
        prop: String,
        op: String,
        val: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let op_parsed = parse_binary_op(&op)?;
        let lit = py_to_literal(val)?;
        self.inner.where_property(var, prop, op_parsed, lit);
        Ok(())
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

    fn where_ne(&mut self, var: String, prop: String, val: &Bound<'_, PyAny>) -> PyResult<()> {
        let lit = py_to_literal(val)?;
        self.inner.where_property(var, prop, BinaryOp::Neq, lit);
        Ok(())
    }

    fn where_in(&mut self, var: String, prop: String, val: &Bound<'_, PyAny>) -> PyResult<()> {
        let lit = py_to_literal(val)?;
        self.inner.where_property(var, prop, BinaryOp::In, lit);
        Ok(())
    }

    fn where_not_in(&mut self, var: String, prop: String, val: &Bound<'_, PyAny>) -> PyResult<()> {
        let lit = py_to_literal(val)?;
        self.inner.where_property(var, prop, BinaryOp::NotIn, lit);
        Ok(())
    }

    fn where_contains(&mut self, var: String, prop: String, val: String) {
        self.inner.where_contains(var, prop, val);
    }

    fn where_starts_with(&mut self, var: String, prop: String, val: String) {
        self.inner
            .where_property(var, prop, BinaryOp::StartsWith, LiteralValue::String(val));
    }

    fn where_ends_with(&mut self, var: String, prop: String, val: String) {
        self.inner
            .where_property(var, prop, BinaryOp::EndsWith, LiteralValue::String(val));
    }

    fn r#with(&mut self) {
        self.inner.r#with();
    }

    #[pyo3(name = "with_")]
    fn with_py(&mut self) {
        self.inner.r#with();
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

    #[pyo3(signature = (source, rel_type, target, direction="outgoing", variable=None, properties=None))]
    fn create_relationship(
        &mut self,
        source: String,
        rel_type: String,
        target: String,
        direction: &str,
        variable: Option<String>,
        properties: Option<Vec<(String, Bound<'_, PyAny>)>>,
    ) -> PyResult<()> {
        let direction = parse_direction(direction)?;
        let properties = optional_property_list_to_literals(properties)?;
        self.inner
            .create_relationship(source, rel_type, direction, variable, target, properties)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(())
    }

    #[pyo3(signature = (source, rel_type, target, direction="outgoing", variable=None, properties=None))]
    fn merge_relationship(
        &mut self,
        source: String,
        rel_type: String,
        target: String,
        direction: &str,
        variable: Option<String>,
        properties: Option<Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let direction = parse_direction(direction)?;
        let mut parts = MergeRelationshipProperties::new();
        if let Some(bundle) = properties {
            let bundle = bundle.cast::<PyTuple>().map_err(|_| {
                PyTypeError::new_err(
                    "merge_relationship properties must be a (key, shared, create_only) tuple of property lists",
                )
            })?;
            if bundle.len() != 3 {
                return Err(PyTypeError::new_err(
                    "merge_relationship properties must be a (key, shared, create_only) tuple of property lists",
                ));
            }
            parts.key = property_list_to_literals(&bundle.get_item(0)?)?;
            parts.shared = property_list_to_literals(&bundle.get_item(1)?)?;
            parts.create_only = property_list_to_literals(&bundle.get_item(2)?)?;
        }
        self.inner
            .merge_relationship(source, rel_type, direction, variable, target, parts)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(())
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

    fn on_create_and_match_set(
        &mut self,
        var: String,
        prop: String,
        val: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let lit = py_to_literal(val)?;
        self.inner.on_create_and_match_set(var, prop, lit);
        Ok(())
    }

    fn fold_merge_eq(
        &mut self,
        variable: String,
        property: String,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let lit = py_to_literal(value)?;
        self.inner
            .fold_merge_eq(&variable, &property, lit)
            .map_err(|e| PyValueError::new_err(e.to_string()))
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

    fn unwind(&mut self, param_name: String, alias: String) {
        self.inner.unwind_param(param_name, alias);
    }

    #[pyo3(signature = (url, with_headers=true, alias="row".to_string()))]
    fn load_csv(&mut self, url: String, with_headers: bool, alias: String) {
        self.inner.load_csv(url, with_headers, alias);
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
        if prop == "*" || prop.is_empty() {
            let expr = self.inner.ident(var);
            self.inner.select_aggregate(expr, agg, alias);
        } else {
            self.inner.select_property_aggregate(var, prop, agg, alias);
        }
        Ok(())
    }

    fn order_by(&mut self, var: String, prop: String, ascending: bool) {
        if ascending {
            self.inner.order_by_asc(var, prop);
        } else {
            self.inner.order_by_desc(var, prop);
        }
    }

    fn order_by_expr(&mut self, expr: &Bound<'_, PyAny>, ascending: bool) -> PyResult<()> {
        let h = py_to_node_handle(&mut self.inner, expr)?;
        self.inner.order_by(h, ascending);
        Ok(())
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

    fn offset(&mut self, offset: u64) {
        self.inner.offset(offset);
    }

    fn let_(&mut self, variable: String, expr: &Bound<'_, PyAny>) -> PyResult<()> {
        let h = py_to_node_handle(&mut self.inner, expr)?;
        self.inner.let_(variable, h);
        Ok(())
    }

    fn filter_(&mut self, predicate: &Bound<'_, PyAny>) -> PyResult<()> {
        let h = py_to_node_handle(&mut self.inner, predicate)?;
        self.inner.filter_(h);
        Ok(())
    }

    fn linear_filter(&mut self, predicate: &Bound<'_, PyAny>) -> PyResult<()> {
        self.filter_(predicate)
    }

    #[pyo3(signature = (procedure_name, args=vec![], kwargs=std::collections::HashMap::new()))]
    fn call_procedure(
        &mut self,
        procedure_name: String,
        args: Vec<Bound<'_, PyAny>>,
        kwargs: std::collections::HashMap<String, Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let mut arg_handles = Vec::with_capacity(args.len() + kwargs.len());
        for arg in args {
            let lit = py_to_literal(&arg)?;
            let h = self.inner.literal(lit);
            arg_handles.push(h);
        }
        for (_k, v) in kwargs {
            let lit = py_to_literal(&v)?;
            let h = self.inner.literal(lit);
            arg_handles.push(h);
        }
        self.inner.call_procedure(procedure_name, arg_handles);
        Ok(())
    }

    fn yield_items(&mut self, items: Vec<String>) {
        self.inner.yield_items(items);
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (dialect, is_relationship, target_name, falkordb_target, query_string, properties=vec![], yield_node="node", yield_score="score"))]
    fn fulltext_search(
        &mut self,
        dialect: &str,
        is_relationship: bool,
        target_name: &str,
        falkordb_target: &str,
        query_string: &str,
        properties: Vec<String>,
        yield_node: &str,
        yield_score: &str,
    ) -> PyResult<()> {
        let d = DdlDialect::parse_str(dialect).unwrap_or(DdlDialect::Cypher);
        self.inner.fulltext_search(
            d,
            is_relationship,
            target_name,
            falkordb_target,
            query_string,
            &properties,
            Some(yield_node),
            Some(yield_score),
        );
        Ok(())
    }

    #[pyo3(signature = (dialect="cypher", graph_name=None, optimize=false, optimization_level="standard"))]
    fn compile<'py>(
        &self,
        dialect: &str,
        graph_name: Option<String>,
        optimize: bool,
        optimization_level: &str,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyDict>> {
        check_warn_cypher_path_mode(py, dialect, self.inner.get_path_mode().as_deref())?;

        let (mut arena, root) = self
            .inner
            .clone()
            .try_build()
            .map_err(|e| PyValueError::new_err(e.to_string()))?;

        if optimize {
            let opt_level = OptimizationLevel::from_str_opt(optimization_level);
            let optimizer = AstOptimizer::new(opt_level);
            optimizer
                .optimize(&mut arena, root)
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
        }

        let compiled = match dialect.to_lowercase().as_str() {
            "cypher" | "opencypher" | "neo4j" | "memgraph" | "falkordb" | "falkor" => {
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
            "age" | "apache_age" | "postgres_age" => {
                let name = graph_name
                    .unwrap_or_else(|| voyager_ogm::emitters::age::DEFAULT_AGE_GRAPH.into());
                let mut emitter = AgeEmitter::new(name);
                emitter
                    .visit_query(&arena, root)
                    .map_err(|e| PyValueError::new_err(e.to_string()))?
            }
            other => {
                return Err(make_voyager_err(
                    voyager_ogm::ErrorCode::UnsupportedDialect,
                    format!(
                        "Unsupported query dialect: '{other}'. Choose from 'cypher', 'sql_pgq', 'iso_gql', or 'apache_age'."
                    ),
                ));
            }
        };

        compiled_query_to_py_dict(py, &compiled)
    }

    /// Extracts native graph topology (nodes and edges) directly from the AST arena.
    fn extract_topology<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let (arena, root) = self
            .inner
            .clone()
            .try_build()
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let topology = GraphTopology::from_arena(&arena, Some(root));
        topology_to_py_dict(&topology, py)
    }

    /// Validates the query AST against the schema registry.
    #[pyo3(signature = (registry=None))]
    fn validate<'py>(
        &self,
        registry: Option<&PyNativeSchemaRegistry>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let (arena, root) = self
            .inner
            .clone()
            .try_build()
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let topology = GraphTopology::from_arena(&arena, Some(root));
        let reg = match registry {
            Some(r) => r.registry(),
            None => global_schema_registry(),
        };
        let report = reg.validate_topology(&topology);
        conformance_report_to_py_dict(&report, py)
    }
}

fn record_batch_to_py_dicts<'py>(
    batch: &arrow_array::RecordBatch,
    py: Python<'py>,
) -> PyResult<Bound<'py, PyList>> {
    use arrow_array::*;
    use arrow_schema::DataType;

    let num_rows = batch.num_rows();
    let num_cols = batch.num_columns();
    let schema = batch.schema();
    let fields = schema.fields();
    let list = PyList::empty(py);

    for row in 0..num_rows {
        let dict = PyDict::new(py);
        for col_idx in 0..num_cols {
            let col = batch.column(col_idx);
            let field_name = fields[col_idx].name().as_str();
            if col.is_null(row) {
                dict.set_item(field_name, py.None())?;
                continue;
            }
            match col.data_type() {
                DataType::Int64 => {
                    let arr = col.as_any().downcast_ref::<Int64Array>().unwrap();
                    dict.set_item(field_name, arr.value(row))?;
                }
                DataType::Int32 => {
                    let arr = col.as_any().downcast_ref::<Int32Array>().unwrap();
                    dict.set_item(field_name, arr.value(row))?;
                }
                DataType::Float64 => {
                    let arr = col.as_any().downcast_ref::<Float64Array>().unwrap();
                    dict.set_item(field_name, arr.value(row))?;
                }
                DataType::Float32 => {
                    let arr = col.as_any().downcast_ref::<Float32Array>().unwrap();
                    dict.set_item(field_name, arr.value(row) as f64)?;
                }
                DataType::Boolean => {
                    let arr = col.as_any().downcast_ref::<BooleanArray>().unwrap();
                    dict.set_item(field_name, arr.value(row))?;
                }
                DataType::Utf8 => {
                    let arr = col.as_any().downcast_ref::<StringArray>().unwrap();
                    dict.set_item(field_name, arr.value(row))?;
                }
                DataType::LargeUtf8 => {
                    let arr = col.as_any().downcast_ref::<LargeStringArray>().unwrap();
                    dict.set_item(field_name, arr.value(row))?;
                }
                _ => {
                    dict.set_item(field_name, py.None())?;
                }
            }
        }
        list.append(dict)?;
    }
    Ok(list)
}

/// Zero-copy Arrow Stream implementing the Python Arrow PyCapsule Protocol (`__arrow_c_stream__`).
#[pyclass(name = "ArrowStream", from_py_object)]
#[derive(Clone)]
pub struct PyArrowStream {
    batch: arrow_array::RecordBatch,
    consumed: Arc<AtomicBool>,
}

impl PyArrowStream {
    /// Creates a new `PyArrowStream` wrapping an Arrow `RecordBatch`.
    pub fn from_batch(batch: arrow_array::RecordBatch) -> Self {
        Self {
            batch,
            consumed: Arc::new(AtomicBool::new(false)),
        }
    }
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

    /// Returns whether this stream has already been consumed via `__arrow_c_stream__`.
    #[getter]
    fn is_consumed(&self) -> bool {
        self.consumed.load(Ordering::SeqCst)
    }

    /// Official Python Arrow PyCapsule Protocol for zero-copy streaming into Polars / PyArrow.
    #[pyo3(signature = (requested_schema=None))]
    fn __arrow_c_stream__<'py>(
        &self,
        py: Python<'py>,
        requested_schema: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, pyo3::types::PyCapsule>> {
        let _ = requested_schema;
        if self.consumed.swap(true, Ordering::SeqCst) {
            return Err(PyRuntimeError::new_err(
                "ArrowArrayStream has already been consumed: the Apache Arrow C Data Interface specification enforces single-pass stream consumption.",
            ));
        }
        let ffi_stream = voyager_ogm::arrow::export_batch_to_c_stream(self.batch.clone());
        pyo3::types::PyCapsule::new_with_value(py, ffi_stream, c"arrow_array_stream")
    }

    /// Exports rows as a list of Python dictionaries.
    fn to_dicts<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        record_batch_to_py_dicts(&self.batch, py)
    }

    /// Slices the record batch into chunked batches of `batch_size` rows each, returning PyArrowStreams.
    #[pyo3(signature = (batch_size=10_000))]
    fn iter_batches(&self, batch_size: usize) -> PyResult<Vec<PyArrowStream>> {
        let chunk_size = batch_size.max(1);
        let num_rows = self.batch.num_rows();
        let mut chunks = Vec::new();
        let mut offset = 0;
        while offset < num_rows {
            let length = chunk_size.min(num_rows - offset);
            let sliced = self.batch.slice(offset, length);
            chunks.push(PyArrowStream::from_batch(sliced));
            offset += length;
        }
        Ok(chunks)
    }
}

/// Generates a synthetic Arrow RecordBatch stream for microbenchmarks.
#[pyfunction]
fn generate_synthetic_stream(count: usize) -> PyResult<PyArrowStream> {
    let batch = voyager_ogm::arrow::GraphBatchBuilder::generate_synthetic_nodes(count);
    Ok(PyArrowStream::from_batch(batch))
}

/// Replaces a Rust `Debug` rendering of an AST node in a compile error message
/// with the node kind alone.
///
/// Emitters report failures such as `Unsupported expression node:
/// QueryStatement(..)`; a spec author cannot act on the arena indexes and field
/// values inside that dump, so only the variant name survives.
fn sanitize_compile_message(message: &str) -> String {
    fn node_kind(text: &str) -> Option<&str> {
        let mut end = 0;
        for (idx, ch) in text.char_indices() {
            let allowed = if idx == 0 {
                ch.is_ascii_uppercase()
            } else {
                ch.is_ascii_alphanumeric() || ch == '_'
            };
            if allowed {
                end = idx + ch.len_utf8();
            } else {
                break;
            }
        }
        if end == 0 {
            return None;
        }
        let rest = &text[end..];
        if rest.starts_with('(') || rest.starts_with('{') || rest.starts_with(" {") {
            Some(&text[..end])
        } else {
            None
        }
    }
    for marker in [": ", " got "] {
        for (idx, _) in message.match_indices(marker) {
            let tail = &message[idx + marker.len()..];
            if let Some(kind) = node_kind(tail) {
                return format!("{}{kind} (AST node kind)", &message[..idx + marker.len()]);
            }
        }
    }
    message.to_string()
}

/// Maps a core `try_build` or emitter failure from `compile_query_from_spec`.
///
/// Build and emission failures are spec failures: they surface as
/// `VoyagerError` with a code and a sanitized message. Dialect limitations keep
/// their `UNSUPPORTED_FEATURE` / `UNSUPPORTED_DIALECT` codes; every other
/// failure is `INVALID_SPEC` at the document root.
fn map_compile_error(err: voyager_ogm::Error) -> PyErr {
    let message = sanitize_compile_message(&err.to_string());
    match err.code() {
        voyager_ogm::ErrorCode::UnsupportedFeature | voyager_ogm::ErrorCode::UnsupportedDialect => {
            let py_err = make_voyager_err(err.code(), message);
            Python::attach(|py| {
                let _ = py_err.value(py).setattr("path", "");
            });
            py_err
        }
        _ => make_spec_err(&SpecPath::spec_root(), message),
    }
}

/// Compiles a query spec dictionary directly into a target dialect query with parameters in a single FFI call.
///
/// The spec must be a Query Spec v1 object (`spec_version: 1`). Any malformed
/// structure raises `VoyagerError` with `code == "INVALID_SPEC"`; the JSON
/// Pointer to the offending element is available as `err.path` and also appears
/// in the message (for example `/matches/0/paths/1/2`). In an expression
/// position every array must be a tagged expression: bare or untagged arrays
/// (an empty array, a non-string first element, or an unknown tag) are rejected,
/// so a literal list is written as `["lit", [...]]` and a list of expressions as
/// `["list", [...]]`. A `where` array whose first element is a string is one
/// expression, while an array whose first element is an array or object is a
/// list of predicates. Node and edge path steps accept an optional properties
/// object (the 4th node item and the 7th edge item) whose values are
/// expressions; omitted, `null` and `{}` mean no properties. Error messages use
/// JSON type names (`string`, `number`, `array`, ...) rather than Python type
/// reprs.
#[pyfunction]
#[pyo3(signature = (spec, dialect="cypher", graph_name=None, optimize=false, optimization_level="standard", use_cache=true))]
fn compile_query_from_spec<'py>(
    py: Python<'py>,
    spec: &Bound<'py, PyAny>,
    dialect: &str,
    graph_name: Option<String>,
    optimize: bool,
    optimization_level: &str,
    use_cache: bool,
) -> PyResult<Bound<'py, PyDict>> {
    let spec = spec.cast::<PyDict>().map_err(|_| {
        make_spec_err(
            &SpecPath::spec_root(),
            format!(
                "expected a query spec object (dict), got {}",
                json_type_name(spec)
            ),
        )
    })?;
    let cache_key = if use_cache {
        Some(format!(
            "{}:{}:{}:{}:{}",
            dialect,
            graph_name.as_deref().unwrap_or(""),
            spec.repr()?,
            optimize,
            optimization_level
        ))
    } else {
        None
    };

    if let Some(ref k) = cache_key
        && let Some(cached) = voyager_ogm::global_query_cache().get(k)
    {
        return compiled_query_to_py_dict(py, &cached);
    }

    let mut builder = QueryBuilder::new();
    build_query_from_spec_internal(&mut builder, spec, 0, &SpecPath::spec_root())?;
    check_warn_cypher_path_mode(py, dialect, builder.get_path_mode().as_deref())?;
    let (mut arena, root) = builder.try_build().map_err(map_compile_error)?;

    if optimize {
        let opt_level = OptimizationLevel::from_str_opt(optimization_level);
        let optimizer = AstOptimizer::new(opt_level);
        optimizer
            .optimize(&mut arena, root)
            .map_err(map_compile_error)?;
    }

    let compiled = match dialect.to_lowercase().as_str() {
        "cypher" | "opencypher" | "neo4j" | "memgraph" | "falkordb" | "falkor" => {
            let mut emitter = CypherEmitter::new();
            emitter
                .visit_query(&arena, root)
                .map_err(map_compile_error)?
        }
        "sql_pgq" | "pgq" | "duckpgq" | "sql" => {
            let name = graph_name.unwrap_or_else(|| "graph_table".into());
            let mut emitter = SqlPgqEmitter::new(name);
            emitter
                .visit_query(&arena, root)
                .map_err(map_compile_error)?
        }
        "iso_gql" | "gql" => {
            let mut emitter = IsoGqlEmitter::new();
            emitter
                .visit_query(&arena, root)
                .map_err(map_compile_error)?
        }
        "age" | "apache_age" | "postgres_age" => {
            let name =
                graph_name.unwrap_or_else(|| voyager_ogm::emitters::age::DEFAULT_AGE_GRAPH.into());
            let mut emitter = AgeEmitter::new(name);
            emitter
                .visit_query(&arena, root)
                .map_err(map_compile_error)?
        }
        other => {
            return Err(make_voyager_err(
                voyager_ogm::ErrorCode::UnsupportedDialect,
                format!(
                    "Unsupported query dialect: '{other}'. Choose from 'cypher', 'sql_pgq', 'iso_gql', or 'apache_age'."
                ),
            ));
        }
    };

    if let Some(k) = cache_key {
        voyager_ogm::global_query_cache().put(k, compiled.clone());
    }

    compiled_query_to_py_dict(py, &compiled)
}

/// Returns telemetry metrics for the global compiled query cache.
#[pyfunction]
fn get_query_cache_stats<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
    let m = voyager_ogm::global_query_cache().metrics();
    let dict = PyDict::new(py);
    dict.set_item("hits", m.hits)?;
    dict.set_item("misses", m.misses)?;
    dict.set_item("len", m.len)?;
    dict.set_item("capacity", m.capacity)?;
    dict.set_item("evictions", m.evictions)?;
    dict.set_item("hit_ratio", m.hit_ratio())?;
    Ok(dict)
}

/// Clears all entries from the global compiled query cache.
#[pyfunction]
fn clear_query_cache() {
    voyager_ogm::global_query_cache().clear();
}

/// Returns the native Voyager OGM engine version string.
#[pyfunction]
fn version() -> &'static str {
    voyager_ogm::VERSION
}

/// Native In-Memory Unit of Work for dirty entity state management.
#[pyclass(name = "NativeUnitOfWork")]
pub struct PyUnitOfWork {
    inner: voyager_ogm::transaction::UnitOfWork,
}

#[pymethods]
impl PyUnitOfWork {
    #[new]
    fn new() -> Self {
        Self {
            inner: voyager_ogm::transaction::UnitOfWork::new(),
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

/// Native in-memory Unit of Work scope with savepoints and dirty-state rollback.
#[pyclass(name = "NativeUnitOfWorkScope")]
pub struct PyUnitOfWorkScope {
    inner: voyager_ogm::transaction::Transaction,
    uow: voyager_ogm::transaction::UnitOfWork,
    arena: voyager_ogm::ast::QueryAstArena,
}

#[pymethods]
impl PyUnitOfWorkScope {
    #[new]
    fn new(id: u64) -> Self {
        let uow = voyager_ogm::transaction::UnitOfWork::new();
        let arena = voyager_ogm::ast::QueryAstArena::new();
        let inner = voyager_ogm::transaction::Transaction::new(id, &uow, &arena);
        Self { inner, uow, arena }
    }

    #[getter]
    fn id(&self) -> u64 {
        self.inner.id()
    }

    #[getter]
    fn state(&self) -> String {
        match self.inner.state() {
            voyager_ogm::transaction::TransactionState::Active => "ACTIVE".into(),
            voyager_ogm::transaction::TransactionState::Committed => "COMMITTED".into(),
            voyager_ogm::transaction::TransactionState::RolledBack => "ROLLED_BACK".into(),
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

/// Compiles a high-speed bulk create Cypher/GQL query: `UNWIND $batch AS row CREATE ...`.
#[pyfunction]
#[pyo3(signature = (label, properties, batch_param="batch", row_alias="row", dialect="cypher", graph_name=None))]
fn compile_bulk_create<'py>(
    py: Python<'py>,
    label: String,
    properties: Vec<String>,
    batch_param: &str,
    row_alias: &str,
    dialect: &str,
    graph_name: Option<String>,
) -> PyResult<Bound<'py, PyDict>> {
    let prop_refs: Vec<&str> = properties.iter().map(|s| s.as_str()).collect();
    let compiled = voyager_ogm::bulk::compile_bulk_create_with_graph(
        &label,
        &prop_refs,
        batch_param,
        row_alias,
        dialect,
        graph_name.as_deref(),
    )
    .map_err(|e| PyValueError::new_err(e.to_string()))?;

    let dict = PyDict::new(py);
    dict.set_item("statement", compiled.statement)?;
    let params_dict = PyDict::new(py);
    for (k, v) in compiled.parameters {
        params_dict.set_item(k, literal_to_py(&v, py)?)?;
    }
    dict.set_item("parameters", params_dict)?;
    Ok(dict)
}

/// Compiles a high-speed bulk upsert / MERGE Cypher/GQL query: `UNWIND $batch AS row MERGE ...`.
#[pyfunction]
#[pyo3(signature = (label, key_property, properties, batch_param="batch", row_alias="row", dialect="cypher", graph_name=None))]
fn compile_bulk_merge<'py>(
    label: String,
    key_property: String,
    properties: Bound<'py, PyAny>,
    batch_param: &str,
    row_alias: &str,
    dialect: &str,
    graph_name: Option<String>,
) -> PyResult<Bound<'py, PyDict>> {
    let py = properties.py();
    let prop_strings: Vec<String> = properties.extract()?;
    let prop_refs: Vec<&str> = prop_strings.iter().map(|s| s.as_str()).collect();
    let compiled = voyager_ogm::bulk::compile_bulk_merge_with_graph(
        &label,
        &key_property,
        &prop_refs,
        batch_param,
        row_alias,
        dialect,
        graph_name.as_deref(),
    )
    .map_err(|e| PyValueError::new_err(e.to_string()))?;

    let dict = PyDict::new(py);
    dict.set_item("statement", compiled.statement)?;
    let params_dict = PyDict::new(py);
    for (k, v) in compiled.parameters {
        params_dict.set_item(k, literal_to_py(&v, py)?)?;
    }
    dict.set_item("parameters", params_dict)?;
    Ok(dict)
}

/// Compiles a bulk upsert with properties that are assigned only on creation.
#[pyfunction]
#[pyo3(signature = (label, key_property, properties, create_only_properties, dialect="cypher", graph_name=None))]
fn compile_bulk_merge_create_only<'py>(
    py: Python<'py>,
    label: String,
    key_property: String,
    properties: Vec<String>,
    create_only_properties: Vec<String>,
    dialect: &str,
    graph_name: Option<String>,
) -> PyResult<Bound<'py, PyDict>> {
    let prop_refs: Vec<&str> = properties.iter().map(|s| s.as_str()).collect();
    let create_only_refs: Vec<&str> = create_only_properties.iter().map(|s| s.as_str()).collect();
    let compiled =
        voyager_ogm::bulk::compile_bulk_merge_split(&voyager_ogm::bulk::BulkMergeConfig {
            label: &label,
            key_property: &key_property,
            properties: &prop_refs,
            create_only_properties: &create_only_refs,
            batch_param: "batch",
            row_alias: "row",
            dialect,
            graph_name: graph_name.as_deref(),
        })
        .map_err(|e| PyValueError::new_err(e.to_string()))?;

    let dict = PyDict::new(py);
    dict.set_item("statement", compiled.statement)?;
    let params_dict = PyDict::new(py);
    for (k, v) in compiled.parameters {
        params_dict.set_item(k, literal_to_py(&v, py)?)?;
    }
    dict.set_item("parameters", params_dict)?;
    Ok(dict)
}

/// Compiles a high-speed bulk relationship creation query.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (rel_type, properties, from_label, from_key, to_label, to_key, batch_param="batch", row_alias="row", dialect="cypher", graph_name=None))]
fn compile_bulk_create_rel<'py>(
    py: Python<'py>,
    rel_type: String,
    properties: Vec<String>,
    from_label: String,
    from_key: String,
    to_label: String,
    to_key: String,
    batch_param: &str,
    row_alias: &str,
    dialect: &str,
    graph_name: Option<String>,
) -> PyResult<Bound<'py, PyDict>> {
    let prop_refs: Vec<&str> = properties.iter().map(|s| s.as_str()).collect();
    let config = voyager_ogm::bulk::BulkRelConfig {
        rel_type: &rel_type,
        properties: &prop_refs,
        from_label: &from_label,
        from_key: &from_key,
        to_label: &to_label,
        to_key: &to_key,
        batch_param,
        row_alias,
        dialect,
        graph_name: graph_name.as_deref(),
    };
    let compiled = voyager_ogm::bulk::compile_bulk_create_rel_with_config(&config)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;

    let dict = PyDict::new(py);
    dict.set_item("statement", compiled.statement)?;
    let params_dict = PyDict::new(py);
    for (k, v) in compiled.parameters {
        params_dict.set_item(k, literal_to_py(&v, py)?)?;
    }
    dict.set_item("parameters", params_dict)?;
    Ok(dict)
}

fn py_any_to_json_value(val: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    if val.is_none() {
        Ok(serde_json::Value::Null)
    } else if let Ok(b) = val.cast::<PyBool>() {
        Ok(serde_json::Value::Bool(b.is_true()))
    } else if let Ok(i) = val.cast::<PyInt>() {
        let val_i: i64 = i.extract()?;
        Ok(serde_json::Value::Number(val_i.into()))
    } else if let Ok(f) = val.cast::<PyFloat>() {
        if let Some(n) = serde_json::Number::from_f64(f.value()) {
            Ok(serde_json::Value::Number(n))
        } else {
            Ok(serde_json::Value::Null)
        }
    } else if let Ok(s) = val.cast::<PyString>() {
        Ok(serde_json::Value::String(s.to_string_lossy().into_owned()))
    } else if let Ok(list) = val.cast::<PyList>() {
        let mut items = Vec::with_capacity(list.len());
        for item in list {
            items.push(py_any_to_json_value(&item)?);
        }
        Ok(serde_json::Value::Array(items))
    } else if let Ok(tuple) = val.cast::<PyTuple>() {
        let mut items = Vec::with_capacity(tuple.len());
        for item in tuple {
            items.push(py_any_to_json_value(&item)?);
        }
        Ok(serde_json::Value::Array(items))
    } else if let Ok(dict) = val.cast::<PyDict>() {
        let mut map = serde_json::Map::with_capacity(dict.len());
        for (k, v) in dict {
            let key = if let Ok(s) = k.cast::<PyString>() {
                s.to_string_lossy().into_owned()
            } else {
                k.extract::<String>()?
            };
            map.insert(key, py_any_to_json_value(&v)?);
        }
        Ok(serde_json::Value::Object(map))
    } else if val.hasattr("isoformat")? {
        let iso: String = val.call_method0("isoformat")?.extract()?;
        Ok(serde_json::Value::String(iso))
    } else if val.hasattr("hex")? && val.hasattr("urn")? {
        let s: String = val.str()?.extract()?;
        Ok(serde_json::Value::String(s))
    } else if val.get_type().name()? == "Decimal" {
        if let Ok(f) = val
            .call_method0("__float__")
            .and_then(|v| v.extract::<f64>())
        {
            if let Some(n) = serde_json::Number::from_f64(f) {
                Ok(serde_json::Value::Number(n))
            } else {
                Ok(serde_json::Value::Null)
            }
        } else {
            let s: String = val.str()?.extract()?;
            Ok(serde_json::Value::String(s))
        }
    } else if val.hasattr("to_cypher_dict")? {
        let cypher_dict = val.call_method0("to_cypher_dict")?;
        py_any_to_json_value(&cypher_dict)
    } else {
        let s: String = val.str()?.extract()?;
        Ok(serde_json::Value::String(s))
    }
}

fn py_dict_to_json_params(
    dict_opt: Option<Bound<'_, PyDict>>,
) -> PyResult<HashMap<String, serde_json::Value>> {
    let mut params = HashMap::new();
    if let Some(dict) = dict_opt {
        params.reserve(dict.len());
        for (k, v) in dict {
            let key = if let Ok(s) = k.cast::<PyString>() {
                s.to_string_lossy().into_owned()
            } else {
                k.extract::<String>()?
            };
            let val = py_any_to_json_value(&v)?;
            params.insert(key, val);
        }
    }
    Ok(params)
}

/// Native query result containing columnar Apache Arrow record batch and execution summary counters.
#[pyclass(name = "NativeQueryResult", from_py_object)]
#[derive(Clone)]
pub struct PyNativeQueryResult {
    stream: PyArrowStream,
    columns: Vec<String>,
    summary: voyager_drivers::QuerySummary,
}

#[pymethods]
impl PyNativeQueryResult {
    /// Ordered list of projected return column names.
    #[getter]
    fn columns(&self) -> Vec<String> {
        self.columns.clone()
    }

    /// Number of rows in the result batch.
    #[getter]
    fn num_rows(&self) -> usize {
        self.stream.num_rows()
    }

    /// Number of columns in the result batch.
    #[getter]
    fn num_columns(&self) -> usize {
        self.stream.num_columns()
    }

    /// Returns whether this stream has already been consumed.
    #[getter]
    fn is_consumed(&self) -> bool {
        self.stream.is_consumed()
    }

    /// Number of graph nodes created by the query.
    #[getter]
    fn nodes_created(&self) -> u64 {
        self.summary.nodes_created
    }

    /// Number of graph nodes deleted by the query.
    #[getter]
    fn nodes_deleted(&self) -> u64 {
        self.summary.nodes_deleted
    }

    /// Number of graph relationships created by the query.
    #[getter]
    fn relationships_created(&self) -> u64 {
        self.summary.relationships_created
    }

    /// Number of graph relationships deleted by the query.
    #[getter]
    fn relationships_deleted(&self) -> u64 {
        self.summary.relationships_deleted
    }

    /// Number of properties set or updated on nodes/relationships.
    #[getter]
    fn properties_set(&self) -> u64 {
        self.summary.properties_set
    }

    /// Total server-side execution duration in milliseconds.
    #[getter]
    fn execution_time_ms(&self) -> u64 {
        self.summary.execution_time_ms
    }

    /// Access to the underlying ArrowStream.
    #[getter]
    fn stream(&self) -> PyArrowStream {
        self.stream.clone()
    }

    /// Official Python Arrow PyCapsule Protocol for zero-copy streaming into Polars / PyArrow.
    #[pyo3(signature = (requested_schema=None))]
    fn __arrow_c_stream__<'py>(
        &self,
        py: Python<'py>,
        requested_schema: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, pyo3::types::PyCapsule>> {
        self.stream.__arrow_c_stream__(py, requested_schema)
    }

    /// Exports rows as a list of Python dictionaries.
    fn to_dicts<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        self.stream.to_dicts(py)
    }

    /// Access to result records as a list of Python dictionaries.
    #[getter]
    fn records<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        self.stream.to_dicts(py)
    }

    /// Access to this result's execution summary metrics.
    #[getter]
    fn summary(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        slf
    }
}

fn query_result_to_py(query_result: voyager_drivers::QueryResult) -> PyResult<PyNativeQueryResult> {
    let columns = query_result.columns.clone();
    let summary = query_result.summary.clone();
    let batch = query_result
        .into_single_batch()
        .map_err(|e| PyValueError::new_err(e.to_string()))?
        .unwrap_or_else(|| {
            arrow_array::RecordBatch::new_empty(std::sync::Arc::new(arrow_schema::Schema::empty()))
        });

    Ok(PyNativeQueryResult {
        stream: PyArrowStream::from_batch(batch),
        columns,
        summary,
    })
}

use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

static RUNTIME_PID: AtomicU32 = AtomicU32::new(0);
static ACTIVE_RUNTIME: parking_lot::RwLock<Option<Arc<tokio::runtime::Runtime>>> =
    parking_lot::RwLock::new(None);

/// Returns a shared `Arc<tokio::runtime::Runtime>` guaranteed healthy for the current OS process.
///
/// In multi-process async web environments (`gunicorn -w 4 -k uvicorn.workers.UvicornWorker`),
/// worker processes are spawned via Unix `os.fork()`. Forking copies address space but terminates
/// all background OS worker threads and epoll/IOCP event loop state.
///
/// This accessor compares the stored process PID against `std::process::id()`.
/// If a fork is detected, it cleanly creates a fresh multi-threaded Tokio runtime for the child.
pub fn get_tokio_runtime() -> Arc<tokio::runtime::Runtime> {
    let current_pid = std::process::id();
    let stored_pid = RUNTIME_PID.load(AtomicOrdering::Acquire);

    if stored_pid == current_pid
        && let Some(ref rt) = *ACTIVE_RUNTIME.read()
    {
        return rt.clone();
    }

    let mut guard = ACTIVE_RUNTIME.write();
    let stored_pid = RUNTIME_PID.load(AtomicOrdering::Acquire);
    if stored_pid == current_pid
        && let Some(ref rt) = *guard
    {
        return rt.clone();
    }

    let new_rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to initialize fork-safe Tokio runtime for Voyager native client"),
    );

    *guard = Some(new_rt.clone());
    RUNTIME_PID.store(current_pid, AtomicOrdering::Release);
    new_rt
}

/// Returns the process ID of the active Tokio runtime for fork-safety diagnostics.
#[pyfunction]
fn get_runtime_pid() -> u32 {
    let _ = get_tokio_runtime();
    RUNTIME_PID.load(AtomicOrdering::Acquire)
}

/// Native asynchronous Rust network client managing connection pooling and wire protocols.
#[pyclass(name = "NativeClient")]
pub struct PyNativeClient {
    client: voyager_drivers::NativeClient,
}

#[pymethods]
impl PyNativeClient {
    /// Creates a new `NativeClient` connecting to the given URI with connection pooling.
    #[new]
    #[pyo3(signature = (uri, min_idle=1, max_size=10))]
    fn new(uri: &str, min_idle: usize, max_size: usize) -> PyResult<Self> {
        let pool_config = voyager_drivers::PoolConfig {
            min_idle,
            max_size,
            ..Default::default()
        };
        let client = voyager_drivers::NativeClient::new(uri, Some(pool_config))
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { client })
    }

    /// Returns the target connection URI.
    #[getter]
    fn uri(&self) -> &str {
        self.client.uri()
    }

    /// Returns the active database protocol name.
    #[getter]
    fn protocol(&self) -> String {
        self.client.protocol().to_string()
    }

    /// Synchronously executes a query releasing the Python GIL during network I/O.
    #[pyo3(signature = (query, params=None))]
    fn execute_sync<'py>(
        &self,
        py: Python<'py>,
        query: String,
        params: Option<Bound<'py, PyDict>>,
    ) -> PyResult<PyNativeQueryResult> {
        let params_map = py_dict_to_json_params(params)?;
        let client = self.client.clone();
        let query_result = py
            .detach(|| {
                get_tokio_runtime()
                    .block_on(async move { client.execute(&query, &params_map).await })
            })
            .map_err(|e| PyValueError::new_err(e.to_string()))?;

        let columns = query_result.columns.clone();
        let summary = query_result.summary.clone();
        let batch = query_result
            .into_single_batch()
            .map_err(|e| PyValueError::new_err(e.to_string()))?
            .unwrap_or_else(|| {
                arrow_array::RecordBatch::new_empty(std::sync::Arc::new(
                    arrow_schema::Schema::empty(),
                ))
            });

        Ok(PyNativeQueryResult {
            stream: PyArrowStream::from_batch(batch),
            columns,
            summary,
        })
    }

    /// Asynchronously executes a query returning a Python asyncio Future.
    #[pyo3(signature = (query, params=None))]
    fn execute<'py>(
        &self,
        py: Python<'py>,
        query: String,
        params: Option<Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let params_map = py_dict_to_json_params(params)?;
        let client = self.client.clone();
        let _ = get_tokio_runtime();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let query_result = client
                .execute(&query, &params_map)
                .await
                .map_err(|e| PyValueError::new_err(e.to_string()))?;

            let columns = query_result.columns.clone();
            let summary = query_result.summary.clone();
            let batch = query_result
                .into_single_batch()
                .map_err(|e| PyValueError::new_err(e.to_string()))?
                .unwrap_or_else(|| {
                    arrow_array::RecordBatch::new_empty(std::sync::Arc::new(
                        arrow_schema::Schema::empty(),
                    ))
                });

            Ok(PyNativeQueryResult {
                stream: PyArrowStream::from_batch(batch),
                columns,
                summary,
            })
        })
    }

    /// Synchronously checks database connectivity.
    fn ping_sync(&self, py: Python<'_>) -> PyResult<bool> {
        let client = self.client.clone();
        py.detach(|| get_tokio_runtime().block_on(async move { client.ping().await }))
            .map(|_| true)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Asynchronously checks database connectivity.
    fn ping<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        let _ = get_tokio_runtime();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .ping()
                .await
                .map(|_| true)
                .map_err(|e| PyValueError::new_err(e.to_string()))
        })
    }

    /// Synchronously leases one pooled connection and begins a native transaction.
    fn begin_transaction_sync(&self, py: Python<'_>) -> PyResult<PyNativeTransaction> {
        let client = self.client.clone();
        let tx = py
            .detach(|| {
                get_tokio_runtime().block_on(async move { client.begin_transaction().await })
            })
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(PyNativeTransaction::from_handle(tx))
    }

    /// Asynchronously leases one pooled connection and begins a native transaction.
    fn begin_transaction<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        let _ = get_tokio_runtime();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx = client
                .begin_transaction()
                .await
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            Ok(PyNativeTransaction::from_handle(tx))
        })
    }

    /// Closes the connection pool.
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        let client = self.client.clone();
        py.detach(|| get_tokio_runtime().block_on(async move { client.close().await }));
        Ok(())
    }
}

type NativeTransactionHandle =
    voyager_drivers::NativeTransaction<Box<dyn voyager_drivers::AsyncConnection>>;

const TX_STATE_ACTIVE: u8 = 1;
const TX_STATE_COMMITTED: u8 = 2;
const TX_STATE_ROLLED_BACK: u8 = 3;
const TX_STATE_FAILED: u8 = 4;

/// Marks a PyO3 transaction failed if its future is cancelled mid-await.
///
/// The taken [`NativeTransaction`] is dropped on cancellation, which marks its
/// pooled connection broken (never RESET/reused with in-flight frames). The
/// guard additionally records the FAILED lifecycle state so Python sees an
/// aborted transaction instead of a stale ACTIVE handle with no connection.
struct TxFutureGuard {
    inner: Arc<parking_lot::Mutex<Option<NativeTransactionHandle>>>,
    state: Arc<AtomicU8>,
    armed: bool,
}

impl TxFutureGuard {
    fn new(
        inner: Arc<parking_lot::Mutex<Option<NativeTransactionHandle>>>,
        state: Arc<AtomicU8>,
    ) -> Self {
        Self {
            inner,
            state,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TxFutureGuard {
    fn drop(&mut self) {
        if self.armed {
            self.state.store(TX_STATE_FAILED, Ordering::Release);
            self.inner.lock().take();
        }
    }
}

fn native_transaction_state_code(state: voyager_drivers::NativeTransactionState) -> u8 {
    match state {
        voyager_drivers::NativeTransactionState::Active => TX_STATE_ACTIVE,
        voyager_drivers::NativeTransactionState::Failed => TX_STATE_FAILED,
        voyager_drivers::NativeTransactionState::Committed => TX_STATE_COMMITTED,
        voyager_drivers::NativeTransactionState::RolledBack => TX_STATE_ROLLED_BACK,
    }
}

/// Native explicit database transaction holding one pooled connection.
#[pyclass(name = "NativeTransaction")]
pub struct PyNativeTransaction {
    inner: Arc<parking_lot::Mutex<Option<NativeTransactionHandle>>>,
    state: Arc<AtomicU8>,
    supports_savepoints: bool,
}

impl PyNativeTransaction {
    fn from_handle(tx: NativeTransactionHandle) -> Self {
        let supports_savepoints = tx.supports_savepoints();
        Self {
            inner: Arc::new(parking_lot::Mutex::new(Some(tx))),
            state: Arc::new(AtomicU8::new(TX_STATE_ACTIVE)),
            supports_savepoints,
        }
    }

    fn ensure_active(&self) -> PyResult<()> {
        match self.state.load(Ordering::Acquire) {
            TX_STATE_ACTIVE => Ok(()),
            TX_STATE_FAILED => Err(PyRuntimeError::new_err(
                "transaction is aborted; roll back before executing or committing more statements",
            )),
            _ => Err(PyRuntimeError::new_err(
                "Transaction is not active; it was already committed or rolled back",
            )),
        }
    }

    fn ensure_rollback_allowed(&self) -> PyResult<()> {
        match self.state.load(Ordering::Acquire) {
            TX_STATE_ACTIVE | TX_STATE_FAILED => Ok(()),
            _ => Err(PyRuntimeError::new_err(
                "Transaction is not active; it was already committed or rolled back",
            )),
        }
    }

    fn sync_state_from_handle(&self, tx: &NativeTransactionHandle) {
        self.state
            .store(native_transaction_state_code(tx.state()), Ordering::Release);
    }

    fn take(&self) -> PyResult<NativeTransactionHandle> {
        self.inner.lock().take().ok_or_else(|| {
            PyRuntimeError::new_err("Transaction is not active; no pooled connection is leased")
        })
    }

    fn put(&self, tx: NativeTransactionHandle) {
        *self.inner.lock() = Some(tx);
    }

    fn ensure_savepoints(&self) -> PyResult<()> {
        if self.supports_savepoints {
            Ok(())
        } else {
            Err(PyRuntimeError::new_err(
                "Savepoints are not supported by this backend (Bolt/Neo4j/Memgraph has no savepoints)",
            ))
        }
    }
}

#[pymethods]
impl PyNativeTransaction {
    /// Returns the lifecycle state ('ACTIVE', 'COMMITTED', or 'ROLLED_BACK').
    #[getter]
    fn state(&self) -> &'static str {
        match self.state.load(Ordering::Acquire) {
            TX_STATE_COMMITTED => "COMMITTED",
            TX_STATE_ROLLED_BACK => "ROLLED_BACK",
            TX_STATE_FAILED => "FAILED",
            _ => "ACTIVE",
        }
    }

    /// Returns `true` while the transaction accepts statements.
    #[getter]
    fn is_active(&self) -> bool {
        self.state.load(Ordering::Acquire) == TX_STATE_ACTIVE
    }

    /// Returns `true` when this transaction supports real savepoints.
    #[getter]
    fn supports_savepoints(&self) -> bool {
        self.supports_savepoints
    }

    /// Synchronously executes a statement inside the transaction.
    #[pyo3(signature = (query, params=None))]
    fn execute_sync(
        &self,
        py: Python<'_>,
        query: String,
        params: Option<Bound<'_, PyDict>>,
    ) -> PyResult<PyNativeQueryResult> {
        self.ensure_active()?;
        let params_map = py_dict_to_json_params(params)?;
        let mut tx = self.take()?;
        let (tx, result) = py.detach(|| {
            let result =
                get_tokio_runtime().block_on(async { tx.execute(&query, &params_map).await });
            (tx, result)
        });
        self.sync_state_from_handle(&tx);
        self.put(tx);
        query_result_to_py(result.map_err(|e| PyValueError::new_err(e.to_string()))?)
    }

    /// Asynchronously executes a statement inside the transaction.
    #[pyo3(signature = (query, params=None))]
    fn execute<'py>(
        &self,
        py: Python<'py>,
        query: String,
        params: Option<Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.ensure_active()?;
        let params_map = py_dict_to_json_params(params)?;
        let inner = self.inner.clone();
        let state = self.state.clone();
        let _ = get_tokio_runtime();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut guard = TxFutureGuard::new(inner.clone(), state.clone());
            let mut tx = match inner.lock().take() {
                Some(tx) => tx,
                None => {
                    guard.disarm();
                    return Err(PyRuntimeError::new_err("Transaction is not active"));
                }
            };
            let result = tx.execute(&query, &params_map).await;
            let tx_state = tx.state();
            *inner.lock() = Some(tx);
            state.store(native_transaction_state_code(tx_state), Ordering::Release);
            guard.disarm();
            query_result_to_py(result.map_err(|e| PyValueError::new_err(e.to_string()))?)
        })
    }

    /// Synchronously commits the transaction.
    fn commit_sync(&self, py: Python<'_>) -> PyResult<()> {
        self.ensure_active()?;
        let mut tx = self.take()?;
        let (tx, result) = py.detach(|| {
            let result = get_tokio_runtime().block_on(async { tx.commit().await });
            (tx, result)
        });
        match result {
            Ok(_) => {
                self.state.store(TX_STATE_COMMITTED, Ordering::Release);
                Ok(())
            }
            Err(err) => {
                self.sync_state_from_handle(&tx);
                self.put(tx);
                Err(PyValueError::new_err(err.to_string()))
            }
        }
    }

    /// Asynchronously commits the transaction.
    fn commit<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.ensure_active()?;
        let inner = self.inner.clone();
        let state = self.state.clone();
        let _ = get_tokio_runtime();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut guard = TxFutureGuard::new(inner.clone(), state.clone());
            let mut tx = match inner.lock().take() {
                Some(tx) => tx,
                None => {
                    guard.disarm();
                    return Err(PyRuntimeError::new_err("Transaction is not active"));
                }
            };
            match tx.commit().await {
                Ok(_) => {
                    state.store(TX_STATE_COMMITTED, Ordering::Release);
                    guard.disarm();
                    Ok(())
                }
                Err(err) => {
                    let tx_state = tx.state();
                    *inner.lock() = Some(tx);
                    state.store(native_transaction_state_code(tx_state), Ordering::Release);
                    guard.disarm();
                    Err(PyValueError::new_err(err.to_string()))
                }
            }
        })
    }

    /// Synchronously rolls the transaction back.
    fn rollback_sync(&self, py: Python<'_>) -> PyResult<()> {
        self.ensure_rollback_allowed()?;
        let mut tx = self.take()?;
        let (tx, result) = py.detach(|| {
            let result = get_tokio_runtime().block_on(async { tx.rollback().await });
            (tx, result)
        });
        match result {
            Ok(()) => {
                self.state.store(TX_STATE_ROLLED_BACK, Ordering::Release);
                Ok(())
            }
            Err(err) => {
                self.sync_state_from_handle(&tx);
                self.put(tx);
                Err(PyValueError::new_err(err.to_string()))
            }
        }
    }

    /// Asynchronously rolls the transaction back.
    fn rollback<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.ensure_rollback_allowed()?;
        let inner = self.inner.clone();
        let state = self.state.clone();
        let _ = get_tokio_runtime();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut guard = TxFutureGuard::new(inner.clone(), state.clone());
            let mut tx = match inner.lock().take() {
                Some(tx) => tx,
                None => {
                    guard.disarm();
                    return Err(PyRuntimeError::new_err("Transaction is not active"));
                }
            };
            match tx.rollback().await {
                Ok(()) => {
                    state.store(TX_STATE_ROLLED_BACK, Ordering::Release);
                    guard.disarm();
                    Ok(())
                }
                Err(err) => {
                    let tx_state = tx.state();
                    *inner.lock() = Some(tx);
                    state.store(native_transaction_state_code(tx_state), Ordering::Release);
                    guard.disarm();
                    Err(PyValueError::new_err(err.to_string()))
                }
            }
        })
    }

    /// Synchronously creates a real database savepoint.
    fn savepoint_sync(&self, py: Python<'_>, name: String) -> PyResult<()> {
        self.ensure_active()?;
        self.ensure_savepoints()?;
        let mut tx = self.take()?;
        let (tx, result) = py.detach(|| {
            let result = get_tokio_runtime().block_on(async { tx.savepoint(&name).await });
            (tx, result)
        });
        self.sync_state_from_handle(&tx);
        self.put(tx);
        result.map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Asynchronously creates a real database savepoint.
    fn savepoint<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        self.ensure_active()?;
        self.ensure_savepoints()?;
        let inner = self.inner.clone();
        let state = self.state.clone();
        let _ = get_tokio_runtime();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut guard = TxFutureGuard::new(inner.clone(), state.clone());
            let mut tx = match inner.lock().take() {
                Some(tx) => tx,
                None => {
                    guard.disarm();
                    return Err(PyRuntimeError::new_err("Transaction is not active"));
                }
            };
            let result = tx.savepoint(&name).await;
            let tx_state = tx.state();
            *inner.lock() = Some(tx);
            state.store(native_transaction_state_code(tx_state), Ordering::Release);
            guard.disarm();
            result.map_err(|e| PyValueError::new_err(e.to_string()))
        })
    }

    /// Synchronously rolls the transaction back to a savepoint.
    fn rollback_to_savepoint_sync(&self, py: Python<'_>, name: String) -> PyResult<()> {
        self.ensure_active()?;
        self.ensure_savepoints()?;
        let mut tx = self.take()?;
        let (tx, result) = py.detach(|| {
            let result =
                get_tokio_runtime().block_on(async { tx.rollback_to_savepoint(&name).await });
            (tx, result)
        });
        self.sync_state_from_handle(&tx);
        self.put(tx);
        result.map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Asynchronously rolls the transaction back to a savepoint.
    fn rollback_to_savepoint<'py>(
        &self,
        py: Python<'py>,
        name: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.ensure_active()?;
        self.ensure_savepoints()?;
        let inner = self.inner.clone();
        let state = self.state.clone();
        let _ = get_tokio_runtime();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut guard = TxFutureGuard::new(inner.clone(), state.clone());
            let mut tx = match inner.lock().take() {
                Some(tx) => tx,
                None => {
                    guard.disarm();
                    return Err(PyRuntimeError::new_err("Transaction is not active"));
                }
            };
            let result = tx.rollback_to_savepoint(&name).await;
            let tx_state = tx.state();
            *inner.lock() = Some(tx);
            state.store(native_transaction_state_code(tx_state), Ordering::Release);
            guard.disarm();
            result.map_err(|e| PyValueError::new_err(e.to_string()))
        })
    }

    /// Synchronously releases a database savepoint.
    fn release_savepoint_sync(&self, py: Python<'_>, name: String) -> PyResult<()> {
        self.ensure_active()?;
        self.ensure_savepoints()?;
        let mut tx = self.take()?;
        let (tx, result) = py.detach(|| {
            let result = get_tokio_runtime().block_on(async { tx.release_savepoint(&name).await });
            (tx, result)
        });
        self.sync_state_from_handle(&tx);
        self.put(tx);
        result.map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Asynchronously releases a database savepoint.
    fn release_savepoint<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        self.ensure_active()?;
        self.ensure_savepoints()?;
        let inner = self.inner.clone();
        let state = self.state.clone();
        let _ = get_tokio_runtime();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut guard = TxFutureGuard::new(inner.clone(), state.clone());
            let mut tx = match inner.lock().take() {
                Some(tx) => tx,
                None => {
                    guard.disarm();
                    return Err(PyRuntimeError::new_err("Transaction is not active"));
                }
            };
            let result = tx.release_savepoint(&name).await;
            let tx_state = tx.state();
            *inner.lock() = Some(tx);
            state.store(native_transaction_state_code(tx_state), Ordering::Release);
            guard.disarm();
            result.map_err(|e| PyValueError::new_err(e.to_string()))
        })
    }
}

/// Native thread-safe circuit breaker and backend router for Voyager sessions.
#[pyclass(name = "NativeCircuitRouter")]
pub struct PyNativeCircuitRouter {
    router: voyager_drivers::CircuitRouter,
}

#[pymethods]
impl PyNativeCircuitRouter {
    /// Creates a new `NativeCircuitRouter` with configurable failure threshold, cooldown, and success threshold.
    #[new]
    #[pyo3(signature = (failure_threshold=1, cooldown_seconds=30.0, success_threshold=1))]
    fn new(failure_threshold: u32, cooldown_seconds: f64, success_threshold: u32) -> Self {
        let config = voyager_drivers::CircuitConfig::new(
            failure_threshold,
            std::time::Duration::from_secs_f64(cooldown_seconds.max(0.0)),
        )
        .with_success_threshold(success_threshold);
        Self {
            router: voyager_drivers::CircuitRouter::new(config),
        }
    }

    /// Returns the current state of the circuit breaker ("closed", "open", "half_open").
    #[getter]
    fn state(&self) -> &'static str {
        self.router.state().as_str()
    }

    /// Determines the routing destination for an incoming query ("native", "fallback", "probe").
    fn route(&self) -> &'static str {
        self.router.route().as_str()
    }

    /// Returns `true` if the cooldown has elapsed and a trial probe should be executed.
    fn should_probe(&self) -> bool {
        self.router.should_probe()
    }

    /// Returns `true` if the query should be routed to the native engine or probed.
    fn should_route_native(&self) -> bool {
        self.router.should_route_native()
    }

    /// Records a query failure, tripping the circuit if transient and threshold reached.
    #[pyo3(signature = (is_transient=true))]
    fn record_failure(&self, is_transient: bool) {
        self.router.record_failure(is_transient);
    }

    /// Records a successful query execution, resetting failures or closing a half-open circuit.
    fn record_success(&self) {
        self.router.record_success();
    }

    /// Classifies an error message and records it, returning `true` if counted as transient.
    fn record_error(&self, error_msg: &str) -> bool {
        self.router.record_error(error_msg)
    }

    /// Manually trips the circuit breaker to `Open`.
    fn trip(&self) {
        self.router.trip();
    }

    /// Manually resets the circuit breaker to `Closed`.
    fn reset(&self) {
        self.router.reset();
    }

    /// Returns the current count of consecutive transient failures.
    #[getter]
    fn consecutive_failures(&self) -> u32 {
        self.router.consecutive_failures()
    }

    /// Returns the current count of consecutive successful probes.
    #[getter]
    fn consecutive_successes(&self) -> u32 {
        self.router.consecutive_successes()
    }

    /// Returns the configured failure threshold.
    #[getter]
    fn failure_threshold(&self) -> u32 {
        self.router.failure_threshold()
    }

    /// Returns the configured cooldown duration in seconds.
    #[getter]
    fn cooldown_seconds(&self) -> f64 {
        self.router.cooldown_seconds()
    }

    /// Returns the remaining cooldown duration in seconds if currently Open, or `None`.
    fn remaining_cooldown(&self) -> Option<f64> {
        self.router.remaining_cooldown().map(|d| d.as_secs_f64())
    }

    /// Returns an immutable telemetry snapshot dictionary of the circuit breaker metrics.
    fn snapshot<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let snap = self.router.snapshot();
        let dict = PyDict::new(py);
        dict.set_item("state", snap.state.as_str())?;
        dict.set_item("consecutive_failures", snap.consecutive_failures)?;
        dict.set_item("consecutive_successes", snap.consecutive_successes)?;
        dict.set_item("failure_threshold", snap.failure_threshold)?;
        dict.set_item("cooldown_seconds", snap.cooldown_seconds)?;
        dict.set_item(
            "remaining_cooldown_seconds",
            snap.remaining_cooldown_seconds,
        )?;
        Ok(dict)
    }

    /// Checks if an error string represents a query syntax, semantic, or constraint error.
    #[staticmethod]
    fn is_semantic_error(error_msg: &str) -> bool {
        voyager_drivers::is_semantic_error(error_msg)
    }

    /// Classifies an error string as "semantic" or "transient".
    #[staticmethod]
    fn classify_error(error_msg: &str) -> &'static str {
        voyager_drivers::classify_error(error_msg).as_str()
    }

    fn __repr__(&self) -> String {
        format!(
            "NativeCircuitRouter(state='{}', failures={}, threshold={}, cooldown={:.2}s)",
            self.router.state().as_str(),
            self.router.consecutive_failures(),
            self.router.failure_threshold(),
            self.router.cooldown_seconds(),
        )
    }
}

fn parse_field_descriptor(name: &str, obj: &Bound<'_, PyAny>) -> PyResult<FieldDescriptor> {
    if let Ok(dict) = obj.cast::<PyDict>() {
        let field_name = if let Some(n) = dict.get_item("name")? {
            if n.is_none() {
                name.to_string()
            } else {
                let s: String = n.extract()?;
                if s.is_empty() { name.to_string() } else { s }
            }
        } else {
            name.to_string()
        };

        if field_name.is_empty() {
            return Err(PyValueError::new_err(
                "Field descriptor missing property name (key or 'name' attribute)",
            ));
        }

        let primary_key = dict
            .get_item("primary_key")?
            .and_then(|v| v.extract::<bool>().ok())
            .unwrap_or(false);

        let unique = dict
            .get_item("unique")?
            .and_then(|v| v.extract::<bool>().ok())
            .unwrap_or(primary_key);

        let nullable = dict
            .get_item("nullable")?
            .and_then(|v| v.extract::<bool>().ok())
            .unwrap_or(!primary_key);

        let mut indexed = dict
            .get_item("indexed")?
            .or(dict.get_item("index")?)
            .and_then(|v| v.extract::<bool>().ok())
            .unwrap_or(false);

        let mut index_type = if let Some(it) = dict.get_item("index_type")? {
            if it.is_none() {
                if indexed {
                    Some(IndexType::BTree)
                } else {
                    None
                }
            } else {
                let it_str: String = it.extract()?;
                indexed = true;
                IndexType::parse_str(&it_str)
            }
        } else if indexed {
            Some(IndexType::BTree)
        } else {
            None
        };

        let field_type = if let Some(t) = dict.get_item("type")?.or(dict.get_item("field_type")?) {
            if let Ok(s) = t.extract::<String>() {
                FieldType::parse_str(&s)
            } else if t.hasattr("__name__")? {
                let s: String = t.getattr("__name__")?.extract()?;
                FieldType::parse_str(&s)
            } else {
                FieldType::Any
            }
        } else {
            FieldType::Any
        };

        let default_value = if let Some(dv) = dict
            .get_item("default_value")?
            .or(dict.get_item("default")?)
        {
            if dv.is_none() {
                None
            } else {
                py_to_literal(&dv).ok()
            }
        } else {
            None
        };

        let mut vector_config = None;
        if index_type == Some(IndexType::Vector)
            || dict.contains("dimensions")?
            || dict.contains("vector_config")?
        {
            let dims: usize = if let Some(d) = dict.get_item("dimensions")? {
                if d.is_none() {
                    DEFAULT_VECTOR_DIMENSIONS
                } else {
                    match d.extract::<usize>() {
                        Ok(v) if v > 0 => v,
                        Ok(_) => {
                            return Err(PyValueError::new_err(
                                "Vector dimensions must be a positive integer greater than 0",
                            ));
                        }
                        Err(e) => {
                            return Err(PyValueError::new_err(format!(
                                "Invalid vector dimensions: expected positive integer, got: {e}"
                            )));
                        }
                    }
                }
            } else {
                DEFAULT_VECTOR_DIMENSIONS
            };
            let sim_str: String = if let Some(s) = dict.get_item("similarity")? {
                s.extract().unwrap_or_else(|_| "cosine".to_string())
            } else {
                "cosine".to_string()
            };
            let similarity =
                VectorSimilarity::parse_str(&sim_str).unwrap_or(VectorSimilarity::Cosine);
            let idx_name: Option<String> = if let Some(n) = dict.get_item("index_name")? {
                if n.is_none() { None } else { n.extract().ok() }
            } else {
                None
            };
            indexed = true;
            index_type = Some(IndexType::Vector);
            vector_config = Some(VectorIndexConfig {
                dimensions: dims,
                similarity,
                index_name: idx_name,
            });
        }

        let description = if let Some(d) = dict.get_item("description")? {
            if d.is_none() {
                None
            } else {
                d.extract::<String>().ok()
            }
        } else {
            None
        };

        Ok(FieldDescriptor {
            name: field_name,
            field_type,
            nullable,
            primary_key,
            unique,
            indexed,
            index_type,
            default_value,
            vector_config,
            description,
        })
    } else {
        let field_name = if obj.hasattr("name")? {
            let n = obj.getattr("name")?;
            if n.is_none() {
                name.to_string()
            } else {
                let s: String = n.extract()?;
                if s.is_empty() { name.to_string() } else { s }
            }
        } else {
            name.to_string()
        };

        if field_name.is_empty() {
            return Err(PyValueError::new_err(
                "Field descriptor missing property name (key or 'name' attribute)",
            ));
        }

        let primary_key = if obj.hasattr("primary_key")? {
            obj.getattr("primary_key")?
                .extract::<bool>()
                .unwrap_or(false)
        } else {
            false
        };

        let unique = if obj.hasattr("unique")? {
            obj.getattr("unique")?.extract::<bool>().unwrap_or(false) || primary_key
        } else {
            primary_key
        };

        let mut indexed = if obj.hasattr("index")? {
            obj.getattr("index")?.extract::<bool>().unwrap_or(false)
        } else if obj.hasattr("indexed")? {
            obj.getattr("indexed")?.extract::<bool>().unwrap_or(false)
        } else {
            false
        };

        let nullable = if obj.hasattr("nullable")? {
            obj.getattr("nullable")?
                .extract::<bool>()
                .unwrap_or(!primary_key)
        } else {
            !primary_key
        };

        let mut index_type = if obj.hasattr("index_type")? {
            let it = obj.getattr("index_type")?;
            if it.is_none() {
                if indexed {
                    Some(IndexType::BTree)
                } else {
                    None
                }
            } else {
                let it_str: String = it.extract()?;
                indexed = true;
                IndexType::parse_str(&it_str)
            }
        } else if indexed {
            Some(IndexType::BTree)
        } else {
            None
        };

        let field_type = if obj.hasattr("type_annotation")? {
            let ann = obj.getattr("type_annotation")?;
            if ann.is_none() {
                FieldType::Any
            } else if let Ok(s) = ann.extract::<String>() {
                FieldType::parse_str(&s)
            } else if ann.hasattr("__name__")? {
                let s: String = ann.getattr("__name__")?.extract()?;
                FieldType::parse_str(&s)
            } else {
                FieldType::Any
            }
        } else if obj.hasattr("type")? {
            let t = obj.getattr("type")?;
            if let Ok(s) = t.extract::<String>() {
                FieldType::parse_str(&s)
            } else {
                FieldType::Any
            }
        } else {
            FieldType::Any
        };

        let default_value = if obj.hasattr("default")? {
            let dv = obj.getattr("default")?;
            if dv.is_none() {
                None
            } else {
                py_to_literal(&dv).ok()
            }
        } else {
            None
        };

        let mut vector_config = None;
        if index_type == Some(IndexType::Vector)
            || obj.hasattr("dimensions")?
            || obj.hasattr("vector_config")?
        {
            let dims: usize = if obj.hasattr("dimensions")? {
                let d = obj.getattr("dimensions")?;
                if d.is_none() {
                    DEFAULT_VECTOR_DIMENSIONS
                } else {
                    match d.extract::<usize>() {
                        Ok(v) if v > 0 => v,
                        Ok(_) => {
                            return Err(PyValueError::new_err(
                                "Vector dimensions must be a positive integer greater than 0",
                            ));
                        }
                        Err(e) => {
                            return Err(PyValueError::new_err(format!(
                                "Invalid vector dimensions: expected positive integer, got: {e}"
                            )));
                        }
                    }
                }
            } else {
                DEFAULT_VECTOR_DIMENSIONS
            };
            let sim_str: String = if obj.hasattr("similarity")? {
                let s = obj.getattr("similarity")?;
                if s.is_none() {
                    "cosine".to_string()
                } else {
                    s.extract::<String>()
                        .unwrap_or_else(|_| "cosine".to_string())
                }
            } else {
                "cosine".to_string()
            };
            let similarity =
                VectorSimilarity::parse_str(&sim_str).unwrap_or(VectorSimilarity::Cosine);
            let idx_name: Option<String> = if obj.hasattr("index_name")? {
                let n = obj.getattr("index_name")?;
                if n.is_none() { None } else { n.extract().ok() }
            } else {
                None
            };
            indexed = true;
            index_type = Some(IndexType::Vector);
            vector_config = Some(VectorIndexConfig {
                dimensions: dims,
                similarity,
                index_name: idx_name,
            });
        }

        let description = if obj.hasattr("description")? {
            let d = obj.getattr("description")?;
            if d.is_none() {
                None
            } else {
                d.extract::<String>().ok()
            }
        } else {
            None
        };

        Ok(FieldDescriptor {
            name: field_name,
            field_type,
            nullable,
            primary_key,
            unique,
            indexed,
            index_type,
            default_value,
            vector_config,
            description,
        })
    }
}

fn field_descriptor_to_py<'py>(
    py: Python<'py>,
    field: &FieldDescriptor,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("name", &field.name)?;
    dict.set_item("type", field.field_type.as_str())?;
    dict.set_item("nullable", field.nullable)?;
    dict.set_item("primary_key", field.primary_key)?;
    dict.set_item("unique", field.unique)?;
    dict.set_item("indexed", field.indexed)?;
    if let Some(it) = field.index_type {
        dict.set_item("index_type", it.as_str())?;
    } else {
        dict.set_item("index_type", py.None())?;
    }
    if let Some(ref dv) = field.default_value {
        dict.set_item("default_value", literal_to_py(dv, py)?)?;
    } else {
        dict.set_item("default_value", py.None())?;
    }
    if let Some(ref vc) = field.vector_config {
        dict.set_item("dimensions", vc.dimensions)?;
        dict.set_item("similarity", vc.similarity.as_str())?;
        if let Some(ref iname) = vc.index_name {
            dict.set_item("index_name", iname)?;
        } else {
            dict.set_item("index_name", py.None())?;
        }
    }
    if let Some(ref d) = field.description {
        dict.set_item("description", d)?;
    } else {
        dict.set_item("description", py.None())?;
    }
    Ok(dict)
}

fn node_schema_to_py<'py>(py: Python<'py>, node: &NodeSchema) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("name", &node.name)?;
    dict.set_item("labels", &node.labels)?;
    if let Some(ref d) = node.description {
        dict.set_item("description", d)?;
    } else {
        dict.set_item("description", py.None())?;
    }
    if let Some(ref pk) = node.primary_key {
        dict.set_item("primary_key", pk)?;
    } else {
        dict.set_item("primary_key", py.None())?;
    }
    let fields_dict = PyDict::new(py);
    for (k, v) in &node.fields {
        fields_dict.set_item(k, field_descriptor_to_py(py, v)?)?;
    }
    dict.set_item("fields", fields_dict)?;
    let ft_list = PyList::empty(py);
    for ft in &node.fulltext_indexes {
        let ft_dict = PyDict::new(py);
        ft_dict.set_item("name", &ft.name)?;
        ft_dict.set_item("properties", &ft.properties)?;
        if let Some(ref a) = ft.analyzer {
            ft_dict.set_item("analyzer", a)?;
        } else {
            ft_dict.set_item("analyzer", py.None())?;
        }
        ft_list.append(ft_dict)?;
    }
    dict.set_item("fulltext_indexes", ft_list)?;
    Ok(dict)
}

fn relationship_schema_to_py<'py>(
    py: Python<'py>,
    rel: &RelationshipSchema,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("name", &rel.name)?;
    dict.set_item("type_name", &rel.type_name)?;
    if let Some(ref d) = rel.description {
        dict.set_item("description", d)?;
    } else {
        dict.set_item("description", py.None())?;
    }
    dict.set_item("source_labels", &rel.source_labels)?;
    dict.set_item("target_labels", &rel.target_labels)?;
    dict.set_item("directed", rel.directed)?;
    let fields_dict = PyDict::new(py);
    for (k, v) in &rel.fields {
        fields_dict.set_item(k, field_descriptor_to_py(py, v)?)?;
    }
    dict.set_item("fields", fields_dict)?;
    let ft_list = PyList::empty(py);
    for ft in &rel.fulltext_indexes {
        let ft_dict = PyDict::new(py);
        ft_dict.set_item("name", &ft.name)?;
        ft_dict.set_item("properties", &ft.properties)?;
        if let Some(ref a) = ft.analyzer {
            ft_dict.set_item("analyzer", a)?;
        } else {
            ft_dict.set_item("analyzer", py.None())?;
        }
        ft_list.append(ft_dict)?;
    }
    dict.set_item("fulltext_indexes", ft_list)?;
    Ok(dict)
}

fn parse_fulltext_index_config(val: &Bound<'_, PyAny>) -> PyResult<FullTextIndexConfig> {
    if let Ok(dict) = val.cast::<PyDict>() {
        let name: String = dict
            .get_item("name")?
            .and_then(|v| if v.is_none() { None } else { v.extract().ok() })
            .unwrap_or_default();
        let properties: Vec<String> = if let Some(p) = dict.get_item("properties")? {
            if let Ok(vec) = p.extract::<Vec<String>>() {
                vec
            } else if let Ok(s) = p.extract::<String>() {
                vec![s]
            } else {
                vec![]
            }
        } else {
            vec![]
        };
        let analyzer: Option<String> = dict
            .get_item("analyzer")?
            .and_then(|v| if v.is_none() { None } else { v.extract().ok() });
        let mut cfg = FullTextIndexConfig::new(name, properties);
        if let Some(a) = analyzer {
            cfg = cfg.with_analyzer(a);
        }
        Ok(cfg)
    } else {
        let name: String = if val.hasattr("name")? {
            val.getattr("name")?
                .extract::<Option<String>>()?
                .unwrap_or_default()
        } else {
            String::new()
        };
        let properties: Vec<String> = if val.hasattr("properties")? {
            let p = val.getattr("properties")?;
            if let Ok(vec) = p.extract::<Vec<String>>() {
                vec
            } else if let Ok(s) = p.extract::<String>() {
                vec![s]
            } else {
                vec![]
            }
        } else {
            vec![]
        };
        let analyzer: Option<String> = if val.hasattr("analyzer")? {
            val.getattr("analyzer")?.extract::<Option<String>>()?
        } else {
            None
        };
        let mut cfg = FullTextIndexConfig::new(name, properties);
        if let Some(a) = analyzer {
            cfg = cfg.with_analyzer(a);
        }
        Ok(cfg)
    }
}

fn extract_fulltext_configs(obj: &Bound<'_, PyAny>) -> PyResult<Vec<FullTextIndexConfig>> {
    let mut out = Vec::new();
    if let Ok(list) = obj.cast::<PyList>() {
        for item in list {
            out.push(parse_fulltext_index_config(&item)?);
        }
    } else if let Ok(tuple) = obj.cast::<PyTuple>() {
        for item in tuple {
            out.push(parse_fulltext_index_config(&item)?);
        }
    }
    Ok(out)
}

fn extract_node_schema(obj: &Bound<'_, PyAny>) -> PyResult<NodeSchema> {
    if let Ok(dict) = obj.cast::<PyDict>() {
        let name: String = dict
            .get_item("name")?
            .ok_or_else(|| PyValueError::new_err("Missing required key 'name' in node schema"))?
            .extract()?;
        let labels: Vec<String> = if let Some(l) = dict.get_item("labels")? {
            l.extract()?
        } else {
            vec![name.clone()]
        };
        let pk: Option<String> = dict
            .get_item("primary_key")?
            .and_then(|v| if v.is_none() { None } else { v.extract().ok() });
        let mut node = NodeSchema::new(name, labels);
        if let Some(f_obj) = dict.get_item("fields")? {
            if let Ok(f_dict) = f_obj.cast::<PyDict>() {
                for (k, v) in f_dict {
                    let field_name: String = if let Ok(s) = k.cast::<PyString>() {
                        s.to_string_lossy().into_owned()
                    } else {
                        k.extract()?
                    };
                    let desc = parse_field_descriptor(&field_name, &v)?;
                    node = node.with_field(desc);
                }
            } else if let Ok(f_list) = f_obj.cast::<PyList>() {
                for item in f_list {
                    let desc = parse_field_descriptor("", &item)?;
                    node = node.with_field(desc);
                }
            }
        }
        if let Some(pk_name) = pk {
            if let Some(f) = node.fields.get_mut(&pk_name) {
                f.primary_key = true;
                f.unique = true;
                f.nullable = false;
            }
            node.primary_key = Some(pk_name);
        }
        if let Some(ft_obj) = dict.get_item("fulltext_indexes")? {
            for cfg in extract_fulltext_configs(&ft_obj)? {
                node = node.with_fulltext_index(cfg);
            }
        }
        Ok(node)
    } else {
        let name: String = if obj.hasattr("__name__")? {
            obj.getattr("__name__")?.extract()?
        } else {
            return Err(PyTypeError::new_err(
                "Expected node class or schema dict with '__name__'",
            ));
        };
        let labels: Vec<String> = if obj.hasattr("__labels__")? {
            let l = obj.getattr("__labels__")?;
            if let Ok(vec) = l.extract::<Vec<String>>() {
                if vec.is_empty() {
                    vec![name.clone()]
                } else {
                    vec
                }
            } else {
                vec![name.clone()]
            }
        } else {
            vec![name.clone()]
        };
        let mut node = NodeSchema::new(name, labels);
        if obj.hasattr("_schema_fields")? {
            let fields_any = obj.getattr("_schema_fields")?;
            if let Ok(fields_dict) = fields_any.cast::<PyDict>() {
                for (k, v) in fields_dict {
                    let field_name: String = if let Ok(s) = k.cast::<PyString>() {
                        s.to_string_lossy().into_owned()
                    } else {
                        k.extract()?
                    };
                    let desc = parse_field_descriptor(&field_name, &v)?;
                    node = node.with_field(desc);
                }
            }
        }
        if obj.hasattr("__fulltext_indexes__")? {
            let ft_obj = obj.getattr("__fulltext_indexes__")?;
            for cfg in extract_fulltext_configs(&ft_obj)? {
                node = node.with_fulltext_index(cfg);
            }
        }
        Ok(node)
    }
}

fn extract_rel_schema(obj: &Bound<'_, PyAny>) -> PyResult<RelationshipSchema> {
    if let Ok(dict) = obj.cast::<PyDict>() {
        let name: String = dict
            .get_item("name")?
            .ok_or_else(|| {
                PyValueError::new_err("Missing required key 'name' in relationship schema")
            })?
            .extract()?;
        let type_name: String = if let Some(t) = dict
            .get_item("type_name")?
            .or(dict.get_item("type_")?)
            .or(dict.get_item("type")?)
        {
            t.extract()?
        } else {
            name.to_ascii_uppercase()
        };
        let source_labels: Vec<String> = dict
            .get_item("source_labels")?
            .or(dict.get_item("from_labels")?)
            .and_then(|v| if v.is_none() { None } else { v.extract().ok() })
            .unwrap_or_default();
        let target_labels: Vec<String> = dict
            .get_item("target_labels")?
            .or(dict.get_item("to_labels")?)
            .and_then(|v| if v.is_none() { None } else { v.extract().ok() })
            .unwrap_or_default();
        let directed: bool = dict
            .get_item("directed")?
            .and_then(|v| v.extract::<bool>().ok())
            .unwrap_or(true);
        let mut rel = RelationshipSchema::new(name, type_name)
            .with_endpoints(source_labels, target_labels)
            .directed(directed);
        if let Some(f_obj) = dict.get_item("fields")? {
            if let Ok(f_dict) = f_obj.cast::<PyDict>() {
                for (k, v) in f_dict {
                    let field_name: String = if let Ok(s) = k.cast::<PyString>() {
                        s.to_string_lossy().into_owned()
                    } else {
                        k.extract()?
                    };
                    let desc = parse_field_descriptor(&field_name, &v)?;
                    rel = rel.with_field(desc);
                }
            } else if let Ok(f_list) = f_obj.cast::<PyList>() {
                for item in f_list {
                    let desc = parse_field_descriptor("", &item)?;
                    rel = rel.with_field(desc);
                }
            }
        }
        if let Some(ft_obj) = dict.get_item("fulltext_indexes")? {
            for cfg in extract_fulltext_configs(&ft_obj)? {
                rel = rel.with_fulltext_index(cfg);
            }
        }
        Ok(rel)
    } else {
        let name: String = if obj.hasattr("__name__")? {
            obj.getattr("__name__")?.extract()?
        } else {
            return Err(PyTypeError::new_err(
                "Expected relationship class or schema dict with '__name__'",
            ));
        };
        let type_name: String = if obj.hasattr("__type__")? {
            let t: String = obj.getattr("__type__")?.extract()?;
            if t.is_empty() {
                name.to_ascii_uppercase()
            } else {
                t
            }
        } else {
            name.to_ascii_uppercase()
        };
        let source_labels: Vec<String> = if obj.hasattr("__source_labels__")? {
            obj.getattr("__source_labels__")?
                .extract::<Vec<String>>()
                .unwrap_or_default()
        } else {
            vec![]
        };
        let target_labels: Vec<String> = if obj.hasattr("__target_labels__")? {
            obj.getattr("__target_labels__")?
                .extract::<Vec<String>>()
                .unwrap_or_default()
        } else {
            vec![]
        };
        let mut rel =
            RelationshipSchema::new(name, type_name).with_endpoints(source_labels, target_labels);
        if obj.hasattr("_schema_fields")? {
            let fields_any = obj.getattr("_schema_fields")?;
            if let Ok(fields_dict) = fields_any.cast::<PyDict>() {
                for (k, v) in fields_dict {
                    let field_name: String = if let Ok(s) = k.cast::<PyString>() {
                        s.to_string_lossy().into_owned()
                    } else {
                        k.extract()?
                    };
                    let desc = parse_field_descriptor(&field_name, &v)?;
                    rel = rel.with_field(desc);
                }
            }
        }
        if obj.hasattr("__fulltext_indexes__")? {
            let ft_obj = obj.getattr("__fulltext_indexes__")?;
            for cfg in extract_fulltext_configs(&ft_obj)? {
                rel = rel.with_fulltext_index(cfg);
            }
        }
        Ok(rel)
    }
}

enum RegistryStorage {
    Owned(Arc<SchemaRegistry>),
    Global,
}

/// Centralized, thread-safe Native Schema Registry and metadata storage.
#[pyclass(name = "NativeSchemaRegistry")]
pub struct PyNativeSchemaRegistry {
    storage: RegistryStorage,
}

impl PyNativeSchemaRegistry {
    fn registry(&self) -> &SchemaRegistry {
        match &self.storage {
            RegistryStorage::Owned(r) => r.as_ref(),
            RegistryStorage::Global => global_schema_registry(),
        }
    }

    fn build_node_schema(
        name: String,
        labels: Vec<String>,
        fields: &Bound<'_, PyAny>,
        primary_key: Option<String>,
        fulltext_indexes: Option<&Bound<'_, PyAny>>,
        description: Option<String>,
    ) -> PyResult<NodeSchema> {
        let mut node_schema = NodeSchema::new(name, labels);
        if let Some(desc) = description {
            node_schema = node_schema.with_description(desc);
        }
        if let Ok(dict) = fields.cast::<PyDict>() {
            for (k, v) in dict {
                let field_name: String = if let Ok(s) = k.cast::<PyString>() {
                    s.to_string_lossy().into_owned()
                } else {
                    k.extract()?
                };
                let desc = parse_field_descriptor(&field_name, &v)?;
                node_schema = node_schema.with_field(desc);
            }
        } else if let Ok(list) = fields.cast::<PyList>() {
            for item in list {
                let desc = parse_field_descriptor("", &item)?;
                node_schema = node_schema.with_field(desc);
            }
        } else {
            return Err(PyTypeError::new_err("fields must be a dict or list"));
        }

        if let Some(pk) = primary_key {
            if let Some(f) = node_schema.fields.get_mut(&pk) {
                f.primary_key = true;
                f.unique = true;
                f.nullable = false;
            }
            node_schema.primary_key = Some(pk);
        }

        if let Some(ft_any) = fulltext_indexes {
            for cfg in extract_fulltext_configs(ft_any)? {
                node_schema = node_schema.with_fulltext_index(cfg);
            }
        }

        Ok(node_schema)
    }

    fn build_relationship_schema(
        name: String,
        type_name: String,
        endpoints: (Option<Vec<String>>, Option<Vec<String>>),
        fields: Option<&Bound<'_, PyAny>>,
        directed: bool,
        fulltext_indexes: Option<&Bound<'_, PyAny>>,
        description: Option<String>,
    ) -> PyResult<RelationshipSchema> {
        let mut rel_schema = RelationshipSchema::new(name, type_name).directed(directed);
        if let Some(desc) = description {
            rel_schema = rel_schema.with_description(desc);
        }
        if let (Some(src), Some(tgt)) = endpoints {
            rel_schema = rel_schema.with_endpoints(src, tgt);
        }
        if let Some(fields_any) = fields {
            if let Ok(dict) = fields_any.cast::<PyDict>() {
                for (k, v) in dict {
                    let field_name: String = if let Ok(s) = k.cast::<PyString>() {
                        s.to_string_lossy().into_owned()
                    } else {
                        k.extract()?
                    };
                    let desc = parse_field_descriptor(&field_name, &v)?;
                    rel_schema = rel_schema.with_field(desc);
                }
            } else if let Ok(list) = fields_any.cast::<PyList>() {
                for item in list {
                    let desc = parse_field_descriptor("", &item)?;
                    rel_schema = rel_schema.with_field(desc);
                }
            }
        }

        if let Some(ft_any) = fulltext_indexes {
            for cfg in extract_fulltext_configs(ft_any)? {
                rel_schema = rel_schema.with_fulltext_index(cfg);
            }
        }

        Ok(rel_schema)
    }
}

#[pymethods]
impl PyNativeSchemaRegistry {
    #[new]
    fn new() -> Self {
        Self {
            storage: RegistryStorage::Owned(Arc::new(SchemaRegistry::new())),
        }
    }

    /// Accesses the global singleton schema registry.
    #[staticmethod]
    fn global_registry() -> Self {
        Self {
            storage: RegistryStorage::Global,
        }
    }

    /// Alias for `global_registry()`.
    #[staticmethod]
    fn global_() -> Self {
        Self::global_registry()
    }

    /// Registers a node schema with labels, fields, and optional primary key.
    #[pyo3(signature = (name, labels, fields, primary_key=None, fulltext_indexes=None, description=None))]
    fn register_node(
        &self,
        name: String,
        labels: Vec<String>,
        fields: &Bound<'_, PyAny>,
        primary_key: Option<String>,
        fulltext_indexes: Option<&Bound<'_, PyAny>>,
        description: Option<String>,
    ) -> PyResult<()> {
        let node_schema = Self::build_node_schema(
            name,
            labels,
            fields,
            primary_key,
            fulltext_indexes,
            description,
        )?;
        self.registry()
            .register_node(node_schema)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(())
    }

    /// Registers a node schema under a unique model identity.
    ///
    /// The primary key is derived from the field descriptors.
    #[pyo3(signature = (model_id, name, labels, fields, fulltext_indexes=None, description=None))]
    fn register_node_with_identity(
        &self,
        model_id: String,
        name: String,
        labels: Vec<String>,
        fields: &Bound<'_, PyAny>,
        fulltext_indexes: Option<&Bound<'_, PyAny>>,
        description: Option<String>,
    ) -> PyResult<()> {
        let node_schema =
            Self::build_node_schema(name, labels, fields, None, fulltext_indexes, description)?;
        self.registry()
            .register_node_with_id(model_id, node_schema)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(())
    }

    /// Registers a node schema from a dictionary specification.
    fn register_node_schema(&self, schema: &Bound<'_, PyAny>) -> PyResult<()> {
        if let Ok(dict) = schema.cast::<PyDict>() {
            let name: String = dict
                .get_item("name")?
                .ok_or_else(|| PyValueError::new_err("Missing required key 'name' in node schema"))?
                .extract()?;
            let labels: Vec<String> = if let Some(l) = dict.get_item("labels")? {
                l.extract()?
            } else {
                vec![name.clone()]
            };
            let pk: Option<String> = dict
                .get_item("primary_key")?
                .and_then(|v| if v.is_none() { None } else { v.extract().ok() });
            let empty_dict = PyDict::new(schema.py());
            let fields_obj = if let Some(f) = dict.get_item("fields")? {
                f
            } else {
                empty_dict.into_any()
            };
            let ft_obj = dict.get_item("fulltext_indexes")?;
            let description: Option<String> = dict
                .get_item("description")?
                .and_then(|v| if v.is_none() { None } else { v.extract().ok() });
            self.register_node(name, labels, &fields_obj, pk, ft_obj.as_ref(), description)
        } else {
            Err(PyTypeError::new_err("schema must be a dict"))
        }
    }

    /// Registers a relationship schema with endpoints, fields, and direction.
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (name, type_name, source_labels=None, target_labels=None, fields=None, directed=true, fulltext_indexes=None, model_id=None, description=None))]
    fn register_relationship(
        &self,
        name: String,
        type_name: String,
        source_labels: Option<Vec<String>>,
        target_labels: Option<Vec<String>>,
        fields: Option<&Bound<'_, PyAny>>,
        directed: bool,
        fulltext_indexes: Option<&Bound<'_, PyAny>>,
        model_id: Option<String>,
        description: Option<String>,
    ) -> PyResult<()> {
        let rel_schema = Self::build_relationship_schema(
            name,
            type_name,
            (source_labels, target_labels),
            fields,
            directed,
            fulltext_indexes,
            description,
        )?;
        let registry = self.registry();
        if let Some(model_id) = model_id {
            registry.register_relationship_with_id(model_id, rel_schema)
        } else {
            registry.register_relationship(rel_schema)
        }
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(())
    }

    /// Registers a relationship schema from a dictionary specification.
    fn register_relationship_schema(&self, schema: &Bound<'_, PyAny>) -> PyResult<()> {
        if let Ok(dict) = schema.cast::<PyDict>() {
            let name: String = dict
                .get_item("name")?
                .ok_or_else(|| {
                    PyValueError::new_err("Missing required key 'name' in relationship schema")
                })?
                .extract()?;
            let type_name: String = if let Some(t) = dict
                .get_item("type_name")?
                .or(dict.get_item("type_")?)
                .or(dict.get_item("type")?)
            {
                t.extract()?
            } else {
                name.to_ascii_uppercase()
            };
            let source_labels: Option<Vec<String>> = dict
                .get_item("source_labels")?
                .or(dict.get_item("from_labels")?)
                .and_then(|v| if v.is_none() { None } else { v.extract().ok() });
            let target_labels: Option<Vec<String>> = dict
                .get_item("target_labels")?
                .or(dict.get_item("to_labels")?)
                .and_then(|v| if v.is_none() { None } else { v.extract().ok() });
            let directed: bool = dict
                .get_item("directed")?
                .and_then(|v| v.extract::<bool>().ok())
                .unwrap_or(true);
            let fields_obj = dict.get_item("fields")?;
            let ft_obj = dict.get_item("fulltext_indexes")?;
            let model_id: Option<String> = dict
                .get_item("model_id")?
                .and_then(|v| if v.is_none() { None } else { v.extract().ok() });
            let description: Option<String> = dict
                .get_item("description")?
                .and_then(|v| if v.is_none() { None } else { v.extract().ok() });
            self.register_relationship(
                name,
                type_name,
                source_labels,
                target_labels,
                fields_obj.as_ref(),
                directed,
                ft_obj.as_ref(),
                model_id,
                description,
            )
        } else {
            Err(PyTypeError::new_err("schema must be a dict"))
        }
    }

    /// Retrieves a node schema by model identity or bare class name.
    ///
    /// When a bare class name is shared by several registrations, the most
    /// recently registered schema is returned.
    fn get_node<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Option<Bound<'py, PyDict>>> {
        if let Some(node) = self.registry().get_node(name) {
            Ok(Some(node_schema_to_py(py, &node)?))
        } else {
            Ok(None)
        }
    }

    /// Returns the primary key property name for a node or relationship model.
    ///
    /// The value is derived by the native schema from the field descriptors.
    fn primary_key_for(&self, model_id: &str) -> Option<String> {
        self.registry().primary_key_for(model_id)
    }

    /// Resolves the MERGE key property names for a node or relationship model.
    ///
    /// `supplied` lists the property names present on the instance and
    /// `defaulted` lists the subset that only carries a declared default.
    fn merge_key_for(
        &self,
        model_id: &str,
        supplied: Vec<String>,
        defaulted: Vec<String>,
    ) -> Option<Vec<String>> {
        self.registry()
            .merge_key_for(model_id, &supplied, &defaulted)
    }

    /// Retrieves a node schema by primary label.
    fn get_node_by_label<'py>(
        &self,
        py: Python<'py>,
        label: &str,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        if let Some(node) = self.registry().get_node_by_label(label) {
            Ok(Some(node_schema_to_py(py, &node)?))
        } else {
            Ok(None)
        }
    }

    /// Retrieves a relationship schema by model name.
    fn get_relationship<'py>(
        &self,
        py: Python<'py>,
        name: &str,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        if let Some(rel) = self.registry().get_relationship(name) {
            Ok(Some(relationship_schema_to_py(py, &rel)?))
        } else {
            Ok(None)
        }
    }

    /// Retrieves a relationship schema by graph relationship type (case-insensitive).
    fn get_relationship_by_type<'py>(
        &self,
        py: Python<'py>,
        type_name: &str,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        if let Some(rel) = self.registry().get_relationship_by_type(type_name) {
            Ok(Some(relationship_schema_to_py(py, &rel)?))
        } else {
            Ok(None)
        }
    }

    /// Checks if a node schema is registered by name.
    fn has_node(&self, name: &str) -> bool {
        self.registry().has_node(name)
    }

    /// Checks if a relationship schema is registered by name.
    fn has_relationship(&self, name: &str) -> bool {
        self.registry().has_relationship(name)
    }

    /// Removes a node schema by model name, returning it if present.
    fn remove_node<'py>(
        &self,
        py: Python<'py>,
        name: &str,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        if let Some(node) = self.registry().remove_node(name) {
            Ok(Some(node_schema_to_py(py, &node)?))
        } else {
            Ok(None)
        }
    }

    /// Removes a relationship schema by model name, returning it if present.
    fn remove_relationship<'py>(
        &self,
        py: Python<'py>,
        name: &str,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        if let Some(rel) = self.registry().remove_relationship(name) {
            Ok(Some(relationship_schema_to_py(py, &rel)?))
        } else {
            Ok(None)
        }
    }

    /// Returns a list of all registered node schemas as dictionaries.
    fn node_schemas<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let list = PyList::empty(py);
        for n in self.registry().node_schemas() {
            list.append(node_schema_to_py(py, &n)?)?;
        }
        Ok(list)
    }

    /// Returns a list of all registered relationship schemas as dictionaries.
    fn relationship_schemas<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let list = PyList::empty(py);
        for r in self.registry().relationship_schemas() {
            list.append(relationship_schema_to_py(py, &r)?)?;
        }
        Ok(list)
    }

    /// Clears all schemas from the registry.
    fn clear(&self) {
        self.registry().clear();
    }

    /// Serializes all registered schemas to a JSON string.
    fn to_json(&self) -> PyResult<String> {
        self.registry()
            .to_json()
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Deserializes and restores schemas from a JSON string into this registry.
    #[allow(clippy::wrong_self_convention)]
    fn from_json(&self, json_str: &str) -> PyResult<()> {
        self.registry()
            .from_json(json_str)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Deserializes and restores schemas from a JSON string into this registry.
    fn load_json(&self, json_str: &str) -> PyResult<()> {
        self.from_json(json_str)
    }

    /// Returns the total count of registered schemas.
    fn __len__(&self) -> usize {
        self.registry().len()
    }

    /// Generates openCypher constraint and index creation statements.
    #[pyo3(signature = (include_type_constraints=false, model_name=None))]
    fn generate_cypher_ddl(
        &self,
        include_type_constraints: bool,
        model_name: Option<&str>,
    ) -> PyResult<Vec<String>> {
        if let Some(name) = model_name {
            if let Some(stmts) = self
                .registry()
                .generate_node_cypher_ddl(name, include_type_constraints)
            {
                return Ok(stmts);
            }
            if let Some(stmts) = self
                .registry()
                .generate_rel_cypher_ddl(name, include_type_constraints)
            {
                return Ok(stmts);
            }
            Err(PyValueError::new_err(format!(
                "No node or relationship registered with name '{name}'"
            )))
        } else {
            Ok(self
                .registry()
                .generate_cypher_ddl(include_type_constraints))
        }
    }

    /// Generates openCypher DROP statements.
    #[pyo3(signature = (include_type_constraints=false, model_name=None))]
    fn generate_cypher_drop_ddl(
        &self,
        include_type_constraints: bool,
        model_name: Option<&str>,
    ) -> PyResult<Vec<String>> {
        if let Some(name) = model_name {
            if let Some(stmts) = self
                .registry()
                .generate_node_cypher_drop_ddl(name, include_type_constraints)
            {
                return Ok(stmts);
            }
            if let Some(stmts) = self
                .registry()
                .generate_rel_cypher_drop_ddl(name, include_type_constraints)
            {
                return Ok(stmts);
            }
            Err(PyValueError::new_err(format!(
                "No node or relationship registered with name '{name}'"
            )))
        } else {
            Ok(self
                .registry()
                .generate_cypher_drop_ddl(include_type_constraints))
        }
    }

    /// Emits an ISO GQL `CREATE GRAPH TYPE <name> AS { ... }` DDL statement.
    #[pyo3(signature = (graph_type_name, model_names=None))]
    fn generate_gql_graph_type_ddl(
        &self,
        graph_type_name: &str,
        model_names: Option<Vec<String>>,
    ) -> PyResult<String> {
        if let Some(names) = model_names {
            let str_names: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
            Ok(self
                .registry()
                .generate_gql_graph_type_ddl_for(graph_type_name, &str_names))
        } else {
            Ok(self.registry().generate_gql_graph_type_ddl(graph_type_name))
        }
    }

    /// Emits an experimental ISO GQL `ALTER CURRENT GRAPH TYPE ADD NODE TYPE` DDL statement.
    fn generate_gql_alter_node_ddl(&self, node_name: &str) -> PyResult<Option<String>> {
        Ok(self.registry().generate_gql_alter_node_ddl(node_name))
    }

    /// Emits an experimental ISO GQL `ALTER CURRENT GRAPH TYPE ADD RELATIONSHIP TYPE` DDL statement.
    #[pyo3(signature = (rel_name, source_label=None, target_label=None))]
    fn generate_gql_alter_rel_ddl(
        &self,
        rel_name: &str,
        source_label: Option<&str>,
        target_label: Option<&str>,
    ) -> PyResult<Option<String>> {
        Ok(self
            .registry()
            .generate_gql_alter_rel_ddl(rel_name, source_label, target_label))
    }

    /// Emits an ISO GQL `DROP GRAPH TYPE` DDL statement.
    fn generate_gql_drop_graph_type_ddl(&self, graph_type_name: &str) -> PyResult<String> {
        Ok(self
            .registry()
            .generate_gql_drop_graph_type_ddl(graph_type_name))
    }

    /// Emits a SQL:2023 PGQ / DuckPGQ `CREATE PROPERTY GRAPH` DDL statement.
    #[pyo3(signature = (graph_name, model_names=None))]
    fn generate_pgq_ddl(
        &self,
        graph_name: &str,
        model_names: Option<Vec<String>>,
    ) -> PyResult<String> {
        if let Some(names) = model_names {
            let str_names: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
            self.registry()
                .generate_pgq_ddl_for(graph_name, &str_names)
                .map_err(|e| PyValueError::new_err(e.to_string()))
        } else {
            self.registry()
                .generate_pgq_ddl(graph_name)
                .map_err(|e| PyValueError::new_err(e.to_string()))
        }
    }

    /// Emits a SQL:2023 PGQ / DuckPGQ `DROP PROPERTY GRAPH` DDL statement.
    fn generate_pgq_drop_ddl(&self, graph_name: &str) -> PyResult<String> {
        Ok(self.registry().generate_pgq_drop_ddl(graph_name))
    }

    /// Emits a Neo4j Cypher 25 `ALTER CURRENT GRAPH TYPE SET { ... }` DDL statement.
    #[pyo3(signature = (model_names=None))]
    fn generate_cypher25_graph_type_ddl(
        &self,
        model_names: Option<Vec<String>>,
    ) -> PyResult<String> {
        if let Some(names) = model_names {
            let str_names: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
            Ok(self
                .registry()
                .generate_cypher25_graph_type_ddl_for(&str_names))
        } else {
            Ok(self.registry().generate_cypher25_graph_type_ddl())
        }
    }

    /// Emits a Neo4j Cypher 25 `ALTER CURRENT GRAPH TYPE SET {}` statement to reset the graph type.
    fn generate_cypher25_drop_graph_type_ddl(&self) -> PyResult<String> {
        Ok(self.registry().generate_cypher25_drop_graph_type_ddl())
    }

    /// Generates multi-dialect CREATE INDEX DDL statements for registered models.
    #[pyo3(signature = (dialect="cypher", model_names=None, graph_name=None))]
    fn generate_index_ddl(
        &self,
        dialect: &str,
        model_names: Option<Vec<String>>,
        graph_name: Option<&str>,
    ) -> PyResult<Vec<String>> {
        let str_names: Option<Vec<&str>> = model_names
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());
        self.registry()
            .generate_index_ddl_with_graph(dialect, str_names.as_deref(), graph_name)
            .map_err(to_py_schema_err)
    }

    /// Generates multi-dialect CREATE CONSTRAINT DDL statements for registered models.
    #[pyo3(signature = (dialect="cypher", model_names=None, include_type_constraints=false, graph_name=None))]
    fn generate_constraint_ddl(
        &self,
        dialect: &str,
        model_names: Option<Vec<String>>,
        include_type_constraints: bool,
        graph_name: Option<&str>,
    ) -> PyResult<Vec<String>> {
        let str_names: Option<Vec<&str>> = model_names
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());
        self.registry()
            .generate_constraint_ddl_with_graph(
                dialect,
                str_names.as_deref(),
                include_type_constraints,
                graph_name,
            )
            .map_err(to_py_schema_err)
    }

    /// Generates multi-dialect DROP INDEX DDL statements for registered models.
    #[pyo3(signature = (dialect="cypher", model_names=None, graph_name=None))]
    fn generate_drop_index_ddl(
        &self,
        dialect: &str,
        model_names: Option<Vec<String>>,
        graph_name: Option<&str>,
    ) -> PyResult<Vec<String>> {
        let str_names: Option<Vec<&str>> = model_names
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());
        self.registry()
            .generate_drop_index_ddl_with_graph(dialect, str_names.as_deref(), graph_name)
            .map_err(to_py_schema_err)
    }

    /// Generates multi-dialect DROP CONSTRAINT DDL statements for registered models.
    #[pyo3(signature = (dialect="cypher", model_names=None, include_type_constraints=false, graph_name=None))]
    fn generate_drop_constraint_ddl(
        &self,
        dialect: &str,
        model_names: Option<Vec<String>>,
        include_type_constraints: bool,
        graph_name: Option<&str>,
    ) -> PyResult<Vec<String>> {
        let str_names: Option<Vec<&str>> = model_names
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());
        self.registry()
            .generate_drop_constraint_ddl_with_graph(
                dialect,
                str_names.as_deref(),
                include_type_constraints,
                graph_name,
            )
            .map_err(to_py_schema_err)
    }

    /// Validates a query string against this schema registry instance.
    #[pyo3(signature = (query))]
    fn validate_query<'py>(&self, query: &str, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let topology = GraphTopology::from_query_str(query);
        let report = self.registry().validate_topology(&topology);
        conformance_report_to_py_dict(&report, py)
    }

    fn __repr__(&self) -> String {
        format!(
            "NativeSchemaRegistry(nodes={}, relationships={})",
            self.registry().node_schemas().len(),
            self.registry().relationship_schemas().len()
        )
    }
}

#[pyfunction]
#[pyo3(signature = (node, include_type_constraints=false))]
fn emit_cypher_node_ddl(
    node: &Bound<'_, PyAny>,
    include_type_constraints: bool,
) -> PyResult<Vec<String>> {
    let schema = extract_node_schema(node)?;
    Ok(voyager_ogm::emit_cypher_node_ddl(
        &schema,
        include_type_constraints,
    ))
}

#[pyfunction]
#[pyo3(signature = (rel, include_type_constraints=false))]
fn emit_cypher_rel_ddl(
    rel: &Bound<'_, PyAny>,
    include_type_constraints: bool,
) -> PyResult<Vec<String>> {
    let schema = extract_rel_schema(rel)?;
    Ok(voyager_ogm::emit_cypher_rel_ddl(
        &schema,
        include_type_constraints,
    ))
}

#[pyfunction]
#[pyo3(signature = (node, include_type_constraints=false))]
fn emit_cypher_drop_node_ddl(
    node: &Bound<'_, PyAny>,
    include_type_constraints: bool,
) -> PyResult<Vec<String>> {
    let schema = extract_node_schema(node)?;
    Ok(voyager_ogm::emit_cypher_drop_node_ddl(
        &schema,
        include_type_constraints,
    ))
}

#[pyfunction]
#[pyo3(signature = (rel, include_type_constraints=false))]
fn emit_cypher_drop_rel_ddl(
    rel: &Bound<'_, PyAny>,
    include_type_constraints: bool,
) -> PyResult<Vec<String>> {
    let schema = extract_rel_schema(rel)?;
    Ok(voyager_ogm::emit_cypher_drop_rel_ddl(
        &schema,
        include_type_constraints,
    ))
}

#[pyfunction]
fn emit_gql_alter_node_ddl(node: &Bound<'_, PyAny>) -> PyResult<String> {
    let schema = extract_node_schema(node)?;
    Ok(voyager_ogm::emit_gql_alter_node_ddl(&schema))
}

#[pyfunction]
#[pyo3(signature = (rel, source_label=None, target_label=None))]
fn emit_gql_alter_rel_ddl(
    rel: &Bound<'_, PyAny>,
    source_label: Option<&str>,
    target_label: Option<&str>,
) -> PyResult<String> {
    let schema = extract_rel_schema(rel)?;
    Ok(voyager_ogm::emit_gql_alter_rel_ddl(
        &schema,
        source_label,
        target_label,
    ))
}

#[pyfunction]
#[pyo3(signature = (graph_type_name, nodes=None, rels=None))]
fn emit_gql_graph_type_ddl(
    graph_type_name: &str,
    nodes: Option<Vec<Bound<'_, PyAny>>>,
    rels: Option<Vec<Bound<'_, PyAny>>>,
) -> PyResult<String> {
    let mut node_schemas = Vec::new();
    if let Some(n_list) = nodes {
        for n in &n_list {
            node_schemas.push(extract_node_schema(n)?);
        }
    }
    let mut rel_schemas = Vec::new();
    if let Some(r_list) = rels {
        for r in &r_list {
            rel_schemas.push(extract_rel_schema(r)?);
        }
    }
    let node_refs: Vec<&NodeSchema> = node_schemas.iter().collect();
    let rel_refs: Vec<&RelationshipSchema> = rel_schemas.iter().collect();
    Ok(voyager_ogm::emit_gql_graph_type_ddl(
        graph_type_name,
        &node_refs,
        &rel_refs,
    ))
}

#[pyfunction]
fn emit_gql_drop_graph_type_ddl(graph_type_name: &str) -> PyResult<String> {
    Ok(voyager_ogm::emit_gql_drop_graph_type_ddl(graph_type_name))
}

#[pyfunction]
#[pyo3(signature = (graph_name, nodes=None, rels=None))]
fn emit_pgq_property_graph_ddl(
    graph_name: &str,
    nodes: Option<Vec<Bound<'_, PyAny>>>,
    rels: Option<Vec<Bound<'_, PyAny>>>,
) -> PyResult<String> {
    let mut node_schemas = Vec::new();
    if let Some(n_list) = nodes {
        for n in &n_list {
            node_schemas.push(extract_node_schema(n)?);
        }
    }
    let mut rel_schemas = Vec::new();
    if let Some(r_list) = rels {
        for r in &r_list {
            rel_schemas.push(extract_rel_schema(r)?);
        }
    }
    let node_refs: Vec<&NodeSchema> = node_schemas.iter().collect();
    let rel_refs: Vec<&RelationshipSchema> = rel_schemas.iter().collect();
    voyager_ogm::emit_pgq_property_graph_ddl(graph_name, &node_refs, &rel_refs)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn emit_pgq_drop_property_graph_ddl(graph_name: &str) -> PyResult<String> {
    Ok(voyager_ogm::emit_pgq_drop_property_graph_ddl(graph_name))
}

#[pyfunction]
#[pyo3(signature = (nodes=None, rels=None))]
fn emit_cypher25_graph_type_ddl(
    nodes: Option<Vec<Bound<'_, PyAny>>>,
    rels: Option<Vec<Bound<'_, PyAny>>>,
) -> PyResult<String> {
    let mut node_schemas = Vec::new();
    if let Some(n_list) = nodes {
        for n in &n_list {
            node_schemas.push(extract_node_schema(n)?);
        }
    }
    let mut rel_schemas = Vec::new();
    if let Some(r_list) = rels {
        for r in &r_list {
            rel_schemas.push(extract_rel_schema(r)?);
        }
    }
    let node_refs: Vec<&NodeSchema> = node_schemas.iter().collect();
    let rel_refs: Vec<&RelationshipSchema> = rel_schemas.iter().collect();
    Ok(voyager_ogm::emit_cypher25_graph_type_ddl(
        &node_refs, &rel_refs,
    ))
}

#[pyfunction]
fn emit_cypher25_drop_graph_type_ddl() -> PyResult<String> {
    Ok(voyager_ogm::emit_cypher25_drop_graph_type_ddl())
}

#[pyfunction]
#[pyo3(signature = (node, dialect="cypher", graph_name=None))]
fn emit_node_index_ddl(
    node: &Bound<'_, PyAny>,
    dialect: &str,
    graph_name: Option<&str>,
) -> PyResult<Vec<String>> {
    let schema = extract_node_schema(node)?;
    voyager_ogm::emit_node_index_ddl_with_graph(&schema, dialect, graph_name)
        .map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (rel, dialect="cypher", graph_name=None))]
fn emit_rel_index_ddl(
    rel: &Bound<'_, PyAny>,
    dialect: &str,
    graph_name: Option<&str>,
) -> PyResult<Vec<String>> {
    let schema = extract_rel_schema(rel)?;
    voyager_ogm::emit_rel_index_ddl_with_graph(&schema, dialect, graph_name)
        .map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (node, dialect="cypher", include_type_constraints=false, graph_name=None))]
fn emit_node_constraint_ddl(
    node: &Bound<'_, PyAny>,
    dialect: &str,
    include_type_constraints: bool,
    graph_name: Option<&str>,
) -> PyResult<Vec<String>> {
    let schema = extract_node_schema(node)?;
    voyager_ogm::emit_node_constraint_ddl_with_graph(
        &schema,
        dialect,
        include_type_constraints,
        graph_name,
    )
    .map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (rel, dialect="cypher", include_type_constraints=false, graph_name=None))]
fn emit_rel_constraint_ddl(
    rel: &Bound<'_, PyAny>,
    dialect: &str,
    include_type_constraints: bool,
    graph_name: Option<&str>,
) -> PyResult<Vec<String>> {
    let schema = extract_rel_schema(rel)?;
    voyager_ogm::emit_rel_constraint_ddl_with_graph(
        &schema,
        dialect,
        include_type_constraints,
        graph_name,
    )
    .map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (node, dialect="cypher", graph_name=None))]
fn emit_node_drop_index_ddl(
    node: &Bound<'_, PyAny>,
    dialect: &str,
    graph_name: Option<&str>,
) -> PyResult<Vec<String>> {
    let schema = extract_node_schema(node)?;
    voyager_ogm::emit_node_drop_index_ddl_with_graph(&schema, dialect, graph_name)
        .map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (rel, dialect="cypher", graph_name=None))]
fn emit_rel_drop_index_ddl(
    rel: &Bound<'_, PyAny>,
    dialect: &str,
    graph_name: Option<&str>,
) -> PyResult<Vec<String>> {
    let schema = extract_rel_schema(rel)?;
    voyager_ogm::emit_rel_drop_index_ddl_with_graph(&schema, dialect, graph_name)
        .map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (node, dialect="cypher", include_type_constraints=false, graph_name=None))]
fn emit_node_drop_constraint_ddl(
    node: &Bound<'_, PyAny>,
    dialect: &str,
    include_type_constraints: bool,
    graph_name: Option<&str>,
) -> PyResult<Vec<String>> {
    let schema = extract_node_schema(node)?;
    voyager_ogm::emit_node_drop_constraint_ddl_with_graph(
        &schema,
        dialect,
        include_type_constraints,
        graph_name,
    )
    .map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (rel, dialect="cypher", include_type_constraints=false, graph_name=None))]
fn emit_rel_drop_constraint_ddl(
    rel: &Bound<'_, PyAny>,
    dialect: &str,
    include_type_constraints: bool,
    graph_name: Option<&str>,
) -> PyResult<Vec<String>> {
    let schema = extract_rel_schema(rel)?;
    voyager_ogm::emit_rel_drop_constraint_ddl_with_graph(
        &schema,
        dialect,
        include_type_constraints,
        graph_name,
    )
    .map_err(to_py_schema_err)
}

/// Returns the canonical deterministic vector index name for a given label and property across dialects.
///
/// Note: Defaults to Cypher naming (`index_{label.to_lowercase()}_{prop.to_lowercase()}`)
/// if `dialect` is omitted or unrecognized.
#[pyfunction]
#[pyo3(signature = (label, prop, dialect=None))]
fn canonical_vector_index_name(label: &str, prop: &str, dialect: Option<&str>) -> PyResult<String> {
    let ddl_dialect = if let Some(d) = dialect {
        DdlDialect::parse_str(d).unwrap_or(DdlDialect::Cypher)
    } else {
        DdlDialect::Cypher
    };
    Ok(core_canonical_vector_index_name(label, prop, ddl_dialect))
}

/// Returns the canonical deterministic full-text index name for a given label and properties across dialects.
///
/// Note: Defaults to Cypher naming (`index_{label.to_lowercase()}_{props.join("_").to_lowercase()}`)
/// if `dialect` is omitted or unrecognized.
#[pyfunction]
#[pyo3(signature = (label, properties, dialect=None))]
fn canonical_fulltext_index_name(
    label: &str,
    properties: Vec<String>,
    dialect: Option<&str>,
) -> PyResult<String> {
    let ddl_dialect = if let Some(d) = dialect {
        DdlDialect::parse_str(d).unwrap_or(DdlDialect::Cypher)
    } else {
        DdlDialect::Cypher
    };
    Ok(core_canonical_fulltext_index_name(
        label,
        &properties,
        ddl_dialect,
    ))
}

/// Adapts a Memgraph MAGE Tantivy query using quote-aware lexical scanning.
#[pyfunction]
#[pyo3(signature = (query, properties=vec![]))]
fn adapt_tantivy_query(query: &str, properties: Vec<String>) -> PyResult<String> {
    Ok(core_adapt_tantivy_query(query, &properties))
}

/// Resolves cross-dialect full-text procedure name, target arguments, and query adaptation.
#[allow(clippy::too_many_arguments)]
#[pyfunction]
#[pyo3(signature = (dialect, is_relationship, target_name, falkordb_target, query_string, properties=vec![]))]
fn resolve_fulltext_search<'py>(
    py: Python<'py>,
    dialect: &str,
    is_relationship: bool,
    target_name: &str,
    falkordb_target: &str,
    query_string: &str,
    properties: Vec<String>,
) -> PyResult<Bound<'py, PyDict>> {
    let d = DdlDialect::parse_str(dialect).unwrap_or(DdlDialect::Cypher);
    let resolved = core_resolve_fulltext_search(
        d,
        is_relationship,
        target_name,
        falkordb_target,
        query_string,
        &properties,
    );
    let dict = PyDict::new(py);
    dict.set_item("procedure", resolved.procedure)?;
    dict.set_item("target_argument", resolved.target_argument)?;
    dict.set_item("query_string", resolved.query_string)?;
    dict.set_item("yield_entity", resolved.yield_entity)?;
    Ok(dict)
}

/// Native Python module definition for `_voyager_rs`.
#[pymodule]
fn _voyager_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(generate_synthetic_stream, m)?)?;
    m.add_function(wrap_pyfunction!(compile_query_from_spec, m)?)?;
    m.add_function(wrap_pyfunction!(extract_topology_from_query, m)?)?;
    m.add_function(wrap_pyfunction!(validate_query_against_schema, m)?)?;
    m.add_function(wrap_pyfunction!(compile_bulk_create, m)?)?;
    m.add_function(wrap_pyfunction!(compile_bulk_merge, m)?)?;
    m.add_function(wrap_pyfunction!(compile_bulk_merge_create_only, m)?)?;
    m.add_function(wrap_pyfunction!(compile_bulk_create_rel, m)?)?;
    m.add_function(wrap_pyfunction!(get_query_cache_stats, m)?)?;
    m.add_function(wrap_pyfunction!(clear_query_cache, m)?)?;
    m.add_function(wrap_pyfunction!(get_runtime_pid, m)?)?;
    m.add_function(wrap_pyfunction!(emit_cypher_node_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_cypher_rel_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_cypher_drop_node_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_cypher_drop_rel_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_gql_alter_node_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_gql_alter_rel_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_gql_graph_type_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_gql_drop_graph_type_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_pgq_property_graph_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_pgq_drop_property_graph_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_cypher25_graph_type_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_cypher25_drop_graph_type_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_node_index_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_rel_index_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_node_constraint_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_rel_constraint_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_node_drop_index_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_rel_drop_index_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_node_drop_constraint_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(emit_rel_drop_constraint_ddl, m)?)?;
    m.add_function(wrap_pyfunction!(canonical_vector_index_name, m)?)?;
    m.add_function(wrap_pyfunction!(canonical_fulltext_index_name, m)?)?;
    m.add_function(wrap_pyfunction!(adapt_tantivy_query, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_fulltext_search, m)?)?;
    m.add_class::<PyQueryBuilder>()?;
    m.add_class::<PyAstExpr>()?;
    m.add_class::<PyArrowStream>()?;
    m.add_class::<PyNativeQueryResult>()?;
    m.add_class::<PyNativeClient>()?;
    m.add_class::<PyNativeTransaction>()?;
    m.add_class::<PyNativeCircuitRouter>()?;
    m.add_class::<PyUnitOfWork>()?;
    m.add_class::<PyUnitOfWorkScope>()?;
    m.add_class::<PyNativeSchemaRegistry>()?;
    m.add("VoyagerError", m.py().get_type::<VoyagerError>())?;
    // pyo3::create_exception! generates a standard PyErr type whose C-level tp_init rejects keyword arguments.
    // We install a custom Python __init__ so callers can instantiate VoyagerError(message, code="...") ergonomics.
    // `skipped` is initialized to an empty list on every instance; schema helpers replace it with the
    // statements skipped on Neo4j Community. The class-level default also covers errors raised from Rust,
    // which bypass this Python __init__.
    m.py().run(
        c"def _init(self, message='', *args, code=None, **kwargs):\n    super(type(self), self).__init__(message, *args, **kwargs)\n    self.code = code\n    self.path = None\n    self.skipped = []\nVoyagerError.__init__ = _init\nVoyagerError.path = None\nVoyagerError.skipped = []\n",
        Some(&m.dict()),
        None,
    )?;
    m.add("__version__", voyager_ogm::VERSION)?;
    Ok(())
}
