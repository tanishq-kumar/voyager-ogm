//! PackStream binary serialization and deserialization for Bolt wire protocols.
#![warn(clippy::indexing_slicing)]

use bytes::{Buf, BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{NetError, Result};

/// Maximum recursion depth allowed when decoding nested PackStream structures (Lists, Maps, Structs)
/// to prevent stack overflow denial-of-service on untrusted network payloads.
pub const MAX_PACKSTREAM_DEPTH: usize = 64;

/// Represents a graph Node decoded from a Bolt PackStream structure (`tag = 0x4E`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoltNode {
    /// Internal integer ID (Bolt v1-v4).
    pub id: i64,
    /// List of node labels (e.g. `["Person", "Developer"]`).
    pub labels: Vec<String>,
    /// Property key-value map.
    pub properties: HashMap<String, BoltValue>,
    /// String element ID (Bolt v5+).
    pub element_id: Option<String>,
}

/// Represents a graph Relationship decoded from a Bolt PackStream structure (`tag = 0x52`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoltRelationship {
    /// Internal integer ID (Bolt v1-v4).
    pub id: i64,
    /// Start node internal integer ID.
    pub start_node_id: i64,
    /// End node internal integer ID.
    pub end_node_id: i64,
    /// Relationship type label (e.g. `"FOLLOWS"`).
    pub rel_type: String,
    /// Property key-value map.
    pub properties: HashMap<String, BoltValue>,
    /// String element ID (Bolt v5+).
    pub element_id: Option<String>,
    /// Start node element ID (Bolt v5+).
    pub start_element_id: Option<String>,
    /// End node element ID (Bolt v5+).
    pub end_element_id: Option<String>,
}

/// Represents an unbound relationship inside a graph Path (`tag = 0x72`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoltUnboundRelationship {
    /// Internal integer ID.
    pub id: i64,
    /// Relationship type label.
    pub rel_type: String,
    /// Property key-value map.
    pub properties: HashMap<String, BoltValue>,
    /// String element ID.
    pub element_id: Option<String>,
}

/// Represents a graph Path decoded from a Bolt PackStream structure (`tag = 0x50`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoltPath {
    /// Ordered list of unique nodes in the path.
    pub nodes: Vec<BoltNode>,
    /// Ordered list of unique unbound relationships in the path.
    pub relationships: Vec<BoltUnboundRelationship>,
    /// Traversal sequence indices connecting nodes and relationships.
    pub sequence: Vec<i64>,
}

/// Standard spatial reference system identifiers (SRIDs) for Bolt PackStream spatial types.
pub mod srid {
    /// WGS-84 2D geographic coordinate reference system (longitude, latitude).
    pub const WGS_84_2D: i64 = 4326;
    /// WGS-84 3D geographic coordinate reference system (longitude, latitude, height).
    pub const WGS_84_3D: i64 = 4979;
    /// Cartesian 2D coordinate reference system (x, y).
    pub const CARTESIAN_2D: i64 = 7203;
    /// Cartesian 3D coordinate reference system (x, y, z).
    pub const CARTESIAN_3D: i64 = 9157;
}

/// Represents a 2D spatial point decoded from a Bolt PackStream structure (`tag = 0x58`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoltPoint2D {
    /// Spatial reference system identifier (e.g. 4326 for WGS-84, 7203 for Cartesian).
    pub srid: i64,
    /// X coordinate (or longitude for geographic WGS-84).
    pub x: f64,
    /// Y coordinate (or latitude for geographic WGS-84).
    pub y: f64,
}

/// Represents a 3D spatial point decoded from a Bolt PackStream structure (`tag = 0x59`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoltPoint3D {
    /// Spatial reference system identifier (e.g. 4979 for WGS-84 3D, 9157 for Cartesian 3D).
    pub srid: i64,
    /// X coordinate (or longitude for geographic WGS-84).
    pub x: f64,
    /// Y coordinate (or latitude for geographic WGS-84).
    pub y: f64,
    /// Z coordinate (or height for geographic WGS-84).
    pub z: f64,
}

/// Gregorian civil calendar conversion: days since 1970-01-01 to (year, month, day).
/// Howard Hinnant's civil day algorithm: O(1) arithmetic, zero allocations.
pub fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    let z = days.saturating_add(719468);
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Gregorian civil calendar conversion: (year, month, day) to days since 1970-01-01.
pub fn ymd_to_days(year: i32, month: u32, day: u32) -> i64 {
    let y = if month <= 2 {
        year as i64 - 1
    } else {
        year as i64
    };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let m = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

/// Converts nanoseconds since midnight into (hours, minutes, seconds, sub-second nanoseconds).
pub fn nanos_to_hmsn(nanos: i64) -> (u32, u32, u32, u32) {
    let total_secs = (nanos / 1_000_000_000).rem_euclid(86400);
    let sub_nanos = (nanos.rem_euclid(1_000_000_000)) as u32;
    let hour = (total_secs / 3600) as u32;
    let minute = ((total_secs % 3600) / 60) as u32;
    let second = (total_secs % 60) as u32;
    (hour, minute, second, sub_nanos)
}

/// Formats UTC offset in seconds to ISO-8601 offset string (e.g. "+02:00", "-05:00", "+00:00").
pub fn format_tz_offset(offset_seconds: i64) -> String {
    let sign = if offset_seconds >= 0 { '+' } else { '-' };
    let abs_secs = offset_seconds.unsigned_abs();
    let hours = abs_secs / 3600;
    let minutes = (abs_secs % 3600) / 60;
    let seconds = abs_secs % 60;
    if seconds == 0 {
        format!("{}{:02}:{:02}", sign, hours, minutes)
    } else {
        format!("{}{:02}:{:02}:{:02}", sign, hours, minutes, seconds)
    }
}

fn format_time_string(h: u32, m: u32, s: u32, nanos: u32, offset: Option<&str>) -> String {
    let base = if nanos == 0 {
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else if nanos.is_multiple_of(1_000_000) {
        format!("{:02}:{:02}:{:02}.{:03}", h, m, s, nanos / 1_000_000)
    } else if nanos.is_multiple_of(1_000) {
        format!("{:02}:{:02}:{:02}.{:06}", h, m, s, nanos / 1_000)
    } else {
        format!("{:02}:{:02}:{:02}.{:09}", h, m, s, nanos)
    };
    match offset {
        Some(off) => format!("{}{}", base, off),
        None => base,
    }
}

/// Represents a calendar Date decoded from a Bolt PackStream structure (`tag = 0x44`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BoltDate {
    /// Days since Unix epoch (1970-01-01).
    pub days: i64,
}

impl BoltDate {
    /// Creates a new `BoltDate` from days since epoch.
    pub const fn new(days: i64) -> Self {
        Self { days }
    }

    /// Creates a new `BoltDate` from Gregorian year, month, and day.
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Result<Self> {
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(NetError::ProtocolError(format!(
                "Invalid calendar date: {:04}-{:02}-{:02}",
                year, month, day
            )));
        }
        Ok(Self {
            days: ymd_to_days(year, month, day),
        })
    }

    /// Converts days since epoch into (year, month, day).
    pub fn to_ymd(&self) -> (i32, u32, u32) {
        days_to_ymd(self.days)
    }

    /// Formats the date as an ISO-8601 string (`"YYYY-MM-DD"`).
    pub fn to_iso_string(&self) -> String {
        let (y, m, d) = self.to_ymd();
        format!("{:04}-{:02}-{:02}", y, m, d)
    }
}

