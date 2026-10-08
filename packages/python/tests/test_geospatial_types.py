"""Comprehensive tests for Voyager OGM geospatial Point types and spatial distance functions."""

import math

import pytest
from voyager_ogm import (
    Field,
    MockBridge,
    Node,
    Point,
    Query,
    Session,
    fn,
)
from voyager_ogm.types import (
    CARTESIAN_2D,
    CARTESIAN_3D,
    WGS_84_2D,
    WGS_84_3D,
    distance,
)


class Place(Node):
    """Test node entity containing spatial Point fields."""

    name: str = Field(index=True)
    location: Point = Field(...)


def test_point_constructor_wgs84_2d():
    # Construct using latitude and longitude
    p = Point(latitude=52.52, longitude=13.405)
    assert p.srid == WGS_84_2D
    assert p.crs == "wgs-84"
    assert not p.is_3d
    assert p.is_geographic
    assert p.latitude == 52.52
    assert p.longitude == 13.405
    assert p.x == 13.405
    assert p.y == 52.52
    assert p.z is None
    assert p.height is None


def test_point_constructor_wgs84_3d():
    p = Point(latitude=52.52, longitude=13.405, height=100.5)
    assert p.srid == WGS_84_3D
    assert p.crs == "wgs-84-3d"
    assert p.is_3d
    assert p.is_geographic
    assert p.latitude == 52.52
    assert p.longitude == 13.405
    assert p.height == 100.5
    assert p.x == 13.405
    assert p.y == 52.52
    assert p.z == 100.5


def test_point_constructor_cartesian_2d_and_3d():
    p2 = Point(x=10.0, y=20.0)
    assert p2.srid == CARTESIAN_2D
    assert p2.crs == "cartesian"
    assert not p2.is_3d
    assert not p2.is_geographic
    assert p2.x == 10.0
    assert p2.y == 20.0
    assert p2.latitude is None
    assert p2.longitude is None

    p3 = Point(x=1.0, y=2.0, z=3.0)
    assert p3.srid == CARTESIAN_3D
    assert p3.crs == "cartesian-3d"
    assert p3.is_3d
    assert not p3.is_geographic
    assert p3.x == 1.0
    assert p3.y == 2.0
    assert p3.z == 3.0


def test_point_constructor_positional():
    p2 = Point(10.0, 20.0)
    assert p2.x == 10.0
    assert p2.y == 20.0
    assert p2.srid == CARTESIAN_2D

    p3 = Point(1.0, 2.0, 3.0)
    assert p3.x == 1.0
    assert p3.y == 2.0
    assert p3.z == 3.0
    assert p3.srid == CARTESIAN_3D


def test_point_validation_bounds():
    # Latitude out of bounds
    with pytest.raises(ValueError, match="Invalid latitude"):
        Point(latitude=91.0, longitude=0.0)

    with pytest.raises(ValueError, match="Invalid latitude"):
        Point(latitude=-91.0, longitude=0.0)

    # Longitude out of bounds
    with pytest.raises(ValueError, match="Invalid longitude"):
        Point(latitude=0.0, longitude=181.0)

    with pytest.raises(ValueError, match="Invalid longitude"):
        Point(latitude=0.0, longitude=-181.0)


def test_in_memory_distance_wgs84():
    # Berlin to Paris (~878 km)
    berlin = Point(latitude=52.5200, longitude=13.4050)
    paris = Point(latitude=48.8566, longitude=2.3522)

    dist = berlin.distance(paris)
    assert isinstance(dist, float)
    # Distance between Berlin and Paris is ~878,000 meters (+/- 5000m)
    assert 870_000 < dist < 885_000

    # Self-distance is zero
    assert berlin.distance(berlin) == 0.0

    # Module-level distance()
    assert distance(berlin, paris) == dist


def test_in_memory_distance_cartesian():
    p1 = Point(x=0.0, y=0.0)
    p2 = Point(x=3.0, y=4.0)
    assert math.isclose(p1.distance(p2), 5.0)

    p3 = Point(x=0.0, y=0.0, z=0.0)
    p4 = Point(x=1.0, y=2.0, z=2.0)
    assert math.isclose(p3.distance(p4), 3.0)


def test_in_memory_distance_incompatible_crs():
    geo = Point(latitude=52.0, longitude=13.0)
    cart = Point(x=52.0, y=13.0)
    with pytest.raises(ValueError, match="incompatible"):
        geo.distance(cart)


def test_point_dict_serialization_and_deserialization():
    orig_geo = Point(latitude=52.52, longitude=13.405)
    d_geo = orig_geo.to_dict()
    assert d_geo["srid"] == WGS_84_2D
    assert d_geo["latitude"] == 52.52
    assert d_geo["longitude"] == 13.405

    recovered_geo = Point.from_dict(d_geo)
    assert orig_geo == recovered_geo

    # Bolt wire dictionary format (srid, x, y)
    bolt_dict = {"srid": 4326, "x": 13.405, "y": 52.52}
    from_bolt = Point.from_dict(bolt_dict)
    assert from_bolt.latitude == 52.52
    assert from_bolt.longitude == 13.405
    assert from_bolt.srid == 4326


