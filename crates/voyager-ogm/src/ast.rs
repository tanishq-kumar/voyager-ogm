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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// Path traversal search mode (ISO/IEC 39075:2024 GQL).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PathMode {
    /// Standard / Default traversal without explicit path mode modifier.
    #[default]
    None,
    /// WALK traversal mode (allows repeated nodes and relationships).
    Walk,
    /// TRAIL traversal mode (allows repeated nodes, but forbids duplicate relationships).
    Trail,
    /// SIMPLE traversal mode (forbids repeated nodes, except when start and end nodes are identical).
    Simple,
    /// ACYCLIC traversal mode (strictly forbids repeated nodes and cycles).
    Acyclic,
}

impl PathMode {
    /// Returns the uppercase keyword string representation.
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "",
            Self::Walk => "WALK",
            Self::Trail => "TRAIL",
            Self::Simple => "SIMPLE",
            Self::Acyclic => "ACYCLIC",
        }
    }
}

impl fmt::Display for PathMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Unary operators for boolean negation, arithmetic inversion, and null checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum UnaryOp {
    /// Logical NOT (`NOT`)
    Not,
    /// Arithmetic negation (`-`)
    Neg,
    /// IS NULL check (`IS NULL`)
    IsNull,
    /// IS NOT NULL check (`IS NOT NULL`)
    IsNotNull,
}

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Not => write!(f, "NOT"),
            Self::Neg => write!(f, "-"),
            Self::IsNull => write!(f, "IS NULL"),
            Self::IsNotNull => write!(f, "IS NOT NULL"),
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
    /// Addition (`+`)
    Add,
    /// String concatenation (`||`)
    Concat,
    /// Subtraction (`-`)
    Sub,
    /// Multiplication (`*`)
    Mul,
    /// Division (`/`)
    Div,
    /// Modulo / Remainder (`%`)
    Mod,
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
            Self::Add => write!(f, "+"),
            Self::Concat => write!(f, "||"),
            Self::Sub => write!(f, "-"),
            Self::Mul => write!(f, "*"),
            Self::Div => write!(f, "/"),
            Self::Mod => write!(f, "%"),
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
    /// Key-value property map literal
    Map(Vec<(String, LiteralValue)>),
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
            Self::Map(m) => {
                write!(f, "{{")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
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

/// Execution mode for graph query statements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExecutionMode {
    /// Standard query execution without plan inspection.
    #[default]
    Normal,
    /// Dry run plan explanation without query execution.
    Explain,
    /// Live query execution with runtime profiling metrics.
    Profile,
    /// Both plan explanation and runtime profiling metrics requested.
    ExplainAndProfile,
}

impl ExecutionMode {
    /// Combines the current mode with explain.
    #[inline]
    pub fn with_explain(self) -> Self {
        match self {
            Self::Normal => Self::Explain,
            Self::Explain => Self::Explain,
            Self::Profile | Self::ExplainAndProfile => Self::ExplainAndProfile,
        }
    }

    /// Combines the current mode with profile.
    #[inline]
    pub fn with_profile(self) -> Self {
        match self {
            Self::Normal => Self::Profile,
            Self::Profile => Self::Profile,
            Self::Explain | Self::ExplainAndProfile => Self::ExplainAndProfile,
        }
    }

    /// Returns the string representation.
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Explain => "explain",
            Self::Profile => "profile",
            Self::ExplainAndProfile => "explain_and_profile",
        }
    }
}

impl fmt::Display for ExecutionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
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

/// Typed label expression supporting boolean combinations and wildcards.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LabelExpression {
    /// Single label name (e.g. `Person`)
    Label(String),
    /// Logical AND conjunction (e.g. `:Person & :Employee`)
    And(Box<LabelExpression>, Box<LabelExpression>),
    /// Logical OR disjunction (e.g. `:Teacher | :Student`)
    Or(Box<LabelExpression>, Box<LabelExpression>),
    /// Logical NOT negation (e.g. `!Bot`)
    Not(Box<LabelExpression>),
    /// Wildcard label matching any label (`%` or `*`)
    Wildcard,
}

impl LabelExpression {
    /// Creates a single label expression.
    #[inline]
    pub fn label(name: impl Into<String>) -> Self {
        Self::Label(name.into())
    }

    /// Creates an AND conjunction of two label expressions.
    #[inline]
    pub fn and(left: Self, right: Self) -> Self {
        Self::And(Box::new(left), Box::new(right))
    }

