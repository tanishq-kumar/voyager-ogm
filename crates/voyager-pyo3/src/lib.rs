//! Python PyO3 bindings and Apache Arrow bridge for Voyager OGM.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pyo3::exceptions::{
    PyNotImplementedError, PyRuntimeError, PyTypeError, PyUserWarning, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};

fn to_py_schema_err(err: voyager_core::Error) -> PyErr {
    match err {
        voyager_core::Error::UnsupportedDialect { .. }
        | voyager_core::Error::UnsupportedFeature { .. } => {
            PyNotImplementedError::new_err(err.to_string())
        }
        _ => PyValueError::new_err(err.to_string()),
    }
}
use voyager_core::ast::{
    AggregationFunc, BinaryOp, Direction, LiteralValue, NodeHandle, QueryAstArena, UnaryOp,
};
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{AgeEmitter, CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_core::optimizer::{AstOptimizer, OptimizationLevel};
use voyager_core::schema::{
    ConformanceReport, FieldDescriptor, FieldType, IndexType, NodeSchema, RelationshipSchema,
    SchemaRegistry, global_schema_registry,
};
use voyager_core::topology::GraphTopology;
use voyager_core::visitor::{AstVisitor, CompiledQuery};

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
    } else if let Ok(b) = val.downcast::<PyBool>() {
        Ok(LiteralValue::Bool(b.is_true()))
    } else if let Ok(i) = val.downcast::<PyInt>() {
        Ok(LiteralValue::Int64(i.extract()?))
    } else if let Ok(f) = val.downcast::<PyFloat>() {
        Ok(LiteralValue::Float64(f.value()))
    } else if let Ok(s) = val.downcast::<PyString>() {
        Ok(LiteralValue::String(s.to_string_lossy().into_owned()))
    } else if let Ok(list) = val.downcast::<PyList>() {
        let mut items = Vec::with_capacity(list.len());
        for item in list {
            items.push(py_to_literal(&item)?);
        }
        Ok(LiteralValue::List(items))
    } else if let Ok(tuple) = val.downcast::<PyTuple>() {
        let mut items = Vec::with_capacity(tuple.len());
        for item in tuple {
            items.push(py_to_literal(&item)?);
        }
        Ok(LiteralValue::List(items))
    } else if let Ok(dict) = val.downcast::<PyDict>() {
        let mut entries = Vec::with_capacity(dict.len());
        for (k, v) in dict {
            let key_str: String = if let Ok(s) = k.downcast::<PyString>() {
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
            val.get_type()
        )))
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

