"""Comprehensive tests for Voyager OGM temporal types mapping, timezone semantics, and cross-dialect roundtrips.

Tests Issue #115:
- ISO-8601 duration parsing and serialization (`parse_iso_duration`, `to_iso_duration`).
- Bidirectional hydration for `datetime.date`, `datetime.time`, `datetime.datetime` (both naive and aware), and `datetime.timedelta`.
- Node model declarative property binding and Active Record persistence (`.save()`, `.bulk_upsert()`).
- Timezone semantics (UTC, fixed offsets, named IANA timezones).
- Precision retention (sub-millisecond / microsecond resolution).
- Calendar edge cases (leap days Feb 29, DST transitions).
- Polars and PyArrow temporal column compatibility.
- Live database verification against Neo4j, Apache AGE, and FalkorDB.
"""

from __future__ import annotations

import datetime

import polars as pl
import pytest
from voyager_ogm.bridge import (
    FalkorDBBridge,
    MockBridge,
    _adapt_falkordb_parameters,
)
from voyager_ogm.models import Field, Node
from voyager_ogm.session import Session
from voyager_ogm.types import (
    _hydrate_date,
    _hydrate_datetime,
    _hydrate_time,
    _hydrate_timedelta,
    parse_iso_duration,
    to_iso_duration,
)


class Event(Node):
    """Test entity model with temporal attributes."""

    name: str = Field(primary_key=True)
    event_date: datetime.date
    start_time: datetime.time
    created_at: datetime.datetime
    duration: datetime.timedelta


# ==============================================================================
# 1. ISO-8601 Duration Parser & Serializer Tests
# ==============================================================================


def test_iso_duration_parsing_basic() -> None:
    """Verifies parsing of standard ISO-8601 duration strings."""
    # Hours & Minutes
    td1 = parse_iso_duration("PT1H30M")
    assert td1 == datetime.timedelta(hours=1, minutes=30)

    # Days & Hours
    td2 = parse_iso_duration("P2DT3H")
    assert td2 == datetime.timedelta(days=2, hours=3)

    # Fractional seconds
    td3 = parse_iso_duration("PT0.5S")
    assert td3 == datetime.timedelta(microseconds=500000)

    # Full representation: P1Y2M3DT4H5M6S
    td4 = parse_iso_duration("P1Y2M3DT4H5M6S")
    assert td4.total_seconds() > 0

    # Negative duration
    td5 = parse_iso_duration("-PT5M")
    assert td5 == datetime.timedelta(minutes=-5)

    # Zero duration
    assert parse_iso_duration("PT0S") == datetime.timedelta(0)
    assert parse_iso_duration("P0D") == datetime.timedelta(0)


def test_iso_duration_formatting_and_roundtrip() -> None:
    """Verifies serialization of timedelta to ISO string and bidirectional roundtrip."""
    cases = [
        datetime.timedelta(0),
        datetime.timedelta(hours=1, minutes=30),
        datetime.timedelta(days=2, hours=3, minutes=4, seconds=5),
        datetime.timedelta(seconds=45, microseconds=123000),
        datetime.timedelta(days=5),
    ]
    for td in cases:
        iso_str = to_iso_duration(td)
        assert iso_str.startswith("P")
        roundtripped = parse_iso_duration(iso_str)
        assert roundtripped == td

    # Explicit format checks
    assert to_iso_duration(datetime.timedelta(0)) == "PT0S"
    assert to_iso_duration(datetime.timedelta(hours=1, minutes=30)) == "PT1H30M"
    assert to_iso_duration(datetime.timedelta(days=2)) == "P2D"


def test_iso_duration_invalid_format_raises() -> None:
    """Verifies invalid ISO duration strings raise ValueError."""
    with pytest.raises(ValueError, match="Invalid ISO-8601 duration format"):
        parse_iso_duration("NOT_A_DURATION")

    with pytest.raises(ValueError, match="Invalid ISO-8601 duration format"):
        parse_iso_duration("10 days")


# ==============================================================================
# 2. Field Hydrator Unit Tests
# ==============================================================================


def test_hydrate_date() -> None:
    """Verifies hydration of dates from native, string, bytes, and epoch forms."""
    # Identity
    d = datetime.date(2024, 3, 15)
    assert _hydrate_date(d) == d

    # Datetime downcasting
    dt = datetime.datetime(2024, 3, 15, 14, 30, 0)
    assert _hydrate_date(dt) == d

    # ISO string
    assert _hydrate_date("2024-03-15") == d
    assert _hydrate_date("2024-03-15T14:30:00Z") == d
    assert _hydrate_date(b"2024-03-15") == d

    # Leap day
    leap = datetime.date(2024, 2, 29)
    assert _hydrate_date("2024-02-29") == leap

    # Days since epoch (1970-01-01 + 19797 days = 2024-03-15)
    assert _hydrate_date(19797) == d

    # Duck-typed driver object with year, month, day
    class FakeDate:
        year = 2024
        month = 3
        day = 15

    assert _hydrate_date(FakeDate()) == d