/// Represents a Time with timezone offset decoded from a Bolt PackStream structure (`tag = 0x54`).
///
/// ### Bolt Specification & Engine Verification
/// In the Bolt PackStream specification (tag `0x54` / `b"T"`), `nanoseconds` represents
/// **local wall-clock nanoseconds since midnight** (i.e. `(hour * 3600 + min * 60 + sec) * 1e9 + nanos`),
/// while `tz_offset_seconds` represents the timezone offset from UTC.
/// Verified against live Neo4j 5.26 (`RETURN time("14:30:15+02:00")`) and the official `neo4j` Python driver:
/// `hydrate_time(nanoseconds, tz)` decodes `divmod(nanoseconds, 1e9)` directly into wall-clock hour/min/sec
/// and attaches `FixedOffset(tz // 60)` without offset shifting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BoltTime {
    /// Local wall-clock nanoseconds since midnight.
    pub nanoseconds: i64,
    /// Timezone offset from UTC in seconds.
    pub tz_offset_seconds: i64,
}

impl BoltTime {
    /// Creates a new `BoltTime`.
    pub const fn new(nanoseconds: i64, tz_offset_seconds: i64) -> Self {
        Self {
            nanoseconds,
            tz_offset_seconds,
        }
    }

    /// Formats the time as an ISO-8601 string (`"HH:MM:SS.ffffff+HH:MM"`).
    pub fn to_iso_string(&self) -> String {
        let (h, m, s, nanos) = nanos_to_hmsn(self.nanoseconds);
        let offset = format_tz_offset(self.tz_offset_seconds);
        format_time_string(h, m, s, nanos, Some(&offset))
    }
}

/// Represents a local wall-clock Time without timezone decoded from a Bolt PackStream structure (`tag = 0x74`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BoltLocalTime {
    /// Nanoseconds since midnight.
    pub nanoseconds: i64,
}

impl BoltLocalTime {
    /// Creates a new `BoltLocalTime`.
    pub const fn new(nanoseconds: i64) -> Self {
        Self { nanoseconds }
    }

    /// Formats the local time as an ISO-8601 string (`"HH:MM:SS.ffffff"`).
    pub fn to_iso_string(&self) -> String {
        let (h, m, s, nanos) = nanos_to_hmsn(self.nanoseconds);
        format_time_string(h, m, s, nanos, None)
    }
}

/// Represents a DateTime with timezone offset decoded from a Bolt PackStream structure (`tag = 0x49` or `0x46`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BoltDateTime {
    /// Seconds since Unix epoch (1970-01-01T00:00:00Z).
    pub seconds: i64,
    /// Sub-second nanoseconds (0..999_999_999).
    pub nanoseconds: i64,
    /// Timezone offset from UTC in seconds.
    pub tz_offset_seconds: i64,
}

impl BoltDateTime {
    /// Creates a new `BoltDateTime`.
    pub const fn new(seconds: i64, nanoseconds: i64, tz_offset_seconds: i64) -> Self {
        Self {
            seconds,
            nanoseconds,
            tz_offset_seconds,
        }
    }

    /// Formats the datetime as an ISO-8601 string (`"YYYY-MM-DDTHH:MM:SS.ffffff+HH:MM"`).
    pub fn to_iso_string(&self) -> String {
        let wall_secs = self.seconds.saturating_add(self.tz_offset_seconds);
        let days = wall_secs.div_euclid(86400);
        let secs_of_day = wall_secs.rem_euclid(86400);
        let (y, m, d) = days_to_ymd(days);
        let h = (secs_of_day / 3600) as u32;
        let min = ((secs_of_day % 3600) / 60) as u32;
        let sec = (secs_of_day % 60) as u32;
        let offset = format_tz_offset(self.tz_offset_seconds);
        let time_part = format_time_string(h, min, sec, self.nanoseconds as u32, Some(&offset));
        format!("{:04}-{:02}-{:02}T{}", y, m, d, time_part)
    }
}

/// Represents a local DateTime without timezone decoded from a Bolt PackStream structure (`tag = 0x64`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BoltLocalDateTime {
    /// Seconds since local epoch (1970-01-01T00:00:00).
    pub seconds: i64,
    /// Sub-second nanoseconds (0..999_999_999).
    pub nanoseconds: i64,
}

impl BoltLocalDateTime {
    /// Creates a new `BoltLocalDateTime`.
    pub const fn new(seconds: i64, nanoseconds: i64) -> Self {
        Self {
            seconds,
            nanoseconds,
        }
    }

    /// Formats the local datetime as an ISO-8601 string (`"YYYY-MM-DDTHH:MM:SS.ffffff"`).
    pub fn to_iso_string(&self) -> String {
        let days = self.seconds.div_euclid(86400);
        let secs_of_day = self.seconds.rem_euclid(86400);
        let (y, m, d) = days_to_ymd(days);
        let h = (secs_of_day / 3600) as u32;
        let min = ((secs_of_day % 3600) / 60) as u32;
        let sec = (secs_of_day % 60) as u32;
        let time_part = format_time_string(h, min, sec, self.nanoseconds as u32, None);
        format!("{:04}-{:02}-{:02}T{}", y, m, d, time_part)
    }
}

/// Represents a DateTime with named IANA timezone decoded from a Bolt PackStream structure (`tag = 0x4A`, `0x66`, or `0x69`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BoltDateTimeZoneId {
    /// Seconds since Unix epoch (1970-01-01T00:00:00Z).
    pub seconds: i64,
    /// Sub-second nanoseconds (0..999_999_999).
    pub nanoseconds: i64,
    /// IANA timezone identifier (e.g. "Europe/Berlin", "America/New_York", "UTC").
    pub zone_id: String,
}

impl BoltDateTimeZoneId {
    /// Creates a new `BoltDateTimeZoneId`.
    pub fn new(seconds: i64, nanoseconds: i64, zone_id: impl Into<String>) -> Self {
        Self {
            seconds,
            nanoseconds,
            zone_id: zone_id.into(),
        }
    }

