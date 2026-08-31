//! 32-bit Integer Handle Memory Arena and AST node definitions.

use crate::error::{Error, Result};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::fmt;

/// Lightweight 32-bit handle referencing an AST node inside [`QueryAstArena`].
///
/// Using a 32-bit index avoids 64-bit pointer indirection overhead,
/// prevents reference cycles, and guarantees cache-local contiguous packing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(transparent)]
pub struct NodeHandle(pub u32);

impl NodeHandle {
    /// Sentinel null handle indicating no node or absent relationship.
    pub const NULL: Self = Self(u32::MAX);

    /// Checks whether this handle is the sentinel null handle.
    #[inline(always)]
    pub const fn is_null(self) -> bool {
        self.0 == u32::MAX
    }

    /// Returns the raw 32-bit integer offset.
    #[inline(always)]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for NodeHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            write!(f, "NodeHandle(NULL)")
        } else {
            write!(f, "NodeHandle(#{})", self.0)
        }
    }
}

/// Traversal direction for relationship edge patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Direction {
    /// Outgoing edge: `(a)-[r]->(b)`
    Outgoing,
    /// Incoming edge: `(a)<-[r]-(b)`
    Incoming,
    /// Undirected edge: `(a)-[r]-(b)`
    Undirected,
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Outgoing => write!(f, "->"),
            Self::Incoming => write!(f, "<-"),
            Self::Undirected => write!(f, "-"),
        }
    }
}

/// Binary operators for filters and arithmetic expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum BinaryOp {
    /// Equality (`=`)
    Eq,
    /// Inequality (`!=` or `<>`)
    Neq,
    /// Less than (`<`)
    Lt,
    /// Less than or equal (`<=`)
    Lte,
    /// Greater than (`>`)
    Gt,
    /// Greater than or equal (`>=`)
    Gte,
    /// Membership test (`IN`)
    In,
    /// Non-membership test (`NOT IN`)
    NotIn,
    /// String contains substring (`CONTAINS`)
    Contains,
    /// String starts with prefix (`STARTS WITH`)
    StartsWith,
    /// String ends with suffix (`ENDS WITH`)
    EndsWith,
    /// Regular expression match (`=~`)
    RegexMatch,
    /// Logical conjunction (`AND`)
    And,
    /// Logical disjunction (`OR`)
    Or,
    /// Logical exclusion (`XOR`)
    Xor,
}

impl fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eq => write!(f, "="),
            Self::Neq => write!(f, "!="),
            Self::Lt => write!(f, "<"),
            Self::Lte => write!(f, "<="),
            Self::Gt => write!(f, ">"),
            Self::Gte => write!(f, ">="),
            Self::In => write!(f, "IN"),
            Self::NotIn => write!(f, "NOT IN"),
            Self::Contains => write!(f, "CONTAINS"),
            Self::StartsWith => write!(f, "STARTS WITH"),
            Self::EndsWith => write!(f, "ENDS WITH"),
            Self::RegexMatch => write!(f, "=~"),
            Self::And => write!(f, "AND"),
            Self::Or => write!(f, "OR"),
            Self::Xor => write!(f, "XOR"),
        }
    }
}

/// Literal values representable in AST query nodes and parameter maps.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LiteralValue {
    /// Null literal
    Null,
    /// Boolean literal (`true` / `false`)
    Bool(bool),
    /// 64-bit integer literal
    Int64(i64),
    /// 64-bit floating point literal
    Float64(f64),
    /// String literal
    String(String),
    /// Parameter placeholder (e.g. `$p0` or `:p0`)
    ParameterRef(String),
    /// Array/List of literal values
    List(Vec<LiteralValue>),
}

impl fmt::Display for LiteralValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "null"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int64(i) => write!(f, "{i}"),
            Self::Float64(fl) => write!(f, "{fl}"),
            Self::String(s) => write!(f, "'{}'", s.replace('\'', "\\'")),
            Self::ParameterRef(p) => write!(f, "{p}"),
            Self::List(l) => {
                write!(f, "[")?;
                for (i, item) in l.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
        }
    }
}

/// Standard aggregation functions supported across graph query dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum AggregationFunc {
    /// Total count of elements: `COUNT(x)`
    Count,
    /// Distinct count of elements: `COUNT(DISTINCT x)`
    CountDistinct,
    /// Sum of numeric elements: `SUM(x)`
    Sum,
    /// Average of numeric elements: `AVG(x)`
    Avg,
    /// Minimum value: `MIN(x)`
    Min,
    /// Maximum value: `MAX(x)`
    Max,
    /// Collect elements into a list: `COLLECT(x)`
    Collect,
}