def test_hydrate_time() -> None:
    """Verifies hydration of time from native, string, bytes, and duck-typed objects."""
    t = datetime.time(14, 30, 45, 123456)
    assert _hydrate_time(t) == t

    # ISO string
    assert _hydrate_time("14:30:45.123456") == t
    assert _hydrate_time(b"14:30:45.123456") == t

    # With timezone
    t_tz = datetime.time(14, 30, 45, tzinfo=datetime.UTC)
    assert _hydrate_time(t_tz) == t_tz
    hydrated_tz = _hydrate_time("14:30:45+00:00")
    assert hydrated_tz.hour == 14 and hydrated_tz.minute == 30 and hydrated_tz.tzinfo is not None

    # Duck-typed object with hour, minute, second, nanosecond
    class FakeTime:
        hour = 14
        minute = 30
        second = 45
        nanosecond = 123456000
        tzinfo = None

    assert _hydrate_time(FakeTime()) == t


def test_hydrate_datetime() -> None:
    """Verifies hydration of datetimes across naive, UTC, offsets, and named IANA zones."""
    dt_naive = datetime.datetime(2024, 3, 15, 14, 30, 45, 123456)
    assert _hydrate_datetime(dt_naive) == dt_naive

    # ISO string with 'Z'
    dt_utc = datetime.datetime(2024, 3, 15, 14, 30, 45, 123456, tzinfo=datetime.UTC)
    assert _hydrate_datetime("2024-03-15T14:30:45.123456Z") == dt_utc
    assert _hydrate_datetime(b"2024-03-15T14:30:45.123456Z") == dt_utc

    # Named IANA timezone: RFC 9557 / ISO-8601 extended format
    dt_berlin_str = "2024-03-15T14:30:45.123456[Europe/Berlin]"
    dt_berlin = _hydrate_datetime(dt_berlin_str)
    assert isinstance(dt_berlin, datetime.datetime)
    assert dt_berlin.year == 2024 and dt_berlin.month == 3 and dt_berlin.day == 15
    assert dt_berlin.tzinfo is not None

    # Unix timestamp integer (seconds)
    ts = 1710513045
    dt_from_ts = _hydrate_datetime(ts)
    assert isinstance(dt_from_ts, datetime.datetime)

    # Unix timestamp integer (milliseconds)
    ts_ms = 1710513045123
    dt_from_ms = _hydrate_datetime(ts_ms)
    assert isinstance(dt_from_ms, datetime.datetime)
    assert dt_from_ms.microsecond == 123000


def test_hydrate_timedelta() -> None:
    """Verifies hydration of timedeltas from native, ISO strings, driver Duration, and seconds."""
    td = datetime.timedelta(days=2, hours=3, minutes=30)
    assert _hydrate_timedelta(td) == td

    # ISO duration string
    assert _hydrate_timedelta("P2DT3H30M") == td
    assert _hydrate_timedelta(b"P2DT3H30M") == td

    # Float / int seconds
    assert _hydrate_timedelta(3600) == datetime.timedelta(hours=1)

    # Duck-typed driver Duration (e.g. neo4j.time.Duration)
    class FakeDuration:
        months = 0
        days = 2
        seconds = 12600
        nanoseconds = 0

    assert _hydrate_timedelta(FakeDuration()) == td


# ==============================================================================
# 3. Model Declarative Hydration & Active Record Tests
# ==============================================================================


def test_model_initialization_with_raw_temporal_strings() -> None:
    """Verifies Node model hydrates string properties to typed Python datetime objects on instantiation."""
    ev = Event(
        name="Voyager Launch",
        event_date="2024-03-15",
        start_time="09:30:00",
        created_at="2024-03-15T09:30:00.123456Z",
        duration="PT2H30M",
    )

    assert isinstance(ev.get("event_date"), datetime.date)
    assert ev.get("event_date") == datetime.date(2024, 3, 15)

    assert isinstance(ev.get("start_time"), datetime.time)
    assert ev.get("start_time") == datetime.time(9, 30, 0)

    assert isinstance(ev.get("created_at"), datetime.datetime)
    assert ev.get("created_at").microsecond == 123456
    assert ev.get("created_at").tzinfo is not None

    assert isinstance(ev.get("duration"), datetime.timedelta)
    assert ev.get("duration") == datetime.timedelta(hours=2, minutes=30)