    /// Creates an OR disjunction of two label expressions.
    #[inline]
    pub fn or(left: Self, right: Self) -> Self {
        Self::Or(Box::new(left), Box::new(right))
    }

    /// Creates a NOT negation of a label expression.
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn not(inner: Self) -> Self {
        Self::Not(Box::new(inner))
    }

    /// Creates a wildcard matching any label (`%` or `*`).
    #[inline]
    pub fn wildcard() -> Self {
        Self::Wildcard
    }

    /// Parses a string label expression into a typed [`LabelExpression`].
    pub fn parse(input: &str) -> std::result::Result<Self, String> {
        let tokens = tokenize_label_expr(input)?;
        if tokens.is_empty() {
            return Err("Empty label expression".to_string());
        }
        let mut pos = 0;
        let expr = parse_disjunction(&tokens, &mut pos)?;
        if pos < tokens.len() {
            return Err(format!(
                "Unexpected trailing tokens in label expression '{input}'"
            ));
        }
        Ok(expr)
    }

    /// Parses a label string with heuristic fallback.
    ///
    /// If parsing succeeds, returns the typed expression. If parsing encounters
    /// an error, gracefully falls back to a literal single label with the trimmed text.
    pub fn parse_heuristic(input: &str) -> Self {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Self::Wildcard;
        }
        Self::parse(trimmed).unwrap_or_else(|_| Self::Label(trimmed.to_string()))
    }

    /// Constructs a conjunction expression from an iterator of label strings.
    ///
    /// Empty iterators return `None`. Each string is parsed using [`Self::parse_heuristic`]
    /// and joined with `And`.
    pub fn from_labels<I, S>(labels: I) -> Option<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut iter = labels.into_iter();
        let first = iter.next()?;
        let mut current = Self::parse_heuristic(first.as_ref());
        for next in iter {
            let next_expr = Self::parse_heuristic(next.as_ref());
            current = Self::and(current, next_expr);
        }
        Some(current)
    }

    /// Constructs a disjunction expression from an iterator of relationship types.
    ///
    /// Empty iterators return `None`. In Cypher and ISO GQL, multiple relationship types
    /// (e.g. `[:KNOWS|FOLLOWS]`) indicate alternatives (`Or`).
    pub fn from_edge_types<I, S>(edge_types: I) -> Option<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut iter = edge_types.into_iter();
        let first = iter.next()?;
        let mut current = Self::parse_heuristic(first.as_ref());
        for next in iter {
            let next_expr = Self::parse_heuristic(next.as_ref());
            current = Self::or(current, next_expr);
        }
        Some(current)
    }

    /// Collects all concrete label names referenced within this expression in order of appearance.
    pub fn collect_labels(&self) -> Vec<String> {
        let mut result = Vec::new();
        self.collect_labels_into(&mut result);
        result
    }

    fn collect_labels_into(&self, acc: &mut Vec<String>) {
        match self {
            Self::Label(s) => {
                if !acc.contains(s) {
                    acc.push(s.clone());
                }
            }
            Self::And(l, r) | Self::Or(l, r) => {
                l.collect_labels_into(acc);
                r.collect_labels_into(acc);
            }
            Self::Not(inner) => {
                inner.collect_labels_into(acc);
            }
            Self::Wildcard => {}
        }
    }

    /// Returns `true` if this expression is a pure conjunction of one or more simple labels.
    pub fn is_simple_conjunction(&self) -> bool {
        match self {
            Self::Label(_) => true,
            Self::And(l, r) => l.is_simple_conjunction() && r.is_simple_conjunction(),
            _ => false,
        }
    }

    /// Returns `true` if this expression is a pure disjunction of one or more simple labels.
    pub fn is_simple_disjunction(&self) -> bool {
        match self {
            Self::Label(_) => true,
            Self::Or(l, r) => l.is_simple_disjunction() && r.is_simple_disjunction(),
            _ => false,
        }
    }

    /// If this expression is a simple conjunction, returns the flat list of label names.
    pub fn to_conjunction_labels(&self) -> Option<Vec<String>> {
        if self.is_simple_conjunction() {
            let mut labels = Vec::new();
            self.collect_conjunction_labels(&mut labels);
            Some(labels)
        } else {
            None
        }
    }

    fn collect_conjunction_labels(&self, acc: &mut Vec<String>) {
        match self {
            Self::Label(s) => acc.push(s.clone()),
            Self::And(l, r) => {
                l.collect_conjunction_labels(acc);
                r.collect_conjunction_labels(acc);
            }
            _ => {}
        }
    }

    /// If this expression is a simple disjunction, returns the flat list of label names.
    pub fn to_disjunction_labels(&self) -> Option<Vec<String>> {
        if self.is_simple_disjunction() {
            let mut labels = Vec::new();
            self.collect_disjunction_labels(&mut labels);
            Some(labels)
        } else {
            None
        }
    }

    fn collect_disjunction_labels(&self, acc: &mut Vec<String>) {
        match self {
            Self::Label(s) => acc.push(s.clone()),
            Self::Or(l, r) => {
                l.collect_disjunction_labels(acc);
                r.collect_disjunction_labels(acc);
            }
            _ => {}
        }
    }
}