impl fmt::Display for AggregationFunc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Count => write!(f, "count"),
            Self::CountDistinct => write!(f, "count_distinct"),
            Self::Sum => write!(f, "sum"),
            Self::Avg => write!(f, "avg"),
            Self::Min => write!(f, "min"),
            Self::Max => write!(f, "max"),
            Self::Collect => write!(f, "collect"),
        }
    }
}

/// Projection column item inside a RETURN or SELECT clause.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ProjectionItem {
    /// Target expression handle to project
    pub expression: NodeHandle,
    /// Optional output column alias (e.g. `AS alias_name`)
    pub alias: Option<String>,
    /// Optional aggregation function applied to the expression
    pub aggregation: Option<AggregationFunc>,
}

impl ProjectionItem {
    /// Creates a direct projection item without alias or aggregation.
    pub const fn simple(expression: NodeHandle) -> Self {
        Self {
            expression,
            alias: None,
            aggregation: None,
        }
    }

    /// Creates an aliased projection item.
    pub fn aliased(expression: NodeHandle, alias: impl Into<String>) -> Self {
        Self {
            expression,
            alias: Some(alias.into()),
            aggregation: None,
        }
    }

    /// Creates an aggregated projection item with alias.
    pub fn aggregate(
        expression: NodeHandle,
        func: AggregationFunc,
        alias: Option<impl Into<String>>,
    ) -> Self {
        Self {
            expression,
            alias: alias.map(Into::into),
            aggregation: Some(func),
        }
    }
}

/// The core Abstract Syntax Tree (AST) node enum.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum AstNode {
    /// Graph Node pattern: `(variable:Label1:Label2 { predicates })`
    NodePattern {
        /// Optional node variable / binding alias (e.g. `p` in `(p:Person)`)
        variable: Option<String>,
        /// Labels associated with the vertex (e.g. `vec!["Person"]`)
        labels: Vec<String>,
        /// Inlined or attached predicate handles
        predicates: Vec<NodeHandle>,
    },
    /// Relationship pattern: `-[variable:TYPE1|TYPE2*min..max]->`
    EdgePattern {
        /// Optional edge variable alias (e.g. `r` in `-[r:KNOWS]->`)
        variable: Option<String>,
        /// Relationship type labels (e.g. `vec!["KNOWS"]`)
        edge_types: Vec<String>,
        /// Edge traversal direction
        direction: Direction,
        /// Minimum path hops for variable-length traversals (e.g. `1` in `*1..3`)
        min_hops: Option<u32>,
        /// Maximum path hops for variable-length traversals (e.g. `3` in `*1..3`)
        max_hops: Option<u32>,
        /// Inlined predicates on relationship properties
        predicates: Vec<NodeHandle>,
        /// Destination target node handle
        target_node: NodeHandle,
    },
    /// Connected path sequence starting at a node followed by 1..N edge patterns.
    PathChain {
        /// Starting node handle
        start_node: NodeHandle,
        /// Sequence of connected edge pattern handles
        edges: Vec<NodeHandle>,
    },
    /// Binary expression (e.g. `left = right`, `age > 21`, `a AND b`).
    BinaryExpression {
        /// Left-hand operand handle
        left: NodeHandle,
        /// Operator
        op: BinaryOp,
        /// Right-hand operand handle
        right: NodeHandle,
    },
    /// Property access: `target.property_name` (e.g. `p.age`).
    PropertyAccess {
        /// Variable / Target handle
        target: NodeHandle,
        /// Property name string
        property: String,
    },
    /// Literal constant value or query parameter.
    Literal(LiteralValue),
    /// Named identifier reference (e.g. `p`, `director_name`).
    Identifier(String),
    /// Explicit named query parameter reference: `$batch`, `$user_id`.
    Parameter(String),
    /// UNWIND clause for batch unrolling: `UNWIND $batch AS row`.
    UnwindClause {
        /// Expression handle representing the batch list (Parameter / Literal / Identifier)
        expression: NodeHandle,
        /// Alias identifier name for each unrolled row (e.g. "row")
        alias: String,
    },
    /// Dedicated WHERE clause block.
    WhereClause {
        /// Root predicate expression handle
        root_predicate: NodeHandle,
    },
    /// MATCH pattern clause block.
    MatchClause {
        /// Whether this is an `OPTIONAL MATCH`
        optional: bool,
        /// Path pattern handles in this match clause
        paths: Vec<NodeHandle>,
        /// Optional WHERE filter attached directly to this match block
        where_clause: Option<NodeHandle>,
    },
    /// RETURN / COLUMNS projection clause.
    ReturnClause {
        /// Whether to return distinct results (`RETURN DISTINCT`)
        distinct: bool,
        /// Projected column items
        projections: Vec<ProjectionItem>,
        /// Order by clauses: `(expression_handle, is_ascending)`
        order_by: Vec<(NodeHandle, bool)>,
        /// Skip / Offset count
        skip: Option<u64>,
        /// Limit count
        limit: Option<u64>,
    },
    /// Database procedure / function call (e.g. `CALL apoc.path.subgraphNodes(...) YIELD node`).
    ProcedureCall {
        /// Procedure namespace (e.g. `Some("apoc.path")`)
        namespace: Option<String>,
        /// Procedure name (e.g. `"subgraphNodes"`)
        procedure: String,
        /// Arguments expression handles
        arguments: Vec<NodeHandle>,
        /// Output yield items
        yield_items: Vec<String>,
    },
    /// CREATE mutation clause creating nodes or relationship paths: `CREATE (p:Person {name: 'Alice'})`.
    CreateClause {
        /// Patterns to create (NodePattern or PathChain handles)
        paths: Vec<NodeHandle>,
    },
    /// MERGE idempotent upsert clause: `MERGE (p:Person {id: $p0}) ON CREATE SET ... ON MATCH SET ...`.
    MergeClause {
        /// Target pattern to match or create (NodePattern or PathChain handle)
        path: NodeHandle,
        /// Optional ON CREATE SET mutation handles
        on_create_set: Vec<NodeHandle>,
        /// Optional ON MATCH SET mutation handles
        on_match_set: Vec<NodeHandle>,
    },
    /// SET property mutation clause: `SET p.age = $p0, p += $props`.
    SetClause {
        /// Mutation assignment expression handles (SetItem handles)
        items: Vec<NodeHandle>,
    },
    /// Property assignment or map merge item in a SET clause.
    SetItem {
        /// Target PropertyAccess or Variable handle
        target: NodeHandle,
        /// Assigned value or map expression handle
        value: NodeHandle,
        /// Whether this is a map merge assignment (`+=`)
        is_merge: bool,
    },
    /// DELETE node or relationship entity clause: `DELETE p` or `DETACH DELETE p`.
    DeleteClause {
        /// Whether to detach connecting relationships before deleting (`DETACH DELETE`)
        detach: bool,
        /// Variable / Identifier handles of entities to delete
        targets: Vec<NodeHandle>,
    },
    /// REMOVE property or label clause: `REMOVE p.age` or `REMOVE p:Inactive`.
    RemoveClause {
        /// Target handles to remove (PropertyAccess or Variable/Label handles)
        items: Vec<NodeHandle>,
    },
    /// Complete graph query statement combining unwinds, match blocks, mutations, and projections.
    QueryStatement {
        /// Sequence of UNWIND clauses
        unwinds: Vec<NodeHandle>,
        /// Sequence of MATCH clauses
        matches: Vec<NodeHandle>,
        /// Sequence of mutation clauses (CREATE, MERGE, SET, DELETE, REMOVE)
        mutations: Vec<NodeHandle>,
        /// Optional RETURN projection clause
        return_clause: Option<NodeHandle>,
    },
}