    /// Formats the datetime as an ISO-8601 extended string with zone ID (`"YYYY-MM-DDTHH:MM:SS.ffffff[Zone/Id]"`).
    pub fn to_iso_string(&self) -> String {
        let days = self.seconds.div_euclid(86400);
        let secs_of_day = self.seconds.rem_euclid(86400);
        let (y, m, d) = days_to_ymd(days);
        let h = (secs_of_day / 3600) as u32;
        let min = ((secs_of_day % 3600) / 60) as u32;
        let sec = (secs_of_day % 60) as u32;
        let time_part = format_time_string(h, min, sec, self.nanoseconds as u32, Some("Z"));
        format!("{:04}-{:02}-{:02}T{}[{}]", y, m, d, time_part, self.zone_id)
    }
}

/// Represents an ISO-8601 Duration decoded from a Bolt PackStream structure (`tag = 0x45`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BoltDuration {
    /// Number of months.
    pub months: i64,
    /// Number of days.
    pub days: i64,
    /// Number of seconds.
    pub seconds: i64,
    /// Sub-second nanoseconds (0..999_999_999).
    pub nanoseconds: i64,
}

impl BoltDuration {
    /// Creates a new `BoltDuration`.
    pub const fn new(months: i64, days: i64, seconds: i64, nanoseconds: i64) -> Self {
        Self {
            months,
            days,
            seconds,
            nanoseconds,
        }
    }

    /// Formats the duration as an ISO-8601 duration string (e.g. `"P1Y2M3DT4H5M6S"`).
    pub fn to_iso_string(&self) -> String {
        if self.months == 0 && self.days == 0 && self.seconds == 0 && self.nanoseconds == 0 {
            return "PT0S".to_string();
        }
        let mut res = String::from("P");
        if self.months != 0 {
            let years = self.months / 12;
            let rem_months = self.months % 12;
            if years != 0 {
                res.push_str(&format!("{}Y", years));
            }
            if rem_months != 0 || years == 0 {
                res.push_str(&format!("{}M", rem_months));
            }
        }
        if self.days != 0 {
            res.push_str(&format!("{}D", self.days));
        }
        if self.seconds != 0 || self.nanoseconds != 0 {
            res.push('T');
            let h = self.seconds / 3600;
            let m = (self.seconds % 3600) / 60;
            let s = self.seconds % 60;
            if h != 0 {
                res.push_str(&format!("{}H", h));
            }
            if m != 0 {
                res.push_str(&format!("{}M", m));
            }
            if s != 0 || self.nanoseconds != 0 || (h == 0 && m == 0) {
                if self.nanoseconds != 0 {
                    let nanos = self.nanoseconds.unsigned_abs();
                    if nanos.is_multiple_of(1_000_000) {
                        res.push_str(&format!("{}.{:03}S", s, nanos / 1_000_000));
                    } else if nanos.is_multiple_of(1_000) {
                        res.push_str(&format!("{}.{:06}S", s, nanos / 1_000));
                    } else {
                        res.push_str(&format!("{}.{:09}S", s, nanos));
                    }
                } else {
                    res.push_str(&format!("{}S", s));
                }
            }
        }
        res
    }
}

/// Represents any dynamically typed value supported by Bolt PackStream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BoltValue {
    /// Null / empty value (`0xC0`).
    Null,
    /// Boolean flag (`0xC2` false, `0xC3` true).
    Boolean(bool),
    /// 64-bit signed integer.
    Integer(i64),
    /// 64-bit IEEE-754 floating point number (`0xC1`).
    Float(f64),
    /// Raw binary byte buffer (`0xCC`..`0xCE`).
    Bytes(Vec<u8>),
    /// UTF-8 encoded string.
    String(String),
    /// Ordered list of PackStream values.
    List(Vec<BoltValue>),
    /// Key-value dictionary.
    Map(HashMap<String, BoltValue>),
    /// Generic PackStream structure with 1-byte signature tag.
    Structure {
        /// 1-byte structure signature tag.
        tag: u8,
        /// Ordered field elements.
        fields: Vec<BoltValue>,
    },
    /// Graph Node structure (`0x4E`).
    Node(BoltNode),
    /// Graph Relationship structure (`0x52`).
    Relationship(BoltRelationship),
    /// Graph Path structure (`0x50`).
    Path(BoltPath),
    /// 2D spatial point structure (`0x58`).
    Point2D(BoltPoint2D),
    /// 3D spatial point structure (`0x59`).
    Point3D(BoltPoint3D),
    /// Calendar Date structure (`0x44`).
    Date(BoltDate),
    /// Time with timezone offset structure (`0x54`).
    Time(BoltTime),
    /// Local time without timezone structure (`0x74`).
    LocalTime(BoltLocalTime),
    /// DateTime with timezone offset structure (`0x49` or `0x46`).
    DateTime(BoltDateTime),
    /// Local DateTime without timezone structure (`0x64`).
    LocalDateTime(BoltLocalDateTime),
    /// DateTime with named IANA timezone ID structure (`0x4A`, `0x66`, or `0x69`).
    DateTimeZoneId(BoltDateTimeZoneId),
    /// ISO-8601 Duration structure (`0x45`).
    Duration(BoltDuration),
}

impl From<bool> for BoltValue {
    fn from(b: bool) -> Self {
        Self::Boolean(b)
    }
}

impl From<i64> for BoltValue {
    fn from(i: i64) -> Self {
        Self::Integer(i)
    }
}

impl From<i32> for BoltValue {
    fn from(i: i32) -> Self {
        Self::Integer(i as i64)
    }
}

impl From<f64> for BoltValue {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}