impl fmt::Display for LabelExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Label(s) => write!(f, "{s}"),
            Self::Wildcard => write!(f, "%"),
            Self::Not(inner) => match &**inner {
                Self::And(_, _) | Self::Or(_, _) => write!(f, "!({inner})"),
                _ => write!(f, "!{inner}"),
            },
            Self::And(left, right) => {
                let left_str = match &**left {
                    Self::Or(_, _) => format!("({left})"),
                    _ => format!("{left}"),
                };
                let right_str = match &**right {
                    Self::Or(_, _) => format!("({right})"),
                    _ => format!("{right}"),
                };
                write!(f, "{left_str} & {right_str}")
            }
            Self::Or(left, right) => {
                write!(f, "{left} | {right}")
            }
        }
    }
}

impl std::ops::Not for LabelExpression {
    type Output = Self;

    #[inline]
    fn not(self) -> Self::Output {
        Self::Not(Box::new(self))
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum LabelToken {
    Ident(String),
    And,
    Or,
    Not,
    Wildcard,
    LParen,
    RParen,
}

fn tokenize_label_expr(input: &str) -> std::result::Result<Vec<LabelToken>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\t' | '\r' | '\n' | ':' => {
                chars.next();
            }
            '&' => {
                chars.next();
                tokens.push(LabelToken::And);
            }
            '|' => {
                chars.next();
                tokens.push(LabelToken::Or);
            }
            '!' => {
                chars.next();
                tokens.push(LabelToken::Not);
            }
            '%' | '*' => {
                chars.next();
                tokens.push(LabelToken::Wildcard);
            }
            '(' => {
                chars.next();
                tokens.push(LabelToken::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(LabelToken::RParen);
            }
            '`' => {
                chars.next();
                let mut name = String::new();
                for c in chars.by_ref() {
                    if c == '`' {
                        break;
                    }
                    name.push(c);
                }
                tokens.push(LabelToken::Ident(name));
            }
            _ if ch.is_alphanumeric() || ch == '_' || ch == '-' => {
                let mut ident = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' || c == '-' {
                        ident.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                match ident.to_ascii_uppercase().as_str() {
                    "AND" => tokens.push(LabelToken::And),
                    "OR" => tokens.push(LabelToken::Or),
                    "NOT" => tokens.push(LabelToken::Not),
                    _ => tokens.push(LabelToken::Ident(ident)),
                }
            }
            _ => {
                return Err(format!("Unexpected character '{ch}' in label expression"));
            }
        }
    }

    Ok(tokens)
}

fn parse_disjunction(
    tokens: &[LabelToken],
    pos: &mut usize,
) -> std::result::Result<LabelExpression, String> {
    let mut left = parse_conjunction(tokens, pos)?;
    while *pos < tokens.len() && tokens[*pos] == LabelToken::Or {
        *pos += 1;
        let right = parse_conjunction(tokens, pos)?;
        left = LabelExpression::or(left, right);
    }
    Ok(left)
}

fn parse_conjunction(
    tokens: &[LabelToken],
    pos: &mut usize,
) -> std::result::Result<LabelExpression, String> {
    let mut left = parse_unary(tokens, pos)?;
    while *pos < tokens.len() && tokens[*pos] == LabelToken::And {
        *pos += 1;
        let right = parse_unary(tokens, pos)?;
        left = LabelExpression::and(left, right);
    }
    Ok(left)
}

fn parse_unary(
    tokens: &[LabelToken],
    pos: &mut usize,
) -> std::result::Result<LabelExpression, String> {
    if *pos < tokens.len() && tokens[*pos] == LabelToken::Not {
        *pos += 1;
        let inner = parse_unary(tokens, pos)?;
        return Ok(LabelExpression::not(inner));
    }
    parse_primary(tokens, pos)
}

