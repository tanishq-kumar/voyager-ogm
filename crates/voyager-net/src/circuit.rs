//! Thread-safe native connection circuit breaker, failure counters, cooldown timers, and backend routing.
//!
//! Provides [`CircuitRouter`], an atomic and contention-free routing coordinator
//! that manages failover transitions between high-throughput native Rust engine
//! and fallback database bridges across concurrent threads and Tokio tasks.
//!
//! # State Machine
//! - [`CircuitState::Closed`]: Normal operation. All requests route to [`BackendRoute::Native`].
//!   Transient failures increment the failure counter; when `consecutive_failures >= failure_threshold`,
//!   the circuit trips to [`CircuitState::Open`].
//! - [`CircuitState::Open`]: Degradation state. Requests route to [`BackendRoute::Fallback`].
//!   Once the configured cooldown duration elapses, the circuit automatically transitions to [`CircuitState::HalfOpen`].
//! - [`CircuitState::HalfOpen`]: Trial recovery state. A single probe request routes to [`BackendRoute::Probe`].
//!   If the probe succeeds, the circuit returns to [`CircuitState::Closed`].
//!   If the probe fails with a transient error, the circuit immediately resets back to [`CircuitState::Open`].
//!
//! # Error Classification
//! Query syntax errors, schema constraint violations, missing parameters, and type errors are
//! classified as [`ErrorClassification::Semantic`]. Semantic errors never trip the circuit breaker,
//! as they are query-specific defects that would fail identically across all drivers. Only transport
//! timeouts, broken sockets, and pool exhaustion are classified as [`ErrorClassification::Transient`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::NetError;

/// Operational lifecycle state of a [`CircuitRouter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    /// Normal operation: requests are routed to the primary native engine.
    Closed,
    /// Degradation state: primary engine tripped, requests routed to fallback bridge.
    Open,
    /// Trial recovery state: cooldown elapsed, trial probe permitted to test primary engine liveness.
    HalfOpen,
}

