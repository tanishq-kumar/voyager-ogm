"""Comprehensive tests for Voyager OGM geospatial Point types and spatial distance functions."""

import math

import pytest
from voyager_ogm import (
    Field,
    Node,
    Point,
    Query,
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