fn parse_primary(
    tokens: &[LabelToken],
    pos: &mut usize,
) -> std::result::Result<LabelExpression, String> {
    if *pos >= tokens.len() {
        return Err("Unexpected end of label expression".to_string());
    }

    match &tokens[*pos] {
        LabelToken::Ident(name) => {
            let expr = LabelExpression::label(name.clone());
            *pos += 1;
            Ok(expr)
        }
        LabelToken::Wildcard => {
            *pos += 1;
            Ok(LabelExpression::wildcard())
        }
        LabelToken::LParen => {
            *pos += 1;
            let expr = parse_disjunction(tokens, pos)?;
            if *pos >= tokens.len() || tokens[*pos] != LabelToken::RParen {
                return Err("Unclosed parenthesis in label expression".to_string());
            }
            *pos += 1;
            Ok(expr)
        }
        other => Err(format!("Unexpected token {other:?} in label expression")),
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
        /// Typed label expression associated with the vertex (e.g. `Person & Developer`)
        label_expression: Option<LabelExpression>,
        /// Inlined or attached predicate handles
        predicates: Vec<NodeHandle>,
    },
    /// Relationship pattern: `-[variable:TYPE1|TYPE2*min..max]->`
    EdgePattern {
        /// Optional edge variable alias (e.g. `r` in `-[r:KNOWS]->`)
        variable: Option<String>,
        /// Typed relationship label expression (e.g. `KNOWS | FOLLOWS`)
        label_expression: Option<LabelExpression>,
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
        /// Optional path variable alias (e.g. `p` in `p = ...`)
        path_variable: Option<String>,
        /// Path traversal search mode (WALK, TRAIL, SIMPLE, ACYCLIC)
        path_mode: PathMode,
        /// Starting node handle
        start_node: NodeHandle,
        /// Sequence of connected edge pattern handles
        edges: Vec<NodeHandle>,
    },
    /// Binary expression (e.g. `left = right`, `age > 21`, `a + b`, `a AND b`).
    BinaryExpression {
        /// Left-hand operand handle
        left: NodeHandle,
        /// Operator
        op: BinaryOp,
        /// Right-hand operand handle
        right: NodeHandle,
    },
    /// Unary expression (e.g. `NOT p.active`, `-p.score`, `p.name IS NOT NULL`).
    UnaryExpression {
        /// Operator
        op: UnaryOp,
        /// Operand handle
        operand: NodeHandle,
    },
    /// Scalar, string, or temporal function call (e.g. `toLower(p.name)`, `coalesce(a, b)`, `datetime()`).
    FunctionCall {
        /// Function name (e.g. "toLower", "toUpper", "trim", "split", "coalesce", "size", "head", "tail", "datetime", "date.truncate", "duration")
        name: String,
        /// Arguments expression handles
        arguments: Vec<NodeHandle>,
    },
    /// Conditional CASE expression: `CASE [operand] WHEN w1 THEN t1 ... [ELSE e] END`.
    CaseExpression {
        /// Optional operand for simple case: `CASE p.status WHEN 'A' THEN 1 ...`
        operand: Option<NodeHandle>,
        /// List of `(WHEN condition, THEN result)` branches
        when_then_branches: Vec<(NodeHandle, NodeHandle)>,
        /// Optional `ELSE default_expr` branch
        else_branch: Option<NodeHandle>,
    },
    /// List comprehension: `[x IN list WHERE x > 5 | x * 2]`.
    ListComprehension {
        /// Iteration variable name (e.g. "x")
        variable: String,
        /// Source list expression handle
        list_expression: NodeHandle,
        /// Optional WHERE filter predicate handle
        where_filter: Option<NodeHandle>,
        /// Optional mapping/projection expression handle
        map_expression: Option<NodeHandle>,
    },
    /// Pattern comprehension: `[(u)-[:FRIENDS_WITH]->(f) WHERE f.age > 20 | f.name]`.
    PatternComprehension {
        /// Path pattern handle (NodePattern or PathChain)
        path: NodeHandle,
        /// Optional WHERE filter predicate handle
        where_filter: Option<NodeHandle>,
        /// Projection expression handle
        projection: NodeHandle,
    },
    /// Existential subquery block: `EXISTS { MATCH (u)-[:POSTED]->(p) WHERE p.views > 100 }`.
    ExistsSubquery {
        /// Target subquery handle (PathChain, MatchClause, or QueryStatement)
        subquery: NodeHandle,
    },
    /// Scalar subquery count block: `COUNT { (u)-[:FOLLOWS]->() }`.
    CountSubquery {
        /// Target subquery handle (PathChain, MatchClause, or QueryStatement)
        subquery: NodeHandle,
    },
    /// Explicit list literal of expressions: `[expr1, expr2, ...]`.
    ListLiteral(Vec<NodeHandle>),
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
        /// Execution mode (Normal, Explain, Profile, ExplainAndProfile)
        execution_mode: ExecutionMode,
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
    /// LOAD CSV file ingestion clause: `LOAD CSV WITH HEADERS FROM 'file:///...' AS row`.
    LoadCsvClause {
        /// URL literal or parameter expression handle
        url: NodeHandle,
        /// Whether headers are present: `WITH HEADERS`
        with_headers: bool,
        /// Row variable alias (e.g. `row`)
        alias: String,
    },
    /// WITH intermediate pipeline projection clause: `WITH p, count(m) AS cnt WHERE cnt > 5`.
    WithClause {
        /// Whether distinct: `WITH DISTINCT ...`
        distinct: bool,
        /// Projected column items
        projections: Vec<ProjectionItem>,
        /// Order by clauses: `(expression_handle, is_ascending)`
        order_by: Vec<(NodeHandle, bool)>,
        /// Skip / Offset count
        skip: Option<u64>,
        /// Limit count
        limit: Option<u64>,
        /// Optional WHERE filter attached directly to this WITH clause
        where_clause: Option<NodeHandle>,
    },
    /// LET linear computed variable assignment statement: `LET var = expression`.
    LetClause {
        /// Target computed variable alias name
        variable: String,
        /// Expression handle
        expression: NodeHandle,
    },
    /// FILTER standalone linear record filter statement: `FILTER predicate`.
    FilterClause {
        /// Filter predicate expression handle
        predicate: NodeHandle,
    },
    /// Complete graph query statement combining load csv, unwinds, match blocks, linear statements, with pipelines, mutations, and projections.
    QueryStatement {
        /// Execution mode (Normal, Explain, Profile, ExplainAndProfile)
        execution_mode: ExecutionMode,
        /// Optional LOAD CSV clause
        load_csv: Option<NodeHandle>,
        /// Sequence of UNWIND clauses
        unwinds: Vec<NodeHandle>,
        /// Sequence of MATCH clauses
        matches: Vec<NodeHandle>,
        /// Sequence of linear statements (LET, FILTER)
        linear_clauses: Vec<NodeHandle>,
        /// Sequence of intermediate WITH clauses
        with_clauses: Vec<NodeHandle>,
        /// Sequence of mutation clauses (CREATE, MERGE, SET, DELETE, REMOVE)
        mutations: Vec<NodeHandle>,
        /// Optional RETURN projection clause
        return_clause: Option<NodeHandle>,
    },
}