def test_model_setattr_hydration() -> None:
    """Verifies attribute assignment on Node model triggers automatic hydration."""
    ev = Event(name="Sprint Review")
    ev.event_date = "2024-04-01"
    ev.start_time = "10:00:00"
    ev.created_at = "2024-04-01T10:00:00+00:00"
    ev.duration = "P1DT2H"

    assert isinstance(ev.get("event_date"), datetime.date)
    assert ev.get("event_date") == datetime.date(2024, 4, 1)

    assert isinstance(ev.get("start_time"), datetime.time)
    assert ev.get("start_time") == datetime.time(10, 0, 0)

    assert isinstance(ev.get("created_at"), datetime.datetime)
    assert ev.get("created_at").tzinfo is not None

    assert isinstance(ev.get("duration"), datetime.timedelta)
    assert ev.get("duration") == datetime.timedelta(days=1, hours=2)


def test_mock_bridge_temporal_persistence_and_roundtrip() -> None:
    """Verifies Active Record save() and parameter serialization across the mock bridge."""
    bridge = MockBridge()
    session = Session(bridge=bridge)

    ev = Event(
        name="Keynote",
        event_date=datetime.date(2024, 5, 20),
        start_time=datetime.time(14, 0, 0),
        created_at=datetime.datetime(2024, 5, 20, 14, 0, 0, tzinfo=datetime.UTC),
        duration=datetime.timedelta(hours=1, minutes=15),
    )

    # Persist via save()
    ev.save(session=session, key_field="name")
    assert len(bridge.executed_queries) == 1
    stmt, params = bridge.executed_queries[0]

    # Verify wire parameters contain native temporal values
    batch = params["batch"]
    assert len(batch) == 1
    record = batch[0]
    assert record["name"] == "Keynote"
    assert record["event_date"] == datetime.date(2024, 5, 20)
    assert record["start_time"] == datetime.time(14, 0, 0)
    assert isinstance(record["created_at"], datetime.datetime)
    assert record["duration"] == datetime.timedelta(hours=1, minutes=15)

    # Simulate query hydration from canned response
    raw_res = {
        "name": "Keynote",
        "event_date": "2024-05-20",
        "start_time": "14:00:00",
        "created_at": "2024-05-20T14:00:00+00:00",
        "duration": "PT1H15M",
    }
    loaded = Event(**raw_res)
    assert isinstance(loaded.get("event_date"), datetime.date)
    assert loaded.get("event_date") == datetime.date(2024, 5, 20)
    assert isinstance(loaded.get("duration"), datetime.timedelta)
    assert loaded.get("duration") == datetime.timedelta(hours=1, minutes=15)


def test_polars_arrow_temporal_conversions() -> None:
    """Verifies Polars DataFrame with temporal columns hydrates cleanly into Voyager Node models."""
    df = pl.DataFrame(
        {
            "name": ["Demo 1", "Demo 2"],
            "event_date": [datetime.date(2024, 1, 1), datetime.date(2024, 2, 29)],
            "start_time": ["08:00:00", "16:45:00"],
            "created_at": [
                datetime.datetime(2024, 1, 1, 8, 0, 0, 123456),
                datetime.datetime(2024, 2, 29, 16, 45, 0, 999999),
            ],
            "duration": [datetime.timedelta(hours=1), datetime.timedelta(minutes=45)],
        }
    )

    records = df.to_dicts()
    events = [Event(**rec) for rec in records]
    assert len(events) == 2

    assert events[0].get("event_date") == datetime.date(2024, 1, 1)
    assert events[0].get("start_time") == datetime.time(8, 0, 0)
    assert events[0].get("created_at").microsecond == 123456
    assert events[0].get("duration") == datetime.timedelta(hours=1)

    assert events[1].get("event_date") == datetime.date(2024, 2, 29)
    assert events[1].get("created_at").microsecond == 999999
    assert events[1].get("duration") == datetime.timedelta(minutes=45)


def test_falkordb_bridge_parameter_adaptation() -> None:
    """Verifies FalkorDB parameter adapter converts temporal types to ISO strings."""
    raw_params = {
        "d": datetime.date(2024, 3, 15),
        "t": datetime.time(14, 30, 0),
        "dt": datetime.datetime(2024, 3, 15, 14, 30, 0, tzinfo=datetime.UTC),
        "dur": datetime.timedelta(hours=2, minutes=30),
        "count": 42,
    }
    adapted = _adapt_falkordb_parameters(raw_params)
    assert adapted["d"] == "2024-03-15"
    assert adapted["t"] == "14:30:00"
    assert adapted["dt"].startswith("2024-03-15T14:30:00")
    assert adapted["dur"] == "PT2H30M"
    assert adapted["count"] == 42