impl From<String> for BoltValue {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for BoltValue {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<Vec<BoltValue>> for BoltValue {
    fn from(list: Vec<BoltValue>) -> Self {
        Self::List(list)
    }
}

impl From<HashMap<String, BoltValue>> for BoltValue {
    fn from(map: HashMap<String, BoltValue>) -> Self {
        Self::Map(map)
    }
}

impl From<&serde_json::Value> for BoltValue {
    fn from(json: &serde_json::Value) -> Self {
        match json {
            serde_json::Value::Null => BoltValue::Null,
            serde_json::Value::Bool(b) => BoltValue::Boolean(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    BoltValue::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    BoltValue::Float(f)
                } else {
                    BoltValue::Null
                }
            }
            serde_json::Value::String(s) => BoltValue::String(s.clone()),
            serde_json::Value::Array(arr) => {
                BoltValue::List(arr.iter().map(BoltValue::from).collect())
            }
            serde_json::Value::Object(obj) => {
                let mut map = HashMap::with_capacity(obj.len());
                for (k, v) in obj {
                    map.insert(k.clone(), BoltValue::from(v));
                }
                BoltValue::Map(map)
            }
        }
    }
}

/// PackStream encoder and decoder implementation.
pub struct PackStream;

impl PackStream {
    /// Encodes a `BoltValue` into a mutable byte buffer.
    pub fn encode(value: &BoltValue, buf: &mut BytesMut) {
        match value {
            BoltValue::Null => buf.put_u8(0xC0),
            BoltValue::Boolean(false) => buf.put_u8(0xC2),
            BoltValue::Boolean(true) => buf.put_u8(0xC3),
            BoltValue::Integer(i) => Self::encode_integer(*i, buf),
            BoltValue::Float(f) => {
                buf.put_u8(0xC1);
                buf.put_f64(*f);
            }
            BoltValue::Bytes(bytes) => Self::encode_bytes(bytes, buf),
            BoltValue::String(s) => Self::encode_string(s, buf),
            BoltValue::List(list) => Self::encode_list(list, buf),
            BoltValue::Map(map) => Self::encode_map(map, buf),
            BoltValue::Structure { tag, fields } => Self::encode_structure(*tag, fields, buf),
            BoltValue::Node(node) => Self::encode_node(node, buf),
            BoltValue::Relationship(rel) => Self::encode_relationship(rel, buf),
            BoltValue::Path(path) => Self::encode_path(path, buf),
            BoltValue::Point2D(point) => Self::encode_point2d(point, buf),
            BoltValue::Point3D(point) => Self::encode_point3d(point, buf),
            BoltValue::Date(date) => Self::encode_date(date, buf),
            BoltValue::Time(time) => Self::encode_time(time, buf),
            BoltValue::LocalTime(ltime) => Self::encode_local_time(ltime, buf),
            BoltValue::DateTime(dt) => Self::encode_datetime(dt, buf),
            BoltValue::LocalDateTime(ldt) => Self::encode_local_datetime(ldt, buf),
            BoltValue::DateTimeZoneId(dtz) => Self::encode_datetime_zone_id(dtz, buf),
            BoltValue::Duration(dur) => Self::encode_duration(dur, buf),
        }
    }

    /// Decodes a `BoltValue` from a byte buffer slice with default recursion depth (0).
    pub fn decode(buf: &mut Bytes) -> Result<BoltValue> {
        Self::decode_with_depth(buf, 0)
    }