def test_pyarrow_and_polars_conversions():
    p = Point(latitude=52.52, longitude=13.405)

    # PyArrow
    arrow_val = p.to_arrow()
    p_from_arrow = Point.from_arrow(arrow_val)
    assert p == p_from_arrow

    # Polars
    polars_val = p.to_polars()
    p_from_polars = Point.from_polars(polars_val)
    assert p == p_from_polars


def test_model_definition_and_automatic_hydration():
    # Initialized with dict (simulating database record deserialization)
    p1 = Place(name="Berlin HQ", location={"latitude": 52.52, "longitude": 13.405})
    loc1 = p1.get("location")
    assert isinstance(loc1, Point)
    assert loc1.latitude == 52.52
    assert loc1.longitude == 13.405

    # Initialized with Point object
    target = Point(latitude=48.8566, longitude=2.3522)
    p2 = Place(name="Paris Office", location=target)
    assert p2.get("location") is target

    # Attribute mutation with dict
    p1.location = {"srid": 4326, "x": 2.3522, "y": 48.8566}
    loc_updated = p1.get("location")
    assert isinstance(loc_updated, Point)
    assert loc_updated.latitude == 48.8566


def test_query_spatial_distance_cross_dialect():
    p = Place(alias="p")
    target_point = Point(latitude=52.52, longitude=13.405)

    # 1. Query using BoundField.distance_to(other) < threshold
    q = Query.match(p).where(p.location.distance_to(target_point) < 5000.0).return_(p.name)

    # Cypher emission
    compiled_cypher = q.compile("cypher")
    assert "point.distance(p.location, point(" in compiled_cypher.statement
    assert "<" in compiled_cypher.statement

    # ISO GQL emission
    compiled_gql = q.compile("iso_gql")
    assert "point.distance(p.location, point(" in compiled_gql.statement
    assert "<" in compiled_gql.statement

    # SQL/PGQ emission
    compiled_pgq = q.compile("sql_pgq")
    assert "ST_Distance(p.location, point(" in compiled_pgq.statement


def test_query_spatial_fn_helpers():
    p = Place(alias="p")
    target_point = Point(latitude=52.52, longitude=13.405)

    # 2. Query using fn.point.distance(...)
    q_fn_point = (
        Query.match(p).where(fn.point.distance(p.location, target_point) < 10000.0).return_(p.name)
    )
    cypher_fn = q_fn_point.compile("cypher")
    assert "point.distance(p.location, point(" in cypher_fn.statement

    # 3. Query using fn.distance(...)
    q_fn_dist = (
        Query.match(p).where(fn.distance(p.location, target_point) < 10000.0).return_(p.name)
    )
    cypher_dist = q_fn_dist.compile("cypher")
    assert "point.distance(p.location, point(" in cypher_dist.statement

    # 4. Query using fn.point(latitude=..., longitude=...)
    q_point_call = Query.match(p).return_(fn.point(latitude=52.52, longitude=13.405).as_("pt"))
    cypher_pt = q_point_call.compile("cypher")
    assert "point(" in cypher_pt.statement
    assert "AS pt" in cypher_pt.statement


def test_point_write_path_mock_bridge_roundtrip():
    """Verify write path: Point properties are serialized to Cypher dict maps on the wire."""
    bridge = MockBridge()
    session = Session(bridge=bridge)

    # 1. Entity active record .save() with Point
    tower = Place(name="Eiffel Tower", location=Point(latitude=48.8584, longitude=2.2945))
    tower.save(session=session, key_field="name")

    assert len(bridge.executed_queries) == 1
    stmt, params = bridge.executed_queries[0]
    assert "UNWIND $batch AS row" in stmt
    assert ":Place {name: row.name})" in stmt
    assert "batch" in params
    batch_records = params["batch"]
    assert len(batch_records) == 1
    record = batch_records[0]
    assert record["name"] == "Eiffel Tower"
    assert record["location"] == {"latitude": 48.8584, "longitude": 2.2945}

    # 2. Direct session.bulk_upsert()
    bridge.executed_queries.clear()
    plan = session.bulk_upsert(
        Place,
        [{"name": "Louvre", "location": Point(latitude=48.8606, longitude=2.3376)}],
        key_field="name",
    )
    session.run_bulk(plan)
    assert len(bridge.executed_queries) == 1
    stmt2, params2 = bridge.executed_queries[0]
    assert params2["batch"][0]["location"] == {"latitude": 48.8606, "longitude": 2.3376}

    # 3. Direct session.execute() parameter passing
    bridge.executed_queries.clear()
    session.execute(
        "CREATE (p:Place {name: $name, loc: $loc})",
        {"name": "Notre-Dame", "loc": Point(latitude=48.8530, longitude=2.3499)},
    )
    assert len(bridge.executed_queries) == 1
    _, params3 = bridge.executed_queries[0]
    assert params3["loc"] == {"latitude": 48.8530, "longitude": 2.3499}