impl CircuitState {
    /// Returns the string representation of the state ("closed", "open", "half_open").
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Open => "open",
            Self::HalfOpen => "half_open",
        }
    }

    /// Returns `true` if the state is [`CircuitState::Closed`].
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }

    /// Returns `true` if the state is [`CircuitState::Open`].
    pub fn is_open(&self) -> bool {
        matches!(self, Self::Open)
    }

    /// Returns `true` if the state is [`CircuitState::HalfOpen`].
    pub fn is_half_open(&self) -> bool {
        matches!(self, Self::HalfOpen)
    }
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Execution destination determined by the [`CircuitRouter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendRoute {
    /// Route execution to high-throughput native Rust engine.
    Native,
    /// Route execution to fallback driver/bridge.
    Fallback,
    /// Single trial probe execution on native engine in `HalfOpen` state.
    Probe,
}

impl BackendRoute {
    /// Returns the string representation of the route ("native", "fallback", "probe").
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Fallback => "fallback",
            Self::Probe => "probe",
        }
    }

    /// Returns `true` if routing to the native engine or probing.
    pub fn should_route_native(&self) -> bool {
        matches!(self, Self::Native | Self::Probe)
    }

    /// Returns `true` if routing to the fallback bridge.
    pub fn is_fallback(&self) -> bool {
        matches!(self, Self::Fallback)
    }
}

impl std::fmt::Display for BackendRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Classification of execution errors for circuit-breaker accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClassification {
    /// Query syntax, semantic, type, or constraint error. Never trips the circuit breaker.
    Semantic,
    /// Network timeout, socket disconnect, pool exhaustion, or I/O failure. Counts towards circuit trip.
    Transient,
}

impl ErrorClassification {
    /// Returns the string representation ("semantic" or "transient").
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Transient => "transient",
        }
    }

    /// Returns `true` if the error is semantic.
    pub fn is_semantic(&self) -> bool {
        matches!(self, Self::Semantic)
    }

    /// Returns `true` if the error is transient.
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Transient)
    }
}

impl std::fmt::Display for ErrorClassification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Checks if an error message represents a query syntax, semantic, or constraint error.
///
/// In graph databases and web services, semantic query errors must never trip
/// the circuit breaker or degrade the connection backend, as the failure is
/// specific to the query text rather than the network transport.
///
/// NOTE: String message matching is a transitional heuristic. This will be replaced
/// by structured protocol status code classification (e.g. Neo4j Neo.ClientError.*
/// codes, PostgreSQL SQLSTATE 42xxx/23xxx classes, GQLSTATUS codes) as drivers
/// propagate structured server errors directly.
pub fn is_semantic_error(error_msg: &str) -> bool {
    const SEMANTIC_PATTERNS: &[&str] = &[
        "syntaxerror",
        "syntax error",
        "semanticerror",
        "constraintvalidationfailed",
        "parametermissing",
        "typeerror",
        "entitynotfound",
        "unknown function",
        "invalid input",
        "already exists",
        "does not exist",
        "violates unique constraint",
        "violates not-null constraint",
        "violates foreign key constraint",
    ];

    let lower = error_msg.to_lowercase();
    SEMANTIC_PATTERNS
        .iter()
        .any(|pattern| lower.contains(pattern))
}

/// Classifies an error message as either [`ErrorClassification::Semantic`] or [`ErrorClassification::Transient`].
pub fn classify_error(error_msg: &str) -> ErrorClassification {
    if is_semantic_error(error_msg) {
        ErrorClassification::Semantic
    } else {
        ErrorClassification::Transient
    }
}

/// Classifies a [`NetError`] as either [`ErrorClassification::Semantic`] or [`ErrorClassification::Transient`].
pub fn classify_net_error(err: &NetError) -> ErrorClassification {
    match err {
        NetError::ExecutionError(msg) | NetError::ProtocolError(msg) => classify_error(msg),
        NetError::CoreError(_) | NetError::InvalidUri(_) => ErrorClassification::Semantic,
        _ => ErrorClassification::Transient,
    }
}

/// Configuration settings for [`CircuitRouter`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CircuitConfig {
    /// Number of consecutive transient failures required to trip the circuit to `Open`.
    pub failure_threshold: u32,
    /// Duration before an `Open` circuit transitions to `HalfOpen` to allow a trial probe.
    pub cooldown_duration: Duration,
    /// Number of consecutive successful probes in `HalfOpen` required to restore `Closed` state.
    pub success_threshold: u32,
}

impl Default for CircuitConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 1,
            cooldown_duration: Duration::from_secs(30),
            success_threshold: 1,
        }
    }
}

impl CircuitConfig {
    /// Creates a new configuration with the specified threshold and cooldown duration.
    pub fn new(failure_threshold: u32, cooldown_duration: Duration) -> Self {
        Self {
            failure_threshold: failure_threshold.max(1),
            cooldown_duration,
            success_threshold: 1,
        }
    }

    /// Sets the number of successful probes required in `HalfOpen` to close the circuit.
    pub fn with_success_threshold(mut self, threshold: u32) -> Self {
        self.success_threshold = threshold.max(1);
        self
    }
}

/// Telemetry snapshot of circuit breaker metrics and current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitSnapshot {
    /// Active state of the circuit breaker.
    pub state: CircuitState,
    /// Number of consecutive transient failures.
    pub consecutive_failures: u32,
    /// Number of consecutive successful probes in HalfOpen.
    pub consecutive_successes: u32,
    /// Configured failure threshold.
    pub failure_threshold: u32,
    /// Configured cooldown duration in seconds.
    pub cooldown_seconds: f64,
    /// Remaining cooldown duration in seconds if currently Open, or `None`.
    pub remaining_cooldown_seconds: Option<f64>,
}

#[derive(Debug)]
struct CircuitInner {
    state: CircuitState,
    consecutive_failures: u32,
    consecutive_successes: u32,
    opened_at: Option<Instant>,
    config: CircuitConfig,
}

impl CircuitInner {
    fn new(config: CircuitConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            consecutive_failures: 0,
            consecutive_successes: 0,
            opened_at: None,
            config,
        }
    }

    fn check_and_update_state(&mut self, now: Instant) {
        if self.state == CircuitState::Open
            && let Some(opened_at) = self.opened_at
            && now.duration_since(opened_at) >= self.config.cooldown_duration
        {
            self.state = CircuitState::HalfOpen;
            self.consecutive_successes = 0;
        }
    }
}

/// Thread-safe connection circuit breaker and backend routing coordinator.
///
/// Manages state transitions between `Closed`, `Open`, and `HalfOpen` across
/// concurrent threads and Tokio tasks without GIL contention.
#[derive(Debug, Clone)]
pub struct CircuitRouter {
    inner: Arc<Mutex<CircuitInner>>,
}

impl Default for CircuitRouter {
    fn default() -> Self {
        Self::new(CircuitConfig::default())
    }
}

impl CircuitRouter {
    /// Creates a new `CircuitRouter` with the provided configuration.
    pub fn new(config: CircuitConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CircuitInner::new(config))),
        }
    }

    /// Returns the current state of the circuit breaker, updating `Open` to `HalfOpen`
    /// if the cooldown duration has elapsed.
    pub fn state(&self) -> CircuitState {
        let mut inner = self.inner.lock();
        inner.check_and_update_state(Instant::now());
        inner.state
    }

    /// Determines the routing destination for an incoming query.
    pub fn route(&self) -> BackendRoute {
        let mut inner = self.inner.lock();
        inner.check_and_update_state(Instant::now());
        match inner.state {
            CircuitState::Closed => BackendRoute::Native,
            CircuitState::Open => BackendRoute::Fallback,
            CircuitState::HalfOpen => BackendRoute::Probe,
        }
    }

    /// Returns `true` if the cooldown has elapsed and a trial probe should be executed.
    pub fn should_probe(&self) -> bool {
        let mut inner = self.inner.lock();
        inner.check_and_update_state(Instant::now());
        inner.state == CircuitState::HalfOpen
    }

    /// Returns `true` if the query should be routed to the native engine (either normal or probe).
    pub fn should_route_native(&self) -> bool {
        self.route().should_route_native()
    }

    /// Records a successful query execution.
    ///
    /// Resets consecutive failure counters in `Closed` state, or increments successful probes
    /// in `HalfOpen` state until the threshold is satisfied to restore `Closed`.
    pub fn record_success(&self) {
        let mut inner = self.inner.lock();
        match inner.state {
            CircuitState::Closed => {
                inner.consecutive_failures = 0;
            }
            CircuitState::HalfOpen => {
                inner.consecutive_successes += 1;
                if inner.consecutive_successes >= inner.config.success_threshold {
                    inner.state = CircuitState::Closed;
                    inner.consecutive_failures = 0;
                    inner.consecutive_successes = 0;
                    inner.opened_at = None;
                }
            }
            CircuitState::Open => {
                // In-flight queries completing after the circuit has already tripped
                // to Open must not bypass the cooldown duration or HalfOpen probe discipline.
            }
        }
    }

    /// Records a query failure.
    ///
    /// If `is_transient` is `true`, increments failure counters and trips to `Open`
    /// if the threshold is met, or immediately trips if in `HalfOpen` trial state.
    /// If `is_transient` is `false` (semantic error), the failure is ignored.
    pub fn record_failure(&self, is_transient: bool) {
        if !is_transient {
            return;
        }

        let mut inner = self.inner.lock();
        inner.check_and_update_state(Instant::now());
        match inner.state {
            CircuitState::Closed => {
                inner.consecutive_failures += 1;
                if inner.consecutive_failures >= inner.config.failure_threshold {
                    inner.state = CircuitState::Open;
                    inner.opened_at = Some(Instant::now());
                    inner.consecutive_successes = 0;
                }
            }
            CircuitState::HalfOpen => {
                inner.state = CircuitState::Open;
                inner.consecutive_failures += 1;
                inner.opened_at = Some(Instant::now());
                inner.consecutive_successes = 0;
            }
            CircuitState::Open => {
                inner.consecutive_failures += 1;
                inner.opened_at = Some(Instant::now());
            }
        }
    }

    /// Classifies an error message and records it, returning `true` if counted as a transient failure.
    pub fn record_error(&self, error_msg: &str) -> bool {
        let is_transient = classify_error(error_msg).is_transient();
        self.record_failure(is_transient);
        is_transient
    }

    /// Classifies a [`NetError`] and records it, returning `true` if counted as a transient failure.
    pub fn record_net_error(&self, err: &NetError) -> bool {
        let is_transient = classify_net_error(err).is_transient();
        self.record_failure(is_transient);
        is_transient
    }

    /// Manually trips the circuit breaker to `Open` with an immediate cooldown start.
    pub fn trip(&self) {
        let mut inner = self.inner.lock();
        inner.state = CircuitState::Open;
        inner.opened_at = Some(Instant::now());
        inner.consecutive_successes = 0;
        inner.consecutive_failures += 1;
    }

    /// Manually resets the circuit breaker to `Closed`, clearing all counters and cooldowns.
    pub fn reset(&self) {
        let mut inner = self.inner.lock();
        inner.state = CircuitState::Closed;
        inner.consecutive_failures = 0;
        inner.consecutive_successes = 0;
        inner.opened_at = None;
    }

    /// Returns the current count of consecutive transient failures.
    pub fn consecutive_failures(&self) -> u32 {
        self.inner.lock().consecutive_failures
    }

    /// Returns the current count of consecutive successful probes.
    pub fn consecutive_successes(&self) -> u32 {
        self.inner.lock().consecutive_successes
    }

    /// Returns the configured failure threshold.
    pub fn failure_threshold(&self) -> u32 {
        self.inner.lock().config.failure_threshold
    }

    /// Returns the configured cooldown duration.
    pub fn cooldown_duration(&self) -> Duration {
        self.inner.lock().config.cooldown_duration
    }

    /// Returns the configured cooldown duration in seconds as `f64`.
    pub fn cooldown_seconds(&self) -> f64 {
        self.cooldown_duration().as_secs_f64()
    }

    /// Returns remaining cooldown duration if `Open`, or `None` if `Closed` or cooldown elapsed.
    pub fn remaining_cooldown(&self) -> Option<Duration> {
        let inner = self.inner.lock();
        if inner.state == CircuitState::Open
            && let Some(opened_at) = inner.opened_at
        {
            let elapsed = Instant::now().duration_since(opened_at);
            if elapsed < inner.config.cooldown_duration {
                return Some(inner.config.cooldown_duration - elapsed);
            }
        }
        None
    }

    /// Returns an immutable telemetry snapshot of the circuit router's status.
    pub fn snapshot(&self) -> CircuitSnapshot {
        let inner = self.inner.lock();
        let remaining_cooldown = if inner.state == CircuitState::Open
            && let Some(opened_at) = inner.opened_at
        {
            let elapsed = Instant::now().duration_since(opened_at);
            if elapsed < inner.config.cooldown_duration {
                Some((inner.config.cooldown_duration - elapsed).as_secs_f64())
            } else {
                None
            }
        } else {
            None
        };

        CircuitSnapshot {
            state: inner.state,
            consecutive_failures: inner.consecutive_failures,
            consecutive_successes: inner.consecutive_successes,
            failure_threshold: inner.config.failure_threshold,
            cooldown_seconds: inner.config.cooldown_duration.as_secs_f64(),
            remaining_cooldown_seconds: remaining_cooldown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_is_closed_and_routes_native() {
        let router = CircuitRouter::default();
        assert_eq!(router.state(), CircuitState::Closed);
        assert_eq!(router.route(), BackendRoute::Native);
        assert!(router.should_route_native());
        assert!(!router.should_probe());
        assert_eq!(router.consecutive_failures(), 0);
    }

    #[test]
    fn test_transient_failure_trips_to_open() {
        let config = CircuitConfig::new(2, Duration::from_secs(10));
        let router = CircuitRouter::new(config);

        // First failure: threshold 2 not yet met
        router.record_failure(true);
        assert_eq!(router.state(), CircuitState::Closed);
        assert_eq!(router.route(), BackendRoute::Native);
        assert_eq!(router.consecutive_failures(), 1);

        // Second failure: trips to Open
        router.record_failure(true);
        assert_eq!(router.state(), CircuitState::Open);
        assert_eq!(router.route(), BackendRoute::Fallback);
        assert!(!router.should_route_native());
        assert_eq!(router.consecutive_failures(), 2);
    }

    #[test]
    fn test_semantic_error_never_trips_circuit() {
        let config = CircuitConfig::new(1, Duration::from_secs(10));
        let router = CircuitRouter::new(config);

        // Semantic error ignored
        router.record_failure(false);
        assert_eq!(router.state(), CircuitState::Closed);
        assert_eq!(router.consecutive_failures(), 0);

        let counted = router.record_error("Neo.ClientError.Statement.SyntaxError: Invalid token");
        assert!(!counted);
        assert_eq!(router.state(), CircuitState::Closed);
        assert_eq!(router.consecutive_failures(), 0);

        let counted2 = router.record_error("duplicate key value violates unique constraint");
        assert!(!counted2);
        assert_eq!(router.state(), CircuitState::Closed);
    }

    #[test]
    fn test_cooldown_transition_to_half_open_and_probe_recovery() {
        let config = CircuitConfig::new(1, Duration::from_millis(20));
        let router = CircuitRouter::new(config);

        // Trip to open
        router.record_failure(true);
        assert_eq!(router.state(), CircuitState::Open);
        assert_eq!(router.route(), BackendRoute::Fallback);

        // Wait for cooldown to expire
        std::thread::sleep(Duration::from_millis(35));

        // State transitions to HalfOpen, route becomes Probe
        assert_eq!(router.state(), CircuitState::HalfOpen);
        assert_eq!(router.route(), BackendRoute::Probe);
        assert!(router.should_probe());
        assert!(router.should_route_native());

        // Probe succeeds -> restored to Closed
        router.record_success();
        assert_eq!(router.state(), CircuitState::Closed);
        assert_eq!(router.route(), BackendRoute::Native);
        assert_eq!(router.consecutive_failures(), 0);
    }

    #[test]
    fn test_half_open_probe_failure_immediately_reopens() {
        let config = CircuitConfig::new(1, Duration::from_millis(20));
        let router = CircuitRouter::new(config);

        router.record_failure(true);
        assert_eq!(router.state(), CircuitState::Open);

        std::thread::sleep(Duration::from_millis(35));
        assert_eq!(router.state(), CircuitState::HalfOpen);

        // Probe fails with transient error
        router.record_failure(true);
        assert_eq!(router.state(), CircuitState::Open);
        assert_eq!(router.route(), BackendRoute::Fallback);
    }

    #[test]
    fn test_manual_trip_and_reset() {
        let router = CircuitRouter::default();
        router.trip();
        assert_eq!(router.state(), CircuitState::Open);
        assert_eq!(router.route(), BackendRoute::Fallback);

        router.reset();
        assert_eq!(router.state(), CircuitState::Closed);
        assert_eq!(router.route(), BackendRoute::Native);
        assert_eq!(router.consecutive_failures(), 0);
    }

    #[test]
    fn test_error_classification_heuristics() {
        assert!(is_semantic_error("Neo.ClientError.Statement.SyntaxError"));
        assert!(is_semantic_error("syntax error at or near 'WHERE'"));
        assert!(is_semantic_error(
            "ConstraintValidationFailed: Already exists"
        ));
        assert!(is_semantic_error("ParameterMissing: missing param $id"));
        assert!(is_semantic_error("TypeError: expected integer"));
        assert!(is_semantic_error("EntityNotFound: Node not found"));
        assert!(is_semantic_error("column c.name does not exist"));

        assert!(!is_semantic_error("Connection reset by peer"));
        assert!(!is_semantic_error("Timed out waiting for connection"));
        assert!(!is_semantic_error("Broken pipe"));
        assert!(!is_semantic_error("Connection refused"));
        assert!(!is_semantic_error("PoolExhausted"));
    }

    #[test]
    fn test_concurrent_access() {
        let router = CircuitRouter::new(CircuitConfig::new(100, Duration::from_millis(50)));
        let mut handles = Vec::new();

        for i in 0..10 {
            let r = router.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    if i % 2 == 0 {
                        r.record_failure(true);
                    } else {
                        r.record_success();
                    }
                    let _ = r.route();
                    let _ = r.state();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_record_success_while_open_is_noop() {
        let config = CircuitConfig::new(1, Duration::from_millis(50));
        let router = CircuitRouter::new(config);

        router.record_failure(true);
        assert_eq!(router.state(), CircuitState::Open);

        // In-flight success while Open must not close the circuit or bypass probe discipline
        router.record_success();
        assert_eq!(router.state(), CircuitState::Open);
        assert_eq!(router.route(), BackendRoute::Fallback);

        let snap = router.snapshot();
        assert_eq!(snap.state, CircuitState::Open);
        assert!(snap.remaining_cooldown_seconds.is_some());
        assert!(snap.remaining_cooldown_seconds.unwrap() > 0.0);
    }
}