impl AstNode {
    /// Returns the primary variable name if defined on this node or relationship pattern.
    pub fn variable(&self) -> Option<&str> {
        match self {
            Self::NodePattern { variable, .. } | Self::EdgePattern { variable, .. } => {
                variable.as_deref()
            }
            Self::Identifier(id) => Some(id.as_str()),
            _ => None,
        }
    }

    /// Returns the list of concrete labels referenced by this node pattern, or empty if none.
    pub fn node_labels(&self) -> Vec<String> {
        match self {
            Self::NodePattern {
                label_expression: Some(expr),
                ..
            } => expr.collect_labels(),
            _ => Vec::new(),
        }
    }

    /// Returns the list of relationship types referenced by this edge pattern, or empty if none.
    pub fn edge_types(&self) -> Vec<String> {
        match self {
            Self::EdgePattern {
                label_expression: Some(expr),
                ..
            } => expr.collect_labels(),
            _ => Vec::new(),
        }
    }
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

    /// Returns an immutable slice of all nodes currently stored in the arena.
    #[inline(always)]
    pub fn nodes(&self) -> &[AstNode] {
        &self.nodes
    }

    /// Returns a mutable slice of all nodes currently stored in the arena.
    #[inline(always)]
    pub fn nodes_mut(&mut self) -> &mut [AstNode] {
        &mut self.nodes
    }

    /// Consumes the arena, returning the underlying node vector.
    #[inline(always)]
    pub fn into_nodes(self) -> Vec<AstNode> {
        self.nodes
    }
}