/// Contiguous 32-bit memory arena allocator for AST nodes.
#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct QueryAstArena {
    nodes: Vec<AstNode>,
}

impl QueryAstArena {
    /// Creates a new empty memory arena with standard default capacity.
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            nodes: Vec::with_capacity(32),
        }
    }

    /// Creates an arena with pre-allocated node capacity.
    #[inline(always)]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(capacity),
        }
    }

    /// Allocates an AST node into the arena and returns its 32-bit handle.
    #[inline(always)]
    pub fn alloc(&mut self, node: AstNode) -> NodeHandle {
        let index = self.nodes.len() as u32;
        self.nodes.push(node);
        NodeHandle(index)
    }

    /// Retrieves an immutable reference to an AST node by handle.
    #[inline(always)]
    pub fn get(&self, handle: NodeHandle) -> Result<&AstNode> {
        self.nodes
            .get(handle.0 as usize)
            .ok_or(Error::InvalidNodeHandle(handle.0))
    }

    /// Retrieves a mutable reference to an AST node by handle.
    #[inline(always)]
    pub fn get_mut(&mut self, handle: NodeHandle) -> Result<&mut AstNode> {
        self.nodes
            .get_mut(handle.0 as usize)
            .ok_or(Error::InvalidNodeHandle(handle.0))
    }

    /// Returns the number of allocated AST nodes in the arena.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Checks if the arena is empty.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Resets the arena memory for reuse across query compilations without reallocating.
    #[inline(always)]
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    /// Captures the current allocation checkpoint position of the arena.
    #[inline(always)]
    pub fn checkpoint(&self) -> usize {
        self.nodes.len()
    }

    /// Rolls back arena allocations to a prior checkpoint, discarding newly allocated nodes.
    #[inline(always)]
    pub fn rollback_to(&mut self, checkpoint: usize) {
        if checkpoint < self.nodes.len() {
            self.nodes.truncate(checkpoint);
        }
    }
}
