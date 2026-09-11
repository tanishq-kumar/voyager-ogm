//! Python PyO3 bindings and Apache Arrow bridge for Voyager OGM.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use voyager_core::ast::{AggregationFunc, BinaryOp, Direction, LiteralValue, NodeHandle, UnaryOp};
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{AgeEmitter, CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_core::optimizer::{AstOptimizer, OptimizationLevel};
use voyager_core::visitor::AstVisitor;

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
                                _ => AggregationFunc::Count,
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
                            let alias: Option<String> = tuple.get_item(2)?.extract()?;
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

    if let Some(limit_item) = spec.get_item("limit")? {
        if let Ok(limit) = limit_item.extract::<u64>() {
            builder.limit(limit);
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

    fn where_expr(&mut self, expr: &Bound<'_, PyAny>) -> PyResult<()> {
        let h = py_to_node_handle(&mut self.inner, expr)?;
        self.inner.where_expr(h);
        Ok(())
    }

    #[pyo3(signature = (expr, alias=None))]
    fn select_expr(&mut self, expr: &Bound<'_, PyAny>, alias: Option<String>) -> PyResult<()> {
        let h = py_to_node_handle(&mut self.inner, expr)?;
        self.inner.select_expr(h, alias);
        Ok(())
    }

    #[pyo3(signature = (expr, alias=None))]
    fn custom_expr(&mut self, expr: &Bound<'_, PyAny>, alias: Option<String>) -> PyResult<()> {
        self.select_expr(expr, alias)
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
        let dict = PyDict::new(py);
        dict.set_item("statement", cached.statement)?;
        let params_dict = PyDict::new(py);
        for (param_name, val) in cached.parameters {
            params_dict.set_item(param_name, literal_to_py(&val, py)?)?;
        }
        dict.set_item("parameters", params_dict)?;
        return Ok(dict);
    }

    let mut builder = QueryBuilder::new();
    build_query_from_spec_internal(&mut builder, spec)?;
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

    let dict = PyDict::new(py);
    dict.set_item("statement", compiled.statement)?;

    let params_dict = PyDict::new(py);
    for (k, v) in compiled.parameters {
        params_dict.set_item(k, literal_to_py(&v, py)?)?;
    }
    dict.set_item("parameters", params_dict)?;

    Ok(dict)
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

/// Native Python module definition for `_voyager_rs`.
#[pymodule]
fn _voyager_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(generate_synthetic_stream, m)?)?;
    m.add_function(wrap_pyfunction!(compile_query_from_spec, m)?)?;
    m.add_function(wrap_pyfunction!(compile_bulk_create, m)?)?;
    m.add_function(wrap_pyfunction!(compile_bulk_merge, m)?)?;
    m.add_function(wrap_pyfunction!(compile_bulk_create_rel, m)?)?;
    m.add_function(wrap_pyfunction!(get_query_cache_stats, m)?)?;
    m.add_function(wrap_pyfunction!(clear_query_cache, m)?)?;
    m.add_function(wrap_pyfunction!(get_runtime_pid, m)?)?;
    m.add_class::<PyQueryBuilder>()?;
    m.add_class::<PyArrowStream>()?;
    m.add_class::<PyNativeQueryResult>()?;
    m.add_class::<PyNativeClient>()?;
    m.add_class::<PyUnitOfWork>()?;
    m.add_class::<PyTransaction>()?;
    m.add("__version__", voyager_core::VERSION)?;
    Ok(())
}