/// Native AST Expression node handle wrapping a lightweight AST arena.
#[pyclass(name = "AstExpr")]
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
        let left_h = py_to_node_handle_depth(&mut b, left, 0)?;
        let right_h = py_to_node_handle_depth(&mut b, right, 0)?;
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
        let op_h = py_to_node_handle_depth(&mut b, operand, 0)?;
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
            arg_handles.push(py_to_node_handle_depth(&mut b, arg, 0)?);
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
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
        let h = b.binary_expr(left_h, BinaryOp::In, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn not_in(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
        let h = b.binary_expr(left_h, BinaryOp::NotIn, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn contains(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
        let h = b.binary_expr(left_h, BinaryOp::Contains, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn startswith(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
        let h = b.binary_expr(left_h, BinaryOp::StartsWith, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn endswith(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = b.import_subarena(self.arena.clone(), self.handle);
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
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
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
        let h = b.binary_expr(left_h, BinaryOp::Add, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __radd__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0)?;
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
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
        let h = b.binary_expr(left_h, BinaryOp::Sub, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __rsub__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0)?;
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
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
        let h = b.binary_expr(left_h, BinaryOp::Mul, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __rmul__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0)?;
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
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
        let h = b.binary_expr(left_h, BinaryOp::Div, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __rtruediv__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0)?;
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
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
        let h = b.binary_expr(left_h, BinaryOp::Mod, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __rmod__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0)?;
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
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
        let h = b.binary_expr(left_h, BinaryOp::And, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __rand__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0)?;
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
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
        let h = b.binary_expr(left_h, BinaryOp::Or, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __ror__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0)?;
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
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
        let h = b.binary_expr(left_h, BinaryOp::Xor, right_h);
        Ok(Self {
            arena: b.into_arena(),
            handle: h,
        })
    }

    fn __rxor__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut b = QueryBuilder::new();
        let left_h = py_to_node_handle_depth(&mut b, other, 0)?;
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
        let right_h = py_to_node_handle_depth(&mut b, other, 0)?;
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

fn py_to_node_handle(builder: &mut QueryBuilder, val: &Bound<'_, PyAny>) -> PyResult<NodeHandle> {
    py_to_node_handle_depth(builder, val, 0)
}

fn py_to_node_handle_depth(
    builder: &mut QueryBuilder,
    val: &Bound<'_, PyAny>,
    depth: usize,
) -> PyResult<NodeHandle> {
    if depth > MAX_AST_DEPTH {
        return Err(PyValueError::new_err(format!(
            "Maximum AST expression recursion depth exceeded ({MAX_AST_DEPTH}). Query is too deeply nested.",
        )));
    }
    // Direct Native AST Expression fast-path:
    if let Ok(py_expr) = val.downcast::<PyAstExpr>() {
        let borrowed = py_expr.borrow();
        return Ok(builder.import_subarena(borrowed.arena.clone(), borrowed.handle));
    }
    if let Ok(native_attr) = val.getattr("_native_expr")
        && let Ok(py_expr) = native_attr.downcast::<PyAstExpr>()
    {
        let borrowed = py_expr.borrow();
        return Ok(builder.import_subarena(borrowed.arena.clone(), borrowed.handle));
    }
    if let Ok(tuple) = val.downcast::<PyTuple>() {
        if tuple.is_empty() {
            return Err(PyValueError::new_err("Empty expression tuple"));
        }
        let tag: String = tuple.get_item(0)?.extract()?;
        match tag.as_str() {
            "prop" => {
                let var: String = tuple.get_item(1)?.extract()?;
                let prop: String = tuple.get_item(2)?.extract()?;
                Ok(builder.prop(var, prop))
            }
            "ident" => {
                let name: String = tuple.get_item(1)?.extract()?;
                Ok(builder.ident(name))
            }
            "param" => {
                let name: String = tuple.get_item(1)?.extract()?;
                Ok(builder.param(name))
            }
            "lit" => {
                let lit = py_to_literal(&tuple.get_item(1)?)?;
                Ok(builder.literal(lit))
            }
            "bin" => {
                let op_str: String = tuple.get_item(1)?.extract()?;
                let op = parse_binary_op(&op_str)?;
                let left = py_to_node_handle_depth(builder, &tuple.get_item(2)?, depth + 1)?;
                let right = py_to_node_handle_depth(builder, &tuple.get_item(3)?, depth + 1)?;
                Ok(builder.binary_expr(left, op, right))
            }
            "unary" => {
                let op_str: String = tuple.get_item(1)?.extract()?;
                let op = parse_unary_op(&op_str)?;
                let operand = py_to_node_handle_depth(builder, &tuple.get_item(2)?, depth + 1)?;
                Ok(builder.unary_expr(op, operand))
            }
            "fn" => {
                let name: String = tuple.get_item(1)?.extract()?;
                let args_py = tuple.get_item(2)?;
                let args_list = args_py.downcast::<PyList>()?;
                let mut arg_handles = Vec::with_capacity(args_list.len());
                for arg in args_list {
                    arg_handles.push(py_to_node_handle_depth(builder, &arg, depth + 1)?);
                }
                Ok(builder.function(name, arg_handles))
            }
            "case" => {
                let op_item = tuple.get_item(1)?;
                let operand = if op_item.is_none() {
                    None
                } else {
                    Some(py_to_node_handle_depth(builder, &op_item, depth + 1)?)
                };
                let branches_list = tuple.get_item(2)?.downcast::<PyList>()?.clone();
                let mut branches = Vec::with_capacity(branches_list.len());
                for item in &branches_list {
                    let branch_tuple = item.downcast::<PyTuple>()?;
                    let w =
                        py_to_node_handle_depth(builder, &branch_tuple.get_item(0)?, depth + 1)?;
                    let t =
                        py_to_node_handle_depth(builder, &branch_tuple.get_item(1)?, depth + 1)?;
                    branches.push((w, t));
                }
                let else_item = tuple.get_item(3)?;
                let else_branch = if else_item.is_none() {
                    None
                } else {
                    Some(py_to_node_handle_depth(builder, &else_item, depth + 1)?)
                };
                Ok(builder.case_when(operand, branches, else_branch))
            }
            "list_comp" => {
                let var: String = tuple.get_item(1)?.extract()?;
                let list_h = py_to_node_handle_depth(builder, &tuple.get_item(2)?, depth + 1)?;
                let wh_item = tuple.get_item(3)?;
                let where_filter = if wh_item.is_none() {
                    None
                } else {
                    Some(py_to_node_handle_depth(builder, &wh_item, depth + 1)?)
                };
                let map_item = tuple.get_item(4)?;
                let map_expr = if map_item.is_none() {
                    None
                } else {
                    Some(py_to_node_handle_depth(builder, &map_item, depth + 1)?)
                };
                Ok(builder.list_comprehension(var, list_h, where_filter, map_expr))
            }
            "pattern_comp" => {
                let path_spec = tuple.get_item(1)?;
                let path_h = build_path_from_py(builder, &path_spec)?;
                let wh_item = tuple.get_item(2)?;
                let where_filter = if wh_item.is_none() {
                    None
                } else {
                    Some(py_to_node_handle_depth(builder, &wh_item, depth + 1)?)
                };
                let proj_h = py_to_node_handle_depth(builder, &tuple.get_item(3)?, depth + 1)?;
                Ok(builder.pattern_comprehension(path_h, where_filter, proj_h))
            }
            "exists" => {
                let sub_spec = tuple.get_item(1)?;
                let sub_h = build_subquery_from_py(builder, &sub_spec)?;
                Ok(builder.exists_subquery(sub_h))
            }
            "count" => {
                let sub_spec = tuple.get_item(1)?;
                let sub_h = build_subquery_from_py(builder, &sub_spec)?;
                Ok(builder.count_subquery(sub_h))
            }
            "list" => {
                let items_list = tuple.get_item(1)?.downcast::<PyList>()?.clone();
                let mut item_handles = Vec::with_capacity(items_list.len());
                for item in &items_list {
                    item_handles.push(py_to_node_handle_depth(builder, &item, depth + 1)?);
                }
                Ok(builder.list_literal(item_handles))
            }
            "alias" => {
                let inner = tuple.get_item(1)?;
                py_to_node_handle_depth(builder, &inner, depth + 1)
            }
            other => Err(PyValueError::new_err(format!(
                "Unknown expression tuple tag: '{other}'"
            ))),
        }
    } else if let Ok(lit) = py_to_literal(val) {
        Ok(builder.literal(lit))
    } else {
        Err(PyValueError::new_err(format!(
            "Cannot convert Python object to AST node handle: {}",
            val.get_type()
        )))
    }
}

fn parse_direction(dir_str: &str) -> Direction {
    match dir_str.to_ascii_lowercase().as_str() {
        "out" | "outgoing" | "->" => Direction::Outgoing,
        "in" | "incoming" | "<-" => Direction::Incoming,
        _ => Direction::Undirected,
    }
}

fn build_path_steps_into_builder(
    builder: &mut QueryBuilder,
    steps: &Bound<'_, PyList>,
) -> PyResult<()> {
    for step in steps {
        let tuple = step.downcast::<PyTuple>()?;
        let tag: String = tuple.get_item(0)?.extract()?;
        match tag.as_str() {
            "node" => {
                let var: Option<String> = tuple.get_item(1)?.extract()?;
                let labels: Vec<String> = tuple.get_item(2)?.extract()?;
                builder.node(var, labels);
            }
            "edge" => {
                let dir_str: String = tuple.get_item(1)?.extract()?;
                let dir = parse_direction(&dir_str);
                let edge_types: Vec<String> = tuple.get_item(2)?.extract()?;
                let var: Option<String> = tuple.get_item(3)?.extract()?;
                let min_hops: Option<u32> = tuple.get_item(4)?.extract()?;
                let max_hops: Option<u32> = tuple.get_item(5)?.extract()?;
                match dir {
                    Direction::Outgoing => builder.to(edge_types, var),
                    Direction::Incoming => builder.from(edge_types, var),
                    Direction::Undirected => builder.edge(edge_types, var),
                };
                if min_hops.is_some() || max_hops.is_some() {
                    let min = min_hops.unwrap_or(1);
                    let max = max_hops.unwrap_or(min);
                    builder.hops(min, max);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn build_path_from_py(builder: &mut QueryBuilder, spec: &Bound<'_, PyAny>) -> PyResult<NodeHandle> {
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
    if let Ok(list) = spec.downcast::<PyList>() {
        let list_cloned = list.clone();
        let sub_h = builder.subquery(move |q| {
            let _ = build_path_steps_into_builder(q, &list_cloned);
        });
        Ok(sub_h)
    } else {
        py_to_node_handle(builder, spec)
    }
}

fn build_subquery_from_py(
    builder: &mut QueryBuilder,
    spec: &Bound<'_, PyAny>,
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
    if let Ok(dict) = spec.downcast::<PyDict>() {
        let dict_cloned = dict.clone();
        let sub_h = builder.subquery(move |q| {
            let _ = build_query_from_spec_internal(q, &dict_cloned);
        });
        Ok(sub_h)
    } else if let Ok(list) = spec.downcast::<PyList>() {
        let list_cloned = list.clone();
        let sub_h = builder.subquery(move |q| {
            q.r#match();
            let _ = build_path_steps_into_builder(q, &list_cloned);
        });
        Ok(sub_h)
    } else {
        py_to_node_handle(builder, spec)
    }
}

#[allow(clippy::collapsible_if)]
fn build_query_from_spec_internal(
    builder: &mut QueryBuilder,
    spec: &Bound<'_, PyDict>,
) -> PyResult<()> {
    if let Some(load_csv_item) = spec.get_item("load_csv")? {
        if !load_csv_item.is_none() {
            let tuple = load_csv_item.downcast::<PyTuple>()?;
            let url: String = tuple.get_item(0)?.extract()?;
            let with_headers: bool = tuple.get_item(1)?.extract()?;
            let alias: String = tuple.get_item(2)?.extract()?;
            builder.load_csv(url, with_headers, alias);
        }
    }

    if let Some(unwinds_item) = spec.get_item("unwinds")? {
        if let Ok(unwinds_list) = unwinds_item.downcast::<PyList>() {
            for item in unwinds_list {
                let tuple = item.downcast::<PyTuple>()?;
                let param_name: String = tuple.get_item(0)?.extract()?;
                let alias: String = tuple.get_item(1)?.extract()?;
                builder.unwind_param(param_name, alias);
            }
        }
    }

    if let Some(matches_item) = spec.get_item("matches")? {
        if let Ok(matches_list) = matches_item.downcast::<PyList>() {
            for m in matches_list {
                let m_dict = m.downcast::<PyDict>()?;
                let is_optional: bool = m_dict
                    .get_item("optional")?
                    .map(|v| v.extract().unwrap_or(false))
                    .unwrap_or(false);
                if is_optional {
                    builder.optional_match();
                } else {
                    builder.r#match();
                }

                if let Some(mode_item) = m_dict.get_item("path_mode")? {
                    if let Ok(m_str) = mode_item.extract::<String>() {
                        let m = match m_str.to_ascii_lowercase().as_str() {
                            "trail" => voyager_core::ast::PathMode::Trail,
                            "simple" => voyager_core::ast::PathMode::Simple,
                            "acyclic" => voyager_core::ast::PathMode::Acyclic,
                            "walk" => voyager_core::ast::PathMode::Walk,
                            _ => voyager_core::ast::PathMode::None,
                        };
                        builder.path_mode(m);
                    }
                }
                if let Some(var_item) = m_dict.get_item("path_variable")? {
                    if let Ok(v_str) = var_item.extract::<String>() {
                        builder.path_variable(v_str);
                    }
                }

                if let Some(paths_item) = m_dict.get_item("paths")? {
                    if let Ok(paths_list) = paths_item.downcast::<PyList>() {
                        for (p_idx, p_steps) in paths_list.iter().enumerate() {
                            if p_idx > 0 {
                                builder.pattern();
                            }
                            if let Ok(steps_list) = p_steps.downcast::<PyList>() {
                                build_path_steps_into_builder(builder, steps_list)?;
                            }
                        }
                    }
                }

                if let Some(where_item) = m_dict.get_item("where")? {
                    if !where_item.is_none() {
                        if let Ok(wh_list) = where_item.downcast::<PyList>() {
                            for wh in wh_list {
                                let h = py_to_node_handle(builder, &wh)?;
                                builder.where_expr(h);
                            }
                        } else {
                            let h = py_to_node_handle(builder, &where_item)?;
                            builder.where_expr(h);
                        }
                    }
                }
            }
        }
    }

    if let Some(mutations_item) = spec.get_item("mutations")? {
        if let Ok(mutations_list) = mutations_item.downcast::<PyList>() {
            for mut_item in mutations_list {
                let tuple = mut_item.downcast::<PyTuple>()?;
                let tag: String = tuple.get_item(0)?.extract()?;
                match tag.as_str() {
                    "create" => {
                        builder.create();
                        let paths_list = tuple.get_item(1)?.downcast::<PyList>()?.clone();
                        for (p_idx, p_steps) in paths_list.iter().enumerate() {
                            if p_idx > 0 {
                                builder.pattern();
                            }
                            if let Ok(steps_list) = p_steps.downcast::<PyList>() {
                                build_path_steps_into_builder(builder, steps_list)?;
                            }
                        }
                    }
                    "merge" => {
                        builder.merge();
                        let path_steps = tuple.get_item(1)?.downcast::<PyList>()?.clone();
                        build_path_steps_into_builder(builder, &path_steps)?;
                        if let Ok(on_creates) = tuple.get_item(2)?.downcast::<PyList>() {
                            for item in on_creates {
                                let s_tuple = item.downcast::<PyTuple>()?;
                                let var: String = s_tuple.get_item(0)?.extract()?;
                                let prop: String = s_tuple.get_item(1)?.extract()?;
                                let val_h = py_to_node_handle(builder, &s_tuple.get_item(2)?)?;
                                builder.on_create_set_expr(var, prop, val_h);
                            }
                        }
                        if let Ok(on_matches) = tuple.get_item(3)?.downcast::<PyList>() {
                            for item in on_matches {
                                let s_tuple = item.downcast::<PyTuple>()?;
                                let var: String = s_tuple.get_item(0)?.extract()?;
                                let prop: String = s_tuple.get_item(1)?.extract()?;
                                let val_h = py_to_node_handle(builder, &s_tuple.get_item(2)?)?;
                                builder.on_match_set_expr(var, prop, val_h);
                            }
                        }
                    }
                    "set" => {
                        let var: String = tuple.get_item(1)?.extract()?;
                        let prop: String = tuple.get_item(2)?.extract()?;
                        let val_h = py_to_node_handle(builder, &tuple.get_item(3)?)?;
                        builder.set_property_expr(var, prop, val_h);
                    }
                    "delete" => {
                        let detach: bool = tuple.get_item(1)?.extract()?;
                        let targets: Vec<String> = tuple.get_item(2)?.extract()?;
                        if detach {
                            builder.detach_delete(targets);
                        } else {
                            builder.delete(targets);
                        }
                    }
                    "remove" => {
                        let var: String = tuple.get_item(1)?.extract()?;
                        let prop: String = tuple.get_item(2)?.extract()?;
                        builder.remove_property(var, prop);
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(with_item) = spec.get_item("with_clauses")? {
        if let Ok(with_list) = with_item.downcast::<PyList>() {
            for item in with_list {
                let with_dict = item.downcast::<PyDict>()?;
                builder.r#with();
                if let Some(distinct_item) = with_dict.get_item("distinct")? {
                    if let Ok(distinct) = distinct_item.extract::<bool>() {
                        if distinct {
                            builder.distinct(true);
                        }
                    }
                }
                if let Some(proj_item) = with_dict.get_item("projections")? {
                    if let Ok(proj_list) = proj_item.downcast::<PyList>() {
                        for p in proj_list {
                            let tuple = p.downcast::<PyTuple>()?;
                            let tag: String = tuple.get_item(0)?.extract()?;
                            match tag.as_str() {
                                "field" => {
                                    let var: String = tuple.get_item(1)?.extract()?;
                                    let prop: String = tuple.get_item(2)?.extract()?;
                                    let alias: Option<String> = tuple.get_item(3)?.extract()?;
                                    builder.field(var, prop, alias);
                                }
                                "expr" => {
                                    let expr_spec = tuple.get_item(1)?;
                                    let mut alias: Option<String> = tuple.get_item(2)?.extract()?;
                                    if alias.is_none() {
                                        if let Ok(spec_tuple) = expr_spec.downcast::<PyTuple>() {
                                            if !spec_tuple.is_empty()
                                                && spec_tuple
                                                    .get_item(0)?
                                                    .extract::<String>()
                                                    .map(|t| t == "alias")
                                                    .unwrap_or(false)
                                            {
                                                alias = spec_tuple.get_item(2)?.extract().ok();
                                            }
                                        }
                                    }
                                    let h = py_to_node_handle(builder, &expr_spec)?;
                                    builder.select_expr(h, alias);
                                }
                                "agg" => {
                                    let var: String = tuple.get_item(1)?.extract()?;
                                    let prop: String = tuple.get_item(2)?.extract()?;
                                    let func_str: String = tuple.get_item(3)?.extract()?;
                                    let alias: Option<String> = tuple.get_item(4)?.extract()?;
                                    let agg = match func_str.to_lowercase().as_str() {
                                        "count" => AggregationFunc::Count,
                                        "count_distinct" => AggregationFunc::CountDistinct,
                                        "sum" => AggregationFunc::Sum,
                                        "avg" => AggregationFunc::Avg,
                                        "min" => AggregationFunc::Min,
                                        "max" => AggregationFunc::Max,
                                        "collect" => AggregationFunc::Collect,
                                        other => {
                                            return Err(pyo3::exceptions::PyValueError::new_err(
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
                                _ => {}
                            }
                        }
                    }
                }
                if let Some(where_item) = with_dict.get_item("where")? {
                    if let Ok(where_list) = where_item.downcast::<PyList>() {
                        for wh in where_list {
                            let h = py_to_node_handle(builder, &wh)?;
                            builder.where_expr(h);
                        }
                    } else if !where_item.is_none() {
                        let h = py_to_node_handle(builder, &where_item)?;
                        builder.where_expr(h);
                    }
                }
                if let Some(order_item) = with_dict.get_item("order_by")? {
                    if let Ok(order_list) = order_item.downcast::<PyList>() {
                        for item in order_list {
                            let tuple = item.downcast::<PyTuple>()?;
                            if let Ok(var) = tuple.get_item(0)?.extract::<String>() {
                                let prop: String = tuple.get_item(1)?.extract()?;
                                let asc: bool = tuple.get_item(2)?.extract()?;
                                builder.order_by_property(var, prop, asc);
                            } else {
                                let expr_h = py_to_node_handle(builder, &tuple.get_item(0)?)?;
                                let asc: bool = tuple.get_item(1)?.extract()?;
                                builder.order_by(expr_h, asc);
                            }
                        }
                    }
                }
                if let Some(skip_item) = with_dict.get_item("skip")? {
                    if let Ok(skip) = skip_item.extract::<u64>() {
                        builder.skip(skip);
                    }
                }
                if let Some(limit_item) = with_dict.get_item("limit")? {
                    if let Ok(limit) = limit_item.extract::<u64>() {
                        builder.limit(limit);
                    }
                }
            }
        }
    }

    if let Some(proj_item) = spec.get_item("projections")? {
        if let Ok(proj_list) = proj_item.downcast::<PyList>() {
            if !proj_list.is_empty() {
                builder.r#return();
                for item in proj_list {
                    let tuple = item.downcast::<PyTuple>()?;
                    let tag: String = tuple.get_item(0)?.extract()?;
                    match tag.as_str() {
                        "field" => {
                            let var: String = tuple.get_item(1)?.extract()?;
                            let prop: String = tuple.get_item(2)?.extract()?;
                            let alias: Option<String> = tuple.get_item(3)?.extract()?;
                            builder.field(var, prop, alias);
                        }
                        "agg" => {
                            let var: String = tuple.get_item(1)?.extract()?;
                            let prop: String = tuple.get_item(2)?.extract()?;
                            let func_str: String = tuple.get_item(3)?.extract()?;
                            let alias: Option<String> = tuple.get_item(4)?.extract()?;
                            let agg = match func_str.to_lowercase().as_str() {
                                "count" => AggregationFunc::Count,
                                "count_distinct" => AggregationFunc::CountDistinct,
                                "sum" => AggregationFunc::Sum,
                                "avg" => AggregationFunc::Avg,
                                "min" => AggregationFunc::Min,
                                "max" => AggregationFunc::Max,
                                "collect" => AggregationFunc::Collect,
                                other => {
                                    return Err(pyo3::exceptions::PyValueError::new_err(format!(
                                        "Unknown aggregation function '{other}'. Expected count, count_distinct, sum, avg, min, max, collect."
                                    )));
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
                            let expr_spec = tuple.get_item(1)?;
                            let mut alias: Option<String> = tuple.get_item(2)?.extract()?;
                            if alias.is_none() {
                                if let Ok(spec_tuple) = expr_spec.downcast::<PyTuple>() {
                                    if !spec_tuple.is_empty()
                                        && spec_tuple
                                            .get_item(0)?
                                            .extract::<String>()
                                            .map(|t| t == "alias")
                                            .unwrap_or(false)
                                    {
                                        alias = spec_tuple.get_item(2)?.extract().ok();
                                    }
                                }
                            }
                            let h = py_to_node_handle(builder, &expr_spec)?;
                            builder.select_expr(h, alias);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    if let Some(distinct_item) = spec.get_item("distinct")? {
        if let Ok(distinct) = distinct_item.extract::<bool>() {
            if distinct {
                builder.distinct(true);
            }
        }
    }

    if let Some(order_item) = spec.get_item("order_by")? {
        if let Ok(order_list) = order_item.downcast::<PyList>() {
            for item in order_list {
                let tuple = item.downcast::<PyTuple>()?;
                if let Ok(var) = tuple.get_item(0)?.extract::<String>() {
                    let prop: String = tuple.get_item(1)?.extract()?;
                    let asc: bool = tuple.get_item(2)?.extract()?;
                    builder.order_by_property(var, prop, asc);
                } else {
                    let expr_h = py_to_node_handle(builder, &tuple.get_item(0)?)?;
                    let asc: bool = tuple.get_item(1)?.extract()?;
                    builder.order_by(expr_h, asc);
                }
            }
        }
    }

    if let Some(skip_item) = spec.get_item("skip")? {
        if let Ok(skip) = skip_item.extract::<u64>() {
            builder.skip(skip);
        }
    }

    if let Some(offset_item) = spec.get_item("offset")? {
        if let Ok(offset) = offset_item.extract::<u64>() {
            builder.offset(offset);
        }
    }

    if let Some(limit_item) = spec.get_item("limit")? {
        if let Ok(limit) = limit_item.extract::<u64>() {
            builder.limit(limit);
        }
    }

    if let Some(linear_item) = spec.get_item("linear_clauses")? {
        if let Ok(linear_list) = linear_item.downcast::<PyList>() {
            for item in linear_list {
                let tuple = item.downcast::<PyTuple>()?;
                let tag: String = tuple.get_item(0)?.extract()?;
                match tag.as_str() {
                    "let" => {
                        let var: String = tuple.get_item(1)?.extract()?;
                        let expr_h = py_to_node_handle(builder, &tuple.get_item(2)?)?;
                        builder.let_(var, expr_h);
                    }
                    "filter" => {
                        let pred_h = py_to_node_handle(builder, &tuple.get_item(1)?)?;
                        builder.filter_(pred_h);
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(mode_item) = spec.get_item("execution_mode")? {
        if let Ok(mode_str) = mode_item.extract::<String>() {
            match mode_str.to_ascii_lowercase().as_str() {
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
                _ => {}
            }
        }
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
            "trail" => voyager_core::ast::PathMode::Trail,
            "simple" => voyager_core::ast::PathMode::Simple,
            "acyclic" => voyager_core::ast::PathMode::Acyclic,
            "walk" => voyager_core::ast::PathMode::Walk,
            "none" | "" => voyager_core::ast::PathMode::None,
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
        let (h, resolved_alias) = if let Ok(tuple) = expr.downcast::<PyTuple>() {
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

        let (mut arena, root) = self.inner.clone().build();

        if optimize {
            let opt_level = OptimizationLevel::from_str_opt(optimization_level);
            let optimizer = AstOptimizer::new(opt_level);
            optimizer
                .optimize(&mut arena, root)
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
        }

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
            "age" | "apache_age" | "postgres_age" => {
                let name = graph_name.unwrap_or_else(|| "age_graph".into());
                let mut emitter = AgeEmitter::new(name);
                emitter
                    .visit_query(&arena, root)
                    .map_err(|e| PyValueError::new_err(e.to_string()))?
            }
            other => {
                return Err(PyValueError::new_err(format!(
                    "Unsupported query dialect: '{other}'. Choose from 'cypher', 'sql_pgq', 'iso_gql', or 'apache_age'."
                )));
            }
        };

        compiled_query_to_py_dict(py, &compiled)
    }

    /// Extracts native graph topology (nodes and edges) directly from the AST arena.
    fn extract_topology<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let (arena, root) = self.inner.clone().build();
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
        let (arena, root) = self.inner.clone().build();
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
    batch: &arrow::record_batch::RecordBatch,
    py: Python<'py>,
) -> PyResult<Bound<'py, PyList>> {
    use arrow::array::*;
    use arrow::datatypes::DataType;

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
#[pyclass(name = "ArrowStream")]
#[derive(Clone)]
pub struct PyArrowStream {
    batch: arrow::record_batch::RecordBatch,
    consumed: Arc<AtomicBool>,
}

impl PyArrowStream {
    /// Creates a new `PyArrowStream` wrapping an Arrow `RecordBatch`.
    pub fn from_batch(batch: arrow::record_batch::RecordBatch) -> Self {
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
        let ffi_stream = voyager_core::arrow::export_batch_to_c_stream(self.batch.clone());
        let name = std::ffi::CString::new("arrow_array_stream").unwrap();
        pyo3::types::PyCapsule::new(py, ffi_stream, Some(name))
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
    let batch = voyager_core::arrow::GraphBatchBuilder::generate_synthetic_nodes(count);
    Ok(PyArrowStream::from_batch(batch))
}

/// Compiles a query spec dictionary directly into a target dialect query with parameters in a single FFI call.
#[pyfunction]
#[pyo3(signature = (spec, dialect="cypher", graph_name=None, optimize=false, optimization_level="standard", use_cache=true))]
fn compile_query_from_spec<'py>(
    py: Python<'py>,
    spec: &Bound<'py, PyDict>,
    dialect: &str,
    graph_name: Option<String>,
    optimize: bool,
    optimization_level: &str,
    use_cache: bool,
) -> PyResult<Bound<'py, PyDict>> {
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
        && let Some(cached) = voyager_core::global_query_cache().get(k)
    {
        return compiled_query_to_py_dict(py, &cached);
    }

    let mut builder = QueryBuilder::new();
    build_query_from_spec_internal(&mut builder, spec)?;
    check_warn_cypher_path_mode(py, dialect, builder.get_path_mode().as_deref())?;
    let (mut arena, root) = builder.build();

    if optimize {
        let opt_level = OptimizationLevel::from_str_opt(optimization_level);
        let optimizer = AstOptimizer::new(opt_level);
        optimizer
            .optimize(&mut arena, root)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
    }

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
        "age" | "apache_age" | "postgres_age" => {
            let name = graph_name.unwrap_or_else(|| "age_graph".into());
            let mut emitter = AgeEmitter::new(name);
            emitter
                .visit_query(&arena, root)
                .map_err(|e| PyValueError::new_err(e.to_string()))?
        }
        other => {
            return Err(PyValueError::new_err(format!(
                "Unsupported query dialect: '{other}'. Choose from 'cypher', 'sql_pgq', 'iso_gql', or 'apache_age'."
            )));
        }
    };

    if let Some(k) = cache_key {
        voyager_core::global_query_cache().put(k, compiled.clone());
    }

    compiled_query_to_py_dict(py, &compiled)
}

/// Returns telemetry metrics for the global compiled query cache.
#[pyfunction]
fn get_query_cache_stats<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
    let m = voyager_core::global_query_cache().metrics();
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
    voyager_core::global_query_cache().clear();
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

/// Compiles a high-speed bulk create Cypher/GQL query: `UNWIND $batch AS row CREATE ...`.
#[pyfunction]
#[pyo3(signature = (label, properties, batch_param="batch", row_alias="row", dialect="cypher"))]
fn compile_bulk_create<'py>(
    py: Python<'py>,
    label: String,
    properties: Vec<String>,
    batch_param: &str,
    row_alias: &str,
    dialect: &str,
) -> PyResult<Bound<'py, PyDict>> {
    let prop_refs: Vec<&str> = properties.iter().map(|s| s.as_str()).collect();
    let compiled = voyager_core::bulk::compile_bulk_create(
        &label,
        &prop_refs,
        batch_param,
        row_alias,
        dialect,
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
#[pyo3(signature = (label, key_property, properties, batch_param="batch", row_alias="row", dialect="cypher"))]
fn compile_bulk_merge<'py>(
    py: Python<'py>,
    label: String,
    key_property: String,
    properties: Vec<String>,
    batch_param: &str,
    row_alias: &str,
    dialect: &str,
) -> PyResult<Bound<'py, PyDict>> {
    let prop_refs: Vec<&str> = properties.iter().map(|s| s.as_str()).collect();
    let compiled = voyager_core::bulk::compile_bulk_merge(
        &label,
        &key_property,
        &prop_refs,
        batch_param,
        row_alias,
        dialect,
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

/// Compiles a high-speed bulk relationship creation query.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (rel_type, properties, from_label, from_key, to_label, to_key, batch_param="batch", row_alias="row", dialect="cypher"))]
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
) -> PyResult<Bound<'py, PyDict>> {
    let prop_refs: Vec<&str> = properties.iter().map(|s| s.as_str()).collect();
    let compiled = voyager_core::bulk::compile_bulk_create_rel(
        &rel_type,
        &prop_refs,
        &from_label,
        &from_key,
        &to_label,
        &to_key,
        batch_param,
        row_alias,
        dialect,
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

fn py_any_to_json_value(val: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    if val.is_none() {
        Ok(serde_json::Value::Null)
    } else if let Ok(b) = val.downcast::<PyBool>() {
        Ok(serde_json::Value::Bool(b.is_true()))
    } else if let Ok(i) = val.downcast::<PyInt>() {
        let val_i: i64 = i.extract()?;
        Ok(serde_json::Value::Number(val_i.into()))
    } else if let Ok(f) = val.downcast::<PyFloat>() {
        if let Some(n) = serde_json::Number::from_f64(f.value()) {
            Ok(serde_json::Value::Number(n))
        } else {
            Ok(serde_json::Value::Null)
        }
    } else if let Ok(s) = val.downcast::<PyString>() {
        Ok(serde_json::Value::String(s.to_string_lossy().into_owned()))
    } else if let Ok(list) = val.downcast::<PyList>() {
        let mut items = Vec::with_capacity(list.len());
        for item in list {
            items.push(py_any_to_json_value(&item)?);
        }
        Ok(serde_json::Value::Array(items))
    } else if let Ok(tuple) = val.downcast::<PyTuple>() {
        let mut items = Vec::with_capacity(tuple.len());
        for item in tuple {
            items.push(py_any_to_json_value(&item)?);
        }
        Ok(serde_json::Value::Array(items))
    } else if let Ok(dict) = val.downcast::<PyDict>() {
        let mut map = serde_json::Map::with_capacity(dict.len());
        for (k, v) in dict {
            let key = if let Ok(s) = k.downcast::<PyString>() {
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
            let key = if let Ok(s) = k.downcast::<PyString>() {
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
#[pyclass(name = "NativeQueryResult")]
#[derive(Clone)]
pub struct PyNativeQueryResult {
    stream: PyArrowStream,
    columns: Vec<String>,
    summary: voyager_net::QuerySummary,
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
    client: voyager_net::NativeClient,
}

#[pymethods]
impl PyNativeClient {
    /// Creates a new `NativeClient` connecting to the given URI with connection pooling.
    #[new]
    #[pyo3(signature = (uri, min_idle=1, max_size=10))]
    fn new(uri: &str, min_idle: usize, max_size: usize) -> PyResult<Self> {
        let pool_config = voyager_net::PoolConfig {
            min_idle,
            max_size,
            ..Default::default()
        };
        let client = voyager_net::NativeClient::new(uri, Some(pool_config))
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
            .allow_threads(|| {
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
                arrow::record_batch::RecordBatch::new_empty(std::sync::Arc::new(
                    arrow::datatypes::Schema::empty(),
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
                    arrow::record_batch::RecordBatch::new_empty(std::sync::Arc::new(
                        arrow::datatypes::Schema::empty(),
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
        py.allow_threads(|| get_tokio_runtime().block_on(async move { client.ping().await }))
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

    /// Closes the connection pool.
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        let client = self.client.clone();
        py.allow_threads(|| get_tokio_runtime().block_on(async move { client.close().await }));
        Ok(())
    }
}

/// Native thread-safe circuit breaker and backend router for Voyager sessions.
#[pyclass(name = "NativeCircuitRouter")]
pub struct PyNativeCircuitRouter {
    router: voyager_net::CircuitRouter,
}

#[pymethods]
impl PyNativeCircuitRouter {
    /// Creates a new `NativeCircuitRouter` with configurable failure threshold, cooldown, and success threshold.
    #[new]
    #[pyo3(signature = (failure_threshold=1, cooldown_seconds=30.0, success_threshold=1))]
    fn new(failure_threshold: u32, cooldown_seconds: f64, success_threshold: u32) -> Self {
        let config = voyager_net::CircuitConfig::new(
            failure_threshold,
            std::time::Duration::from_secs_f64(cooldown_seconds.max(0.0)),
        )
        .with_success_threshold(success_threshold);
        Self {
            router: voyager_net::CircuitRouter::new(config),
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
        voyager_net::is_semantic_error(error_msg)
    }

    /// Classifies an error string as "semantic" or "transient".
    #[staticmethod]
    fn classify_error(error_msg: &str) -> &'static str {
        voyager_net::classify_error(error_msg).as_str()
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
    if let Ok(dict) = obj.downcast::<PyDict>() {
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

        let index_type = if let Some(it) = dict.get_item("index_type")? {
            if it.is_none() {
                None
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

        Ok(FieldDescriptor {
            name: field_name,
            field_type,
            nullable,
            primary_key,
            unique,
            indexed,
            index_type,
            default_value,
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

        let index_type = if obj.hasattr("index_type")? {
            let it = obj.getattr("index_type")?;
            if it.is_none() {
                None
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

        Ok(FieldDescriptor {
            name: field_name,
            field_type,
            nullable,
            primary_key,
            unique,
            indexed,
            index_type,
            default_value,
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
    Ok(dict)
}

fn node_schema_to_py<'py>(py: Python<'py>, node: &NodeSchema) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("name", &node.name)?;
    dict.set_item("labels", &node.labels)?;
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
    Ok(dict)
}

fn relationship_schema_to_py<'py>(
    py: Python<'py>,
    rel: &RelationshipSchema,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("name", &rel.name)?;
    dict.set_item("type_name", &rel.type_name)?;
    dict.set_item("source_labels", &rel.source_labels)?;
    dict.set_item("target_labels", &rel.target_labels)?;
    dict.set_item("directed", rel.directed)?;
    let fields_dict = PyDict::new(py);
    for (k, v) in &rel.fields {
        fields_dict.set_item(k, field_descriptor_to_py(py, v)?)?;
    }
    dict.set_item("fields", fields_dict)?;
    Ok(dict)
}

fn extract_node_schema(obj: &Bound<'_, PyAny>) -> PyResult<NodeSchema> {
    if let Ok(dict) = obj.downcast::<PyDict>() {
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
            if let Ok(f_dict) = f_obj.downcast::<PyDict>() {
                for (k, v) in f_dict {
                    let field_name: String = if let Ok(s) = k.downcast::<PyString>() {
                        s.to_string_lossy().into_owned()
                    } else {
                        k.extract()?
                    };
                    let desc = parse_field_descriptor(&field_name, &v)?;
                    node = node.with_field(desc);
                }
            } else if let Ok(f_list) = f_obj.downcast::<PyList>() {
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
            if let Ok(fields_dict) = fields_any.downcast::<PyDict>() {
                for (k, v) in fields_dict {
                    let field_name: String = if let Ok(s) = k.downcast::<PyString>() {
                        s.to_string_lossy().into_owned()
                    } else {
                        k.extract()?
                    };
                    let desc = parse_field_descriptor(&field_name, &v)?;
                    node = node.with_field(desc);
                }
            }
        }
        Ok(node)
    }
}

fn extract_rel_schema(obj: &Bound<'_, PyAny>) -> PyResult<RelationshipSchema> {
    if let Ok(dict) = obj.downcast::<PyDict>() {
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
            if let Ok(f_dict) = f_obj.downcast::<PyDict>() {
                for (k, v) in f_dict {
                    let field_name: String = if let Ok(s) = k.downcast::<PyString>() {
                        s.to_string_lossy().into_owned()
                    } else {
                        k.extract()?
                    };
                    let desc = parse_field_descriptor(&field_name, &v)?;
                    rel = rel.with_field(desc);
                }
            } else if let Ok(f_list) = f_obj.downcast::<PyList>() {
                for item in f_list {
                    let desc = parse_field_descriptor("", &item)?;
                    rel = rel.with_field(desc);
                }
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
            if let Ok(fields_dict) = fields_any.downcast::<PyDict>() {
                for (k, v) in fields_dict {
                    let field_name: String = if let Ok(s) = k.downcast::<PyString>() {
                        s.to_string_lossy().into_owned()
                    } else {
                        k.extract()?
                    };
                    let desc = parse_field_descriptor(&field_name, &v)?;
                    rel = rel.with_field(desc);
                }
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
    #[pyo3(signature = (name, labels, fields, primary_key=None))]
    fn register_node(
        &self,
        name: String,
        labels: Vec<String>,
        fields: &Bound<'_, PyAny>,
        primary_key: Option<String>,
    ) -> PyResult<()> {
        let mut node_schema = NodeSchema::new(name, labels);
        if let Ok(dict) = fields.downcast::<PyDict>() {
            for (k, v) in dict {
                let field_name: String = if let Ok(s) = k.downcast::<PyString>() {
                    s.to_string_lossy().into_owned()
                } else {
                    k.extract()?
                };
                let desc = parse_field_descriptor(&field_name, &v)?;
                node_schema = node_schema.with_field(desc);
            }
        } else if let Ok(list) = fields.downcast::<PyList>() {
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

        self.registry()
            .register_node(node_schema)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(())
    }

    /// Registers a node schema from a dictionary specification.
    fn register_node_schema(&self, schema: &Bound<'_, PyAny>) -> PyResult<()> {
        if let Ok(dict) = schema.downcast::<PyDict>() {
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
            self.register_node(name, labels, &fields_obj, pk)
        } else {
            Err(PyTypeError::new_err("schema must be a dict"))
        }
    }

    /// Registers a relationship schema with endpoints, fields, and direction.
    #[pyo3(signature = (name, type_name, source_labels=None, target_labels=None, fields=None, directed=true))]
    fn register_relationship(
        &self,
        name: String,
        type_name: String,
        source_labels: Option<Vec<String>>,
        target_labels: Option<Vec<String>>,
        fields: Option<&Bound<'_, PyAny>>,
        directed: bool,
    ) -> PyResult<()> {
        let mut rel_schema = RelationshipSchema::new(name, type_name).directed(directed);
        if let (Some(src), Some(tgt)) = (source_labels, target_labels) {
            rel_schema = rel_schema.with_endpoints(src, tgt);
        }
        if let Some(fields_any) = fields {
            if let Ok(dict) = fields_any.downcast::<PyDict>() {
                for (k, v) in dict {
                    let field_name: String = if let Ok(s) = k.downcast::<PyString>() {
                        s.to_string_lossy().into_owned()
                    } else {
                        k.extract()?
                    };
                    let desc = parse_field_descriptor(&field_name, &v)?;
                    rel_schema = rel_schema.with_field(desc);
                }
            } else if let Ok(list) = fields_any.downcast::<PyList>() {
                for item in list {
                    let desc = parse_field_descriptor("", &item)?;
                    rel_schema = rel_schema.with_field(desc);
                }
            }
        }

        self.registry()
            .register_relationship(rel_schema)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(())
    }

    /// Registers a relationship schema from a dictionary specification.
    fn register_relationship_schema(&self, schema: &Bound<'_, PyAny>) -> PyResult<()> {
        if let Ok(dict) = schema.downcast::<PyDict>() {
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
            self.register_relationship(
                name,
                type_name,
                source_labels,
                target_labels,
                fields_obj.as_ref(),
                directed,
            )
        } else {
            Err(PyTypeError::new_err("schema must be a dict"))
        }
    }

    /// Retrieves a node schema by model name.
    fn get_node<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Option<Bound<'py, PyDict>>> {
        if let Some(node) = self.registry().get_node(name) {
            Ok(Some(node_schema_to_py(py, &node)?))
        } else {
            Ok(None)
        }
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
    #[pyo3(signature = (dialect="cypher", model_names=None))]
    fn generate_index_ddl(
        &self,
        dialect: &str,
        model_names: Option<Vec<String>>,
    ) -> PyResult<Vec<String>> {
        let str_names: Option<Vec<&str>> = model_names
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());
        self.registry()
            .generate_index_ddl(dialect, str_names.as_deref())
            .map_err(to_py_schema_err)
    }

    /// Generates multi-dialect CREATE CONSTRAINT DDL statements for registered models.
    #[pyo3(signature = (dialect="cypher", model_names=None, include_type_constraints=false))]
    fn generate_constraint_ddl(
        &self,
        dialect: &str,
        model_names: Option<Vec<String>>,
        include_type_constraints: bool,
    ) -> PyResult<Vec<String>> {
        let str_names: Option<Vec<&str>> = model_names
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());
        self.registry()
            .generate_constraint_ddl(dialect, str_names.as_deref(), include_type_constraints)
            .map_err(to_py_schema_err)
    }

    /// Generates multi-dialect DROP INDEX DDL statements for registered models.
    #[pyo3(signature = (dialect="cypher", model_names=None))]
    fn generate_drop_index_ddl(
        &self,
        dialect: &str,
        model_names: Option<Vec<String>>,
    ) -> PyResult<Vec<String>> {
        let str_names: Option<Vec<&str>> = model_names
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());
        self.registry()
            .generate_drop_index_ddl(dialect, str_names.as_deref())
            .map_err(to_py_schema_err)
    }

    /// Generates multi-dialect DROP CONSTRAINT DDL statements for registered models.
    #[pyo3(signature = (dialect="cypher", model_names=None, include_type_constraints=false))]
    fn generate_drop_constraint_ddl(
        &self,
        dialect: &str,
        model_names: Option<Vec<String>>,
        include_type_constraints: bool,
    ) -> PyResult<Vec<String>> {
        let str_names: Option<Vec<&str>> = model_names
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());
        self.registry()
            .generate_drop_constraint_ddl(dialect, str_names.as_deref(), include_type_constraints)
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
    Ok(voyager_core::emit_cypher_node_ddl(
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
    Ok(voyager_core::emit_cypher_rel_ddl(
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
    Ok(voyager_core::emit_cypher_drop_node_ddl(
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
    Ok(voyager_core::emit_cypher_drop_rel_ddl(
        &schema,
        include_type_constraints,
    ))
}

#[pyfunction]
fn emit_gql_alter_node_ddl(node: &Bound<'_, PyAny>) -> PyResult<String> {
    let schema = extract_node_schema(node)?;
    Ok(voyager_core::emit_gql_alter_node_ddl(&schema))
}

#[pyfunction]
#[pyo3(signature = (rel, source_label=None, target_label=None))]
fn emit_gql_alter_rel_ddl(
    rel: &Bound<'_, PyAny>,
    source_label: Option<&str>,
    target_label: Option<&str>,
) -> PyResult<String> {
    let schema = extract_rel_schema(rel)?;
    Ok(voyager_core::emit_gql_alter_rel_ddl(
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
    Ok(voyager_core::emit_gql_graph_type_ddl(
        graph_type_name,
        &node_refs,
        &rel_refs,
    ))
}

#[pyfunction]
fn emit_gql_drop_graph_type_ddl(graph_type_name: &str) -> PyResult<String> {
    Ok(voyager_core::emit_gql_drop_graph_type_ddl(graph_type_name))
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
    voyager_core::emit_pgq_property_graph_ddl(graph_name, &node_refs, &rel_refs)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn emit_pgq_drop_property_graph_ddl(graph_name: &str) -> PyResult<String> {
    Ok(voyager_core::emit_pgq_drop_property_graph_ddl(graph_name))
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
    Ok(voyager_core::emit_cypher25_graph_type_ddl(
        &node_refs, &rel_refs,
    ))
}

#[pyfunction]
fn emit_cypher25_drop_graph_type_ddl() -> PyResult<String> {
    Ok(voyager_core::emit_cypher25_drop_graph_type_ddl())
}

#[pyfunction]
#[pyo3(signature = (node, dialect="cypher"))]
fn emit_node_index_ddl(node: &Bound<'_, PyAny>, dialect: &str) -> PyResult<Vec<String>> {
    let schema = extract_node_schema(node)?;
    voyager_core::emit_node_index_ddl(&schema, dialect).map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (rel, dialect="cypher"))]
fn emit_rel_index_ddl(rel: &Bound<'_, PyAny>, dialect: &str) -> PyResult<Vec<String>> {
    let schema = extract_rel_schema(rel)?;
    voyager_core::emit_rel_index_ddl(&schema, dialect).map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (node, dialect="cypher", include_type_constraints=false))]
fn emit_node_constraint_ddl(
    node: &Bound<'_, PyAny>,
    dialect: &str,
    include_type_constraints: bool,
) -> PyResult<Vec<String>> {
    let schema = extract_node_schema(node)?;
    voyager_core::emit_node_constraint_ddl(&schema, dialect, include_type_constraints)
        .map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (rel, dialect="cypher", include_type_constraints=false))]
fn emit_rel_constraint_ddl(
    rel: &Bound<'_, PyAny>,
    dialect: &str,
    include_type_constraints: bool,
) -> PyResult<Vec<String>> {
    let schema = extract_rel_schema(rel)?;
    voyager_core::emit_rel_constraint_ddl(&schema, dialect, include_type_constraints)
        .map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (node, dialect="cypher"))]
fn emit_node_drop_index_ddl(node: &Bound<'_, PyAny>, dialect: &str) -> PyResult<Vec<String>> {
    let schema = extract_node_schema(node)?;
    voyager_core::emit_node_drop_index_ddl(&schema, dialect).map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (rel, dialect="cypher"))]
fn emit_rel_drop_index_ddl(rel: &Bound<'_, PyAny>, dialect: &str) -> PyResult<Vec<String>> {
    let schema = extract_rel_schema(rel)?;
    voyager_core::emit_rel_drop_index_ddl(&schema, dialect).map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (node, dialect="cypher", include_type_constraints=false))]
fn emit_node_drop_constraint_ddl(
    node: &Bound<'_, PyAny>,
    dialect: &str,
    include_type_constraints: bool,
) -> PyResult<Vec<String>> {
    let schema = extract_node_schema(node)?;
    voyager_core::emit_node_drop_constraint_ddl(&schema, dialect, include_type_constraints)
        .map_err(to_py_schema_err)
}

#[pyfunction]
#[pyo3(signature = (rel, dialect="cypher", include_type_constraints=false))]
fn emit_rel_drop_constraint_ddl(
    rel: &Bound<'_, PyAny>,
    dialect: &str,
    include_type_constraints: bool,
) -> PyResult<Vec<String>> {
    let schema = extract_rel_schema(rel)?;
    voyager_core::emit_rel_drop_constraint_ddl(&schema, dialect, include_type_constraints)
        .map_err(to_py_schema_err)
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
    m.add_class::<PyQueryBuilder>()?;
    m.add_class::<PyAstExpr>()?;
    m.add_class::<PyArrowStream>()?;
    m.add_class::<PyNativeQueryResult>()?;
    m.add_class::<PyNativeClient>()?;
    m.add_class::<PyNativeCircuitRouter>()?;
    m.add_class::<PyUnitOfWork>()?;
    m.add_class::<PyTransaction>()?;
    m.add_class::<PyNativeSchemaRegistry>()?;
    m.add("__version__", voyager_core::VERSION)?;
    Ok(())
}