def test_point_distance_strict_in_memory_type_error():
    """Verify Point.distance() strictly requires a Point instance and raises TypeError for AST expressions."""
    p = Place(alias="p")
    target = Point(latitude=52.52, longitude=13.405)

    with pytest.raises(TypeError, match="Point.distance\\(\\) calculates in-memory distance"):
        target.distance(p.location)  # type: ignore[arg-type]

    with pytest.raises(TypeError, match="Point.distance\\(\\) calculates in-memory distance"):
        target.distance("invalid")  # type: ignore[arg-type]

    with pytest.raises(TypeError, match="distance\\(\\) calculates in-memory distance"):
        distance(target, p.location)


def test_point_eq_dict_coercion_and_hash_tolerance():
    """Verify Point == dict coercion ergonomics and hash consistency across rounding boundaries."""
    p = Point(latitude=52.52, longitude=13.405)

    # Point == dict coercion
    assert p == {"latitude": 52.52, "longitude": 13.405}
    assert {"latitude": 52.52, "longitude": 13.405} == p
    assert p == {"srid": 4326, "x": 13.405, "y": 52.52}
    assert p != {"latitude": 10.0, "longitude": 20.0}
    assert p != {"non_point_dict": True}
    assert p != "non_point_string"

    # Hash consistency for points within floating-point tolerance
    p1 = Point(latitude=52.52000001, longitude=13.40500001)
    p2 = Point(latitude=52.52000004, longitude=13.40500004)
    assert p1 == p2
    assert hash(p1) == hash(p2)


def test_unknown_srid_rejected():
    """Verify unknown or unhandled SRIDs (e.g. 3857) raise ValueError with informative message."""
    with pytest.raises(
        ValueError, match="Unsupported spatial reference system identifier \\(SRID\\): 3857"
    ):
        Point(x=100.0, y=200.0, srid=3857)


def test_mixed_2d_3d_distance():
    """Verify mixed 2D and 3D distance ignores height and computes 2D surface distance."""
    p2d = Point(latitude=52.52, longitude=13.405)
    p3d = Point(latitude=48.8566, longitude=2.3522, height=500.0)
    p3d_flat = Point(latitude=48.8566, longitude=2.3522)

    dist_mixed = p2d.distance(p3d)
    dist_flat = p2d.distance(p3d_flat)
    assert math.isclose(dist_mixed, dist_flat)


def test_point_from_spatial_and_driver_interop():
    """Verify Point.from_spatial handles Voyager Point, dicts, duck-typed objects, and Neo4j spatial points."""
    p_orig = Point(latitude=52.52, longitude=13.405)

    # 1. Self return
    assert Point.from_spatial(p_orig) is p_orig

    # 2. Dict delegation
    p_from_dict = Point.from_spatial({"latitude": 52.52, "longitude": 13.405})
    assert p_from_dict == p_orig

    # 3. Duck-typed spatial object
    class CustomSpatial:
        def __init__(self, srid: int, x: float, y: float, z: float | None = None) -> None:
            self.srid = srid
            self.x = x
            self.y = y
            self.z = z

    custom_geo = CustomSpatial(4326, 13.405, 52.52)
    p_from_custom = Point.from_spatial(custom_geo)
    assert p_from_custom == p_orig
    assert p_from_custom.is_geographic

    custom_cart = CustomSpatial(7203, 10.0, 20.0)
    p_cart = Point.from_spatial(custom_cart)
    assert p_cart.srid == 7203
    assert p_cart.x == 10.0 and p_cart.y == 20.0

    # 4. Neo4j spatial objects if neo4j package is installed
    try:
        from neo4j.spatial import CartesianPoint, WGS84Point

        neo_wgs = WGS84Point((13.405, 52.52))
        p_from_neo = Point.from_spatial(neo_wgs)
        assert p_from_neo == p_orig
        assert p_orig == neo_wgs

        neo_wgs_3d = WGS84Point((13.405, 52.52, 100.0))
        p_from_neo_3d = Point.from_spatial(neo_wgs_3d)
        assert p_from_neo_3d.is_3d
        assert p_from_neo_3d.height == 100.0

        neo_cart = CartesianPoint((10.0, 20.0))
        p_from_cart = Point.from_spatial(neo_cart)
        assert p_from_cart == Point(x=10.0, y=20.0)
        assert p_from_cart == neo_cart

        # Test hydration in Node model
        tower = Place(name="Berlin Tower", location=neo_wgs)
        assert isinstance(tower.get("location"), Point)
        assert tower.get("location") == p_orig
    except ImportError:
        pass

    # 5. Invalid input raises TypeError
    with pytest.raises(TypeError, match="Cannot convert"):
        Point.from_spatial(12345)