    /// Decodes a `BoltValue` from a byte buffer slice tracking nesting recursion depth.
    pub fn decode_with_depth(buf: &mut Bytes, depth: usize) -> Result<BoltValue> {
        if depth > MAX_PACKSTREAM_DEPTH {
            return Err(NetError::ProtocolError(format!(
                "PackStream structure exceeded maximum nesting depth of {}",
                MAX_PACKSTREAM_DEPTH
            )));
        }

        if !buf.has_remaining() {
            return Err(NetError::ProtocolError(
                "Unexpected EOF while decoding PackStream value".to_string(),
            ));
        }

        let marker = buf.get_u8();

        // 1. TinyInt (0x00..0x7F positive, 0xF0..0xFF negative)
        if marker <= 0x7F {
            return Ok(BoltValue::Integer(marker as i64));
        }
        if marker >= 0xF0 {
            return Ok(BoltValue::Integer((marker as i8) as i64));
        }

        // 2. TinyString (0x80..0x8F)
        if (0x80..=0x8F).contains(&marker) {
            let len = (marker & 0x0F) as usize;
            return Self::decode_string_payload(len, buf);
        }

        // 3. TinyList (0x90..0x9F)
        if (0x90..=0x9F).contains(&marker) {
            let len = (marker & 0x0F) as usize;
            return Self::decode_list_payload(len, buf, depth);
        }

        // 4. TinyMap (0xA0..0xAF)
        if (0xA0..=0xAF).contains(&marker) {
            let size = (marker & 0x0F) as usize;
            return Self::decode_map_payload(size, buf, depth);
        }

        // 5. TinyStruct (0xB0..0xBF)
        if (0xB0..=0xBF).contains(&marker) {
            let num_fields = (marker & 0x0F) as usize;
            return Self::decode_structure_payload(num_fields, buf, depth);
        }

        // 6. Explicit Marker Bytes
        match marker {
            0xC0 => Ok(BoltValue::Null),
            0xC1 => {
                if buf.remaining() < 8 {
                    return Err(NetError::ProtocolError("Incomplete Float64".to_string()));
                }
                Ok(BoltValue::Float(buf.get_f64()))
            }
            0xC2 => Ok(BoltValue::Boolean(false)),
            0xC3 => Ok(BoltValue::Boolean(true)),
            0xC8 => {
                if buf.remaining() < 1 {
                    return Err(NetError::ProtocolError("Incomplete Int8".to_string()));
                }
                Ok(BoltValue::Integer(buf.get_i8() as i64))
            }
            0xC9 => {
                if buf.remaining() < 2 {
                    return Err(NetError::ProtocolError("Incomplete Int16".to_string()));
                }
                Ok(BoltValue::Integer(buf.get_i16() as i64))
            }
            0xCA => {
                if buf.remaining() < 4 {
                    return Err(NetError::ProtocolError("Incomplete Int32".to_string()));
                }
                Ok(BoltValue::Integer(buf.get_i32() as i64))
            }
            0xCB => {
                if buf.remaining() < 8 {
                    return Err(NetError::ProtocolError("Incomplete Int64".to_string()));
                }
                Ok(BoltValue::Integer(buf.get_i64()))
            }
            0xCC => {
                if buf.remaining() < 1 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Bytes8 length".to_string(),
                    ));
                }
                let len = buf.get_u8() as usize;
                Self::decode_bytes_payload(len, buf)
            }
            0xCD => {
                if buf.remaining() < 2 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Bytes16 length".to_string(),
                    ));
                }
                let len = buf.get_u16() as usize;
                Self::decode_bytes_payload(len, buf)
            }
            0xCE => {
                if buf.remaining() < 4 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Bytes32 length".to_string(),
                    ));
                }
                let len = buf.get_u32() as usize;
                Self::decode_bytes_payload(len, buf)
            }
            0xD0 => {
                if buf.remaining() < 1 {
                    return Err(NetError::ProtocolError(
                        "Incomplete String8 length".to_string(),
                    ));
                }
                let len = buf.get_u8() as usize;
                Self::decode_string_payload(len, buf)
            }
            0xD1 => {
                if buf.remaining() < 2 {
                    return Err(NetError::ProtocolError(
                        "Incomplete String16 length".to_string(),
                    ));
                }
                let len = buf.get_u16() as usize;
                Self::decode_string_payload(len, buf)
            }
            0xD2 => {
                if buf.remaining() < 4 {
                    return Err(NetError::ProtocolError(
                        "Incomplete String32 length".to_string(),
                    ));
                }
                let len = buf.get_u32() as usize;
                Self::decode_string_payload(len, buf)
            }
            0xD4 => {
                if buf.remaining() < 1 {
                    return Err(NetError::ProtocolError(
                        "Incomplete List8 length".to_string(),
                    ));
                }
                let len = buf.get_u8() as usize;
                Self::decode_list_payload(len, buf, depth)
            }
            0xD5 => {
                if buf.remaining() < 2 {
                    return Err(NetError::ProtocolError(
                        "Incomplete List16 length".to_string(),
                    ));
                }
                let len = buf.get_u16() as usize;
                Self::decode_list_payload(len, buf, depth)
            }
            0xD6 => {
                if buf.remaining() < 4 {
                    return Err(NetError::ProtocolError(
                        "Incomplete List32 length".to_string(),
                    ));
                }
                let len = buf.get_u32() as usize;
                Self::decode_list_payload(len, buf, depth)
            }
            0xD8 => {
                if buf.remaining() < 1 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Map8 length".to_string(),
                    ));
                }
                let len = buf.get_u8() as usize;
                Self::decode_map_payload(len, buf, depth)
            }
            0xD9 => {
                if buf.remaining() < 2 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Map16 length".to_string(),
                    ));
                }
                let len = buf.get_u16() as usize;
                Self::decode_map_payload(len, buf, depth)
            }
            0xDA => {
                if buf.remaining() < 4 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Map32 length".to_string(),
                    ));
                }
                let len = buf.get_u32() as usize;
                Self::decode_map_payload(len, buf, depth)
            }
            0xDC => {
                if buf.remaining() < 1 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Struct8 length".to_string(),
                    ));
                }
                let len = buf.get_u8() as usize;
                Self::decode_structure_payload(len, buf, depth)
            }
            0xDD => {
                if buf.remaining() < 2 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Struct16 length".to_string(),
                    ));
                }
                let len = buf.get_u16() as usize;
                Self::decode_structure_payload(len, buf, depth)
            }
            unknown => Err(NetError::ProtocolError(format!(
                "Unknown PackStream marker byte 0x{:02X}",
                unknown
            ))),
        }
    }

    // --- Encoders ---

    fn encode_integer(val: i64, buf: &mut BytesMut) {
        if (-16..=127).contains(&val) {
            buf.put_u8((val as i8) as u8);
        } else if (i8::MIN as i64..=i8::MAX as i64).contains(&val) {
            buf.put_u8(0xC8);
            buf.put_i8(val as i8);
        } else if (i16::MIN as i64..=i16::MAX as i64).contains(&val) {
            buf.put_u8(0xC9);
            buf.put_i16(val as i16);
        } else if (i32::MIN as i64..=i32::MAX as i64).contains(&val) {
            buf.put_u8(0xCA);
            buf.put_i32(val as i32);
        } else {
            buf.put_u8(0xCB);
            buf.put_i64(val);
        }
    }

    fn encode_bytes(bytes: &[u8], buf: &mut BytesMut) {
        let len = bytes.len();
        if len <= u8::MAX as usize {
            buf.put_u8(0xCC);
            buf.put_u8(len as u8);
        } else if len <= u16::MAX as usize {
            buf.put_u8(0xCD);
            buf.put_u16(len as u16);
        } else {
            buf.put_u8(0xCE);
            buf.put_u32(len as u32);
        }
        buf.put_slice(bytes);
    }

    fn encode_string(s: &str, buf: &mut BytesMut) {
        let len = s.len();
        if len <= 15 {
            buf.put_u8(0x80 | (len as u8));
        } else if len <= u8::MAX as usize {
            buf.put_u8(0xD0);
            buf.put_u8(len as u8);
        } else if len <= u16::MAX as usize {
            buf.put_u8(0xD1);
            buf.put_u16(len as u16);
        } else {
            buf.put_u8(0xD2);
            buf.put_u32(len as u32);
        }
        buf.put_slice(s.as_bytes());
    }

    fn encode_list(list: &[BoltValue], buf: &mut BytesMut) {
        let len = list.len();
        if len <= 15 {
            buf.put_u8(0x90 | (len as u8));
        } else if len <= u8::MAX as usize {
            buf.put_u8(0xD4);
            buf.put_u8(len as u8);
        } else if len <= u16::MAX as usize {
            buf.put_u8(0xD5);
            buf.put_u16(len as u16);
        } else {
            buf.put_u8(0xD6);
            buf.put_u32(len as u32);
        }
        for item in list {
            Self::encode(item, buf);
        }
    }

    fn encode_map(map: &HashMap<String, BoltValue>, buf: &mut BytesMut) {
        let len = map.len();
        if len <= 15 {
            buf.put_u8(0xA0 | (len as u8));
        } else if len <= u8::MAX as usize {
            buf.put_u8(0xD8);
            buf.put_u8(len as u8);
        } else if len <= u16::MAX as usize {
            buf.put_u8(0xD9);
            buf.put_u16(len as u16);
        } else {
            buf.put_u8(0xDA);
            buf.put_u32(len as u32);
        }
        for (k, v) in map {
            Self::encode_string(k, buf);
            Self::encode(v, buf);
        }
    }

    fn encode_structure(tag: u8, fields: &[BoltValue], buf: &mut BytesMut) {
        let len = fields.len();
        if len <= 15 {
            buf.put_u8(0xB0 | (len as u8));
        } else if len <= u8::MAX as usize {
            buf.put_u8(0xDC);
            buf.put_u8(len as u8);
        } else {
            buf.put_u8(0xDD);
            buf.put_u16(len as u16);
        }
        buf.put_u8(tag);
        for field in fields {
            Self::encode(field, buf);
        }
    }

    fn encode_node(node: &BoltNode, buf: &mut BytesMut) {
        let mut fields = vec![
            BoltValue::Integer(node.id),
            BoltValue::List(node.labels.iter().cloned().map(BoltValue::String).collect()),
            BoltValue::Map(node.properties.clone()),
        ];
        if let Some(ref elem_id) = node.element_id {
            fields.push(BoltValue::String(elem_id.clone()));
        }
        Self::encode_structure(0x4E, &fields, buf);
    }

    fn encode_relationship(rel: &BoltRelationship, buf: &mut BytesMut) {
        let mut fields = vec![
            BoltValue::Integer(rel.id),
            BoltValue::Integer(rel.start_node_id),
            BoltValue::Integer(rel.end_node_id),
            BoltValue::String(rel.rel_type.clone()),
            BoltValue::Map(rel.properties.clone()),
        ];
        if let Some(ref elem_id) = rel.element_id {
            fields.push(BoltValue::String(elem_id.clone()));
            fields.push(BoltValue::String(
                rel.start_element_id.clone().unwrap_or_default(),
            ));
            fields.push(BoltValue::String(
                rel.end_element_id.clone().unwrap_or_default(),
            ));
        }
        Self::encode_structure(0x52, &fields, buf);
    }

    fn encode_path(path: &BoltPath, buf: &mut BytesMut) {
        let nodes_val = BoltValue::List(path.nodes.iter().cloned().map(BoltValue::Node).collect());
        let rels_val = BoltValue::List(
            path.relationships
                .iter()
                .cloned()
                .map(|r| BoltValue::Structure {
                    tag: 0x72,
                    fields: {
                        let mut f = vec![
                            BoltValue::Integer(r.id),
                            BoltValue::String(r.rel_type),
                            BoltValue::Map(r.properties),
                        ];
                        if let Some(eid) = r.element_id {
                            f.push(BoltValue::String(eid));
                        }
                        f
                    },
                })
                .collect(),
        );
        let seq_val = BoltValue::List(
            path.sequence
                .iter()
                .map(|&i| BoltValue::Integer(i))
                .collect(),
        );
        Self::encode_structure(0x50, &[nodes_val, rels_val, seq_val], buf);
    }

    fn encode_point2d(point: &BoltPoint2D, buf: &mut BytesMut) {
        let fields = [
            BoltValue::Integer(point.srid),
            BoltValue::Float(point.x),
            BoltValue::Float(point.y),
        ];
        Self::encode_structure(0x58, &fields, buf);
    }

    fn encode_point3d(point: &BoltPoint3D, buf: &mut BytesMut) {
        let fields = [
            BoltValue::Integer(point.srid),
            BoltValue::Float(point.x),
            BoltValue::Float(point.y),
            BoltValue::Float(point.z),
        ];
        Self::encode_structure(0x59, &fields, buf);
    }

    fn encode_date(date: &BoltDate, buf: &mut BytesMut) {
        Self::encode_structure(0x44, &[BoltValue::Integer(date.days)], buf);
    }

    fn encode_time(time: &BoltTime, buf: &mut BytesMut) {
        let fields = [
            BoltValue::Integer(time.nanoseconds),
            BoltValue::Integer(time.tz_offset_seconds),
        ];
        Self::encode_structure(0x54, &fields, buf);
    }

    fn encode_local_time(ltime: &BoltLocalTime, buf: &mut BytesMut) {
        Self::encode_structure(0x74, &[BoltValue::Integer(ltime.nanoseconds)], buf);
    }

    fn encode_datetime(dt: &BoltDateTime, buf: &mut BytesMut) {
        let fields = [
            BoltValue::Integer(dt.seconds),
            BoltValue::Integer(dt.nanoseconds),
            BoltValue::Integer(dt.tz_offset_seconds),
        ];
        Self::encode_structure(0x49, &fields, buf);
    }

    fn encode_local_datetime(ldt: &BoltLocalDateTime, buf: &mut BytesMut) {
        let fields = [
            BoltValue::Integer(ldt.seconds),
            BoltValue::Integer(ldt.nanoseconds),
        ];
        Self::encode_structure(0x64, &fields, buf);
    }

    fn encode_datetime_zone_id(dtz: &BoltDateTimeZoneId, buf: &mut BytesMut) {
        let fields = [
            BoltValue::Integer(dtz.seconds),
            BoltValue::Integer(dtz.nanoseconds),
            BoltValue::String(dtz.zone_id.clone()),
        ];
        Self::encode_structure(0x4A, &fields, buf);
    }

    fn encode_duration(dur: &BoltDuration, buf: &mut BytesMut) {
        let fields = [
            BoltValue::Integer(dur.months),
            BoltValue::Integer(dur.days),
            BoltValue::Integer(dur.seconds),
            BoltValue::Integer(dur.nanoseconds),
        ];
        Self::encode_structure(0x45, &fields, buf);
    }

    // --- Decoders ---

    fn decode_string_payload(len: usize, buf: &mut Bytes) -> Result<BoltValue> {
        if buf.remaining() < len {
            return Err(NetError::ProtocolError(format!(
                "Unexpected EOF reading String of length {}",
                len
            )));
        }
        let bytes = buf.split_to(len);
        let s = std::str::from_utf8(&bytes)
            .map_err(|e| NetError::ProtocolError(format!("Invalid UTF-8 string: {}", e)))?
            .to_string();
        Ok(BoltValue::String(s))
    }

    fn decode_bytes_payload(len: usize, buf: &mut Bytes) -> Result<BoltValue> {
        if buf.remaining() < len {
            return Err(NetError::ProtocolError(format!(
                "Unexpected EOF reading byte buffer of length {}",
                len
            )));
        }
        let bytes = buf.split_to(len).to_vec();
        Ok(BoltValue::Bytes(bytes))
    }

    fn decode_list_payload(len: usize, buf: &mut Bytes, depth: usize) -> Result<BoltValue> {
        // In PackStream, each element requires at least 1 byte (the type marker byte).
        // If claimed element count exceeds remaining buffer bytes, the payload is physically impossible.
        // NOTE: When structured error codes land, this will map to ClaimedSizeExceedsBuffer.
        if len > buf.remaining() {
            return Err(NetError::ProtocolError(format!(
                "List length {} exceeds available buffer bytes {} (minimum 1 byte per element required)",
                len,
                buf.remaining()
            )));
        }
        let mut list = Vec::with_capacity(len.min(1024));
        for _ in 0..len {
            list.push(Self::decode_with_depth(buf, depth + 1)?);
        }
        Ok(BoltValue::List(list))
    }

    fn decode_map_payload(size: usize, buf: &mut Bytes, depth: usize) -> Result<BoltValue> {
        // In PackStream, each map entry consists of a key and a value, requiring at least 2 bytes.
        // If claimed map size requires more bytes than remaining in the buffer, it is physically impossible.
        // NOTE: When structured error codes land, this will map to ClaimedSizeExceedsBuffer.
        let min_required = size.saturating_mul(2);
        if min_required > buf.remaining() {
            return Err(NetError::ProtocolError(format!(
                "Map size {} requires at least {} bytes, which exceeds available buffer bytes {}",
                size,
                min_required,
                buf.remaining()
            )));
        }
        let mut map = HashMap::with_capacity(size.min(1024));
        for _ in 0..size {
            let key = match Self::decode_with_depth(buf, depth + 1)? {
                BoltValue::String(s) => s,
                other => {
                    return Err(NetError::ProtocolError(format!(
                        "Map key must be a String, got: {:?}",
                        other
                    )));
                }
            };
            let value = Self::decode_with_depth(buf, depth + 1)?;
            map.insert(key, value);
        }
        Ok(BoltValue::Map(map))
    }

    fn decode_structure_payload(
        num_fields: usize,
        buf: &mut Bytes,
        depth: usize,
    ) -> Result<BoltValue> {
        if !buf.has_remaining() {
            return Err(NetError::ProtocolError(
                "Unexpected EOF reading structure signature tag".to_string(),
            ));
        }
        let tag = buf.get_u8();
        // In PackStream, each field requires at least 1 byte (the type marker byte).
        if num_fields > buf.remaining() {
            return Err(NetError::ProtocolError(format!(
                "Structure field count {} exceeds available buffer bytes {} (minimum 1 byte per field required)",
                num_fields,
                buf.remaining()
            )));
        }
        let mut fields = Vec::with_capacity(num_fields.min(1024));
        for _ in 0..num_fields {
            fields.push(Self::decode_with_depth(buf, depth + 1)?);
        }

        // Try decoding into high-level graph structures if tag matches
        match tag {
            0x4E => Self::decode_node_structure(fields),
            0x52 => Self::decode_relationship_structure(fields),
            0x50 => Self::decode_path_structure(fields),
            0x58 => Self::decode_point2d_structure(fields),
            0x59 => Self::decode_point3d_structure(fields),
            0x44 => Self::decode_date_structure(fields),
            0x54 => Self::decode_time_structure(fields),
            0x74 => Self::decode_local_time_structure(fields),
            0x49 | 0x46 => Self::decode_datetime_structure(fields, tag),
            0x64 => Self::decode_local_datetime_structure(fields),
            0x4A | 0x66 | 0x69 => Self::decode_datetime_zone_id_structure(fields, tag),
            0x45 => Self::decode_duration_structure(fields),
            _ => Ok(BoltValue::Structure { tag, fields }),
        }
    }

    fn decode_node_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.len() < 3 {
            return Ok(BoltValue::Structure { tag: 0x4E, fields });
        }
        let id = match fields.first() {
            Some(BoltValue::Integer(i)) => *i,
            _ => return Ok(BoltValue::Structure { tag: 0x4E, fields }),
        };
        let labels = match fields.get(1) {
            Some(BoltValue::List(l)) => l
                .iter()
                .filter_map(|v| match v {
                    BoltValue::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let properties = match fields.get(2) {
            Some(BoltValue::Map(m)) => m.clone(),
            _ => HashMap::new(),
        };
        let element_id = if fields.len() >= 4 {
            match fields.get(3) {
                Some(BoltValue::String(s)) => Some(s.clone()),
                _ => None,
            }
        } else {
            None
        };

        Ok(BoltValue::Node(BoltNode {
            id,
            labels,
            properties,
            element_id,
        }))
    }

    fn decode_relationship_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.len() < 5 {
            return Ok(BoltValue::Structure { tag: 0x52, fields });
        }
        let id = match fields.first() {
            Some(BoltValue::Integer(i)) => *i,
            _ => return Ok(BoltValue::Structure { tag: 0x52, fields }),
        };
        let start_node_id = match fields.get(1) {
            Some(BoltValue::Integer(i)) => *i,
            _ => 0,
        };
        let end_node_id = match fields.get(2) {
            Some(BoltValue::Integer(i)) => *i,
            _ => 0,
        };
        let rel_type = match fields.get(3) {
            Some(BoltValue::String(s)) => s.clone(),
            _ => String::new(),
        };
        let properties = match fields.get(4) {
            Some(BoltValue::Map(m)) => m.clone(),
            _ => HashMap::new(),
        };
        let (element_id, start_element_id, end_element_id) = if fields.len() >= 8 {
            (
                fields
                    .get(5)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                fields
                    .get(6)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                fields
                    .get(7)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            )
        } else {
            (None, None, None)
        };

        Ok(BoltValue::Relationship(BoltRelationship {
            id,
            start_node_id,
            end_node_id,
            rel_type,
            properties,
            element_id,
            start_element_id,
            end_element_id,
        }))
    }

    fn decode_path_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.len() < 3 {
            return Ok(BoltValue::Structure { tag: 0x50, fields });
        }
        let nodes = match fields.first() {
            Some(BoltValue::List(l)) => l
                .iter()
                .filter_map(|v| match v {
                    BoltValue::Node(n) => Some(n.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let relationships = match fields.get(1) {
            Some(BoltValue::List(l)) => l
                .iter()
                .filter_map(|v| match v {
                    BoltValue::Structure { tag: 0x72, fields } => {
                        if fields.len() >= 3 {
                            let id = match fields.first() {
                                Some(BoltValue::Integer(i)) => *i,
                                _ => 0,
                            };
                            let rel_type = match fields.get(1) {
                                Some(BoltValue::String(s)) => s.clone(),
                                _ => String::new(),
                            };
                            let properties = match fields.get(2) {
                                Some(BoltValue::Map(m)) => m.clone(),
                                _ => HashMap::new(),
                            };
                            let element_id = fields
                                .get(3)
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            Some(BoltUnboundRelationship {
                                id,
                                rel_type,
                                properties,
                                element_id,
                            })
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let sequence = match fields.get(2) {
            Some(BoltValue::List(l)) => l
                .iter()
                .filter_map(|v| match v {
                    BoltValue::Integer(i) => Some(*i),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };

        Ok(BoltValue::Path(BoltPath {
            nodes,
            relationships,
            sequence,
        }))
    }

    fn decode_point2d_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.len() < 3 {
            return Ok(BoltValue::Structure { tag: 0x58, fields });
        }
        let srid = match fields.first() {
            Some(BoltValue::Integer(i)) => *i,
            _ => return Ok(BoltValue::Structure { tag: 0x58, fields }),
        };
        let x = match fields.get(1) {
            Some(BoltValue::Float(f)) => *f,
            Some(BoltValue::Integer(i)) => *i as f64,
            _ => return Ok(BoltValue::Structure { tag: 0x58, fields }),
        };
        let y = match fields.get(2) {
            Some(BoltValue::Float(f)) => *f,
            Some(BoltValue::Integer(i)) => *i as f64,
            _ => return Ok(BoltValue::Structure { tag: 0x58, fields }),
        };
        Ok(BoltValue::Point2D(BoltPoint2D { srid, x, y }))
    }

    fn decode_point3d_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.len() < 4 {
            return Ok(BoltValue::Structure { tag: 0x59, fields });
        }
        let srid = match fields.first() {
            Some(BoltValue::Integer(i)) => *i,
            _ => return Ok(BoltValue::Structure { tag: 0x59, fields }),
        };
        let x = match fields.get(1) {
            Some(BoltValue::Float(f)) => *f,
            Some(BoltValue::Integer(i)) => *i as f64,
            _ => return Ok(BoltValue::Structure { tag: 0x59, fields }),
        };
        let y = match fields.get(2) {
            Some(BoltValue::Float(f)) => *f,
            Some(BoltValue::Integer(i)) => *i as f64,
            _ => return Ok(BoltValue::Structure { tag: 0x59, fields }),
        };
        let z = match fields.get(3) {
            Some(BoltValue::Float(f)) => *f,
            Some(BoltValue::Integer(i)) => *i as f64,
            _ => return Ok(BoltValue::Structure { tag: 0x59, fields }),
        };
        Ok(BoltValue::Point3D(BoltPoint3D { srid, x, y, z }))
    }

    fn decode_date_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.is_empty() {
            return Ok(BoltValue::Structure { tag: 0x44, fields });
        }
        let days = match fields.first() {
            Some(BoltValue::Integer(d)) => *d,
            _ => return Ok(BoltValue::Structure { tag: 0x44, fields }),
        };
        Ok(BoltValue::Date(BoltDate { days }))
    }

    fn decode_time_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.len() < 2 {
            return Ok(BoltValue::Structure { tag: 0x54, fields });
        }
        let nanoseconds = match fields.first() {
            Some(BoltValue::Integer(n)) => *n,
            _ => return Ok(BoltValue::Structure { tag: 0x54, fields }),
        };
        let tz_offset_seconds = match fields.get(1) {
            Some(BoltValue::Integer(tz)) => *tz,
            _ => return Ok(BoltValue::Structure { tag: 0x54, fields }),
        };
        Ok(BoltValue::Time(BoltTime {
            nanoseconds,
            tz_offset_seconds,
        }))
    }

    fn decode_local_time_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.is_empty() {
            return Ok(BoltValue::Structure { tag: 0x74, fields });
        }
        let nanoseconds = match fields.first() {
            Some(BoltValue::Integer(n)) => *n,
            _ => return Ok(BoltValue::Structure { tag: 0x74, fields }),
        };
        Ok(BoltValue::LocalTime(BoltLocalTime { nanoseconds }))
    }

    fn decode_datetime_structure(fields: Vec<BoltValue>, tag: u8) -> Result<BoltValue> {
        if fields.len() < 3 {
            return Ok(BoltValue::Structure { tag, fields });
        }
        let seconds = match fields.first() {
            Some(BoltValue::Integer(s)) => *s,
            _ => return Ok(BoltValue::Structure { tag, fields }),
        };
        let nanoseconds = match fields.get(1) {
            Some(BoltValue::Integer(n)) => *n,
            _ => return Ok(BoltValue::Structure { tag, fields }),
        };
        let tz_offset_seconds = match fields.get(2) {
            Some(BoltValue::Integer(tz)) => *tz,
            _ => return Ok(BoltValue::Structure { tag, fields }),
        };
        Ok(BoltValue::DateTime(BoltDateTime {
            seconds,
            nanoseconds,
            tz_offset_seconds,
        }))
    }

    fn decode_local_datetime_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.len() < 2 {
            return Ok(BoltValue::Structure { tag: 0x64, fields });
        }
        let seconds = match fields.first() {
            Some(BoltValue::Integer(s)) => *s,
            _ => return Ok(BoltValue::Structure { tag: 0x64, fields }),
        };
        let nanoseconds = match fields.get(1) {
            Some(BoltValue::Integer(n)) => *n,
            _ => return Ok(BoltValue::Structure { tag: 0x64, fields }),
        };
        Ok(BoltValue::LocalDateTime(BoltLocalDateTime {
            seconds,
            nanoseconds,
        }))
    }

    fn decode_datetime_zone_id_structure(fields: Vec<BoltValue>, tag: u8) -> Result<BoltValue> {
        if fields.len() < 3 {
            return Ok(BoltValue::Structure { tag, fields });
        }
        let seconds = match fields.first() {
            Some(BoltValue::Integer(s)) => *s,
            _ => return Ok(BoltValue::Structure { tag, fields }),
        };
        let nanoseconds = match fields.get(1) {
            Some(BoltValue::Integer(n)) => *n,
            _ => return Ok(BoltValue::Structure { tag, fields }),
        };
        let zone_id = match fields.get(2) {
            Some(BoltValue::String(z)) => z.clone(),
            _ => return Ok(BoltValue::Structure { tag, fields }),
        };
        Ok(BoltValue::DateTimeZoneId(BoltDateTimeZoneId {
            seconds,
            nanoseconds,
            zone_id,
        }))
    }

    fn decode_duration_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.len() < 4 {
            return Ok(BoltValue::Structure { tag: 0x45, fields });
        }
        let months = match fields.first() {
            Some(BoltValue::Integer(m)) => *m,
            _ => return Ok(BoltValue::Structure { tag: 0x45, fields }),
        };
        let days = match fields.get(1) {
            Some(BoltValue::Integer(d)) => *d,
            _ => return Ok(BoltValue::Structure { tag: 0x45, fields }),
        };
        let seconds = match fields.get(2) {
            Some(BoltValue::Integer(s)) => *s,
            _ => return Ok(BoltValue::Structure { tag: 0x45, fields }),
        };
        let nanoseconds = match fields.get(3) {
            Some(BoltValue::Integer(n)) => *n,
            _ => return Ok(BoltValue::Structure { tag: 0x45, fields }),
        };
        Ok(BoltValue::Duration(BoltDuration {
            months,
            days,
            seconds,
            nanoseconds,
        }))
    }
}

impl BoltValue {
    /// Returns the string slice if this is a `BoltValue::String`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Returns the integer if this is a `BoltValue::Integer`.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Returns the boolean if this is a `BoltValue::Boolean`.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns the map if this is a `BoltValue::Map`.
    pub fn as_map(&self) -> Option<&HashMap<String, BoltValue>> {
        match self {
            Self::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Returns the list if this is a `BoltValue::List`.
    pub fn as_list(&self) -> Option<&[BoltValue]> {
        match self {
            Self::List(l) => Some(l.as_slice()),
            _ => None,
        }
    }

    /// Returns the date if this is a `BoltValue::Date`.
    pub fn as_date(&self) -> Option<&BoltDate> {
        match self {
            Self::Date(d) => Some(d),
            _ => None,
        }
    }

    /// Returns the time if this is a `BoltValue::Time`.
    pub fn as_time(&self) -> Option<&BoltTime> {
        match self {
            Self::Time(t) => Some(t),
            _ => None,
        }
    }

    /// Returns the local time if this is a `BoltValue::LocalTime`.
    pub fn as_local_time(&self) -> Option<&BoltLocalTime> {
        match self {
            Self::LocalTime(lt) => Some(lt),
            _ => None,
        }
    }

    /// Returns the datetime if this is a `BoltValue::DateTime`.
    pub fn as_datetime(&self) -> Option<&BoltDateTime> {
        match self {
            Self::DateTime(dt) => Some(dt),
            _ => None,
        }
    }

    /// Returns the local datetime if this is a `BoltValue::LocalDateTime`.
    pub fn as_local_datetime(&self) -> Option<&BoltLocalDateTime> {
        match self {
            Self::LocalDateTime(ldt) => Some(ldt),
            _ => None,
        }
    }

    /// Returns the datetime with zone id if this is a `BoltValue::DateTimeZoneId`.
    pub fn as_datetime_zone_id(&self) -> Option<&BoltDateTimeZoneId> {
        match self {
            Self::DateTimeZoneId(dtz) => Some(dtz),
            _ => None,
        }
    }

    /// Returns the duration if this is a `BoltValue::Duration`.
    pub fn as_duration(&self) -> Option<&BoltDuration> {
        match self {
            Self::Duration(dur) => Some(dur),
            _ => None,
        }
    }
}