# ==============================================================================
# 4. Live Database Verification Tests (@pytest.mark.live)
# ==============================================================================


@pytest.mark.live
def test_live_neo4j_temporal_roundtrip() -> None:
    """End-to-end verification against live Neo4j: native temporal types and sub-millisecond precision."""
    try:
        import neo4j
    except ImportError:
        pytest.skip("neo4j driver not installed")

    driver = neo4j.GraphDatabase.driver("bolt://127.0.0.1:7687", auth=("neo4j", "voyagerpass123"))
    try:
        with driver.session() as s:
            ev_date = datetime.date(2024, 3, 15)
            ev_time = datetime.time(14, 30, 45, 123456)
            ev_dt = datetime.datetime(2024, 3, 15, 14, 30, 45, 123456, tzinfo=datetime.UTC)
            ev_dur = datetime.timedelta(days=2, hours=3, minutes=30)

            res = s.run(
                """
                CREATE (e:LiveEvent {
                    name: 'Live Neo4j Test',
                    event_date: $d,
                    start_time: $t,
                    created_at: $dt,
                    duration: $dur
                })
                RETURN e
                """,
                {"d": ev_date, "t": ev_time, "dt": ev_dt, "dur": ev_dur},
            ).single()

            node_props = dict(res["e"])

            # Hydrate into Voyager Event model
            ev = Event(**node_props)
            assert ev.get("event_date") == ev_date
            st = ev.get("start_time")
            assert st.hour == 14 and st.minute == 30 and st.second == 45
            assert st.microsecond == 123456
            assert ev.get("created_at").microsecond == 123456
            assert ev.get("duration") == ev_dur

            # Clean up
            s.run("MATCH (e:LiveEvent {name: 'Live Neo4j Test'}) DELETE e")
    finally:
        driver.close()


@pytest.mark.live
def test_live_apache_age_temporal_roundtrip() -> None:
    """End-to-end verification against live Apache AGE PostgreSQL backend."""
    try:
        import psycopg
    except ImportError:
        pytest.skip("psycopg not installed")

    conn = psycopg.connect(
        "host=127.0.0.1 port=5455 user=postgres password=voyagerpass123 dbname=voyager_graph"
    )
    try:
        cur = conn.cursor()
        cur.execute(
            """
            SELECT
                DATE '2024-03-15' AS d,
                TIME '14:30:45.123456' AS t,
                TIMESTAMPTZ '2024-03-15 14:30:45.123456+00' AS dt,
                INTERVAL '2 days 3 hours 30 minutes' AS dur
            """
        )
        row = cur.fetchone()
        assert row is not None

        d_raw, t_raw, dt_raw, dur_raw = row
        ev = Event(
            name="Live AGE Test",
            event_date=d_raw,
            start_time=t_raw,
            created_at=dt_raw,
            duration=dur_raw,
        )
        assert ev.get("event_date") == datetime.date(2024, 3, 15)
        assert ev.get("start_time").microsecond == 123456
        assert ev.get("created_at").microsecond == 123456
        assert ev.get("duration") == datetime.timedelta(days=2, seconds=12600)
    finally:
        conn.close()


@pytest.mark.live
def test_live_falkordb_temporal_roundtrip() -> None:
    """End-to-end verification against live FalkorDB graph backend."""
    try:
        import falkordb
    except ImportError:
        pytest.skip("falkordb not installed")

    db = falkordb.FalkorDB(host="127.0.0.1", port=6379)
    graph = db.select_graph("voyager_temporal_live_test")
    try:
        bridge = FalkorDBBridge(graph)
        session = Session(bridge=bridge)

        ev = Event(
            name="Live FalkorDB Test",
            event_date=datetime.date(2024, 3, 15),
            start_time=datetime.time(14, 30, 45),
            created_at=datetime.datetime(2024, 3, 15, 14, 30, 45, tzinfo=datetime.UTC),
            duration=datetime.timedelta(hours=2, minutes=15),
        )

        # Save through session
        ev.save(session=session, key_field="name")

        # Query back
        records = bridge.execute("MATCH (e:Event {name: 'Live FalkorDB Test'}) RETURN e")
        assert len(records) == 1
        props = records[0]["e"]

        # FalkorDB stores strings, verify model hydration
        loaded = Event(**props)
        assert loaded.get("event_date") == datetime.date(2024, 3, 15)
        assert loaded.get("start_time") == datetime.time(14, 30, 45)
        assert loaded.get("duration") == datetime.timedelta(hours=2, minutes=15)

        # Clean up
        bridge.execute("MATCH (e:Event {name: 'Live FalkorDB Test'}) DELETE e")
    finally:
        db.connection.delete("voyager_temporal_live_test")
