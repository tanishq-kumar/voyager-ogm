"""Voyager OGM Geospatial Types & Spatial Distance Engine.

Provides spatial Point data types (2D and 3D geographic WGS-84 and Cartesian)
with coordinate validation, in-memory distance calculations (Haversine and Euclidean),
AST expression builder integration, and PyArrow/Polars conversions.
"""

from __future__ import annotations

import math
from typing import TYPE_CHECKING, Any

from voyager_ogm.expressions import (
    Expression,
    FunctionExpr,
    LiteralExpr,
    to_expression,
)

try:
    from voyager_ogm._voyager_rs import AstExpr

    HAS_VOYAGER_RS = True
except ImportError:
    AstExpr = None  # type: ignore[assignment,misc]
    HAS_VOYAGER_RS = False

if TYPE_CHECKING:
    import polars as pl
    import pyarrow as pa

# Standard Spatial Reference System Identifiers (SRID)
WGS_84_2D: int = 4326
WGS_84_3D: int = 4979
CARTESIAN_2D: int = 7203
CARTESIAN_3D: int = 9157

SUPPORTED_SRIDS: frozenset[int] = frozenset({WGS_84_2D, WGS_84_3D, CARTESIAN_2D, CARTESIAN_3D})

# Neo4j and openCypher standard mean Earth radius in meters
EARTH_RADIUS_METERS: float = 6371008.8

# CRS Name to SRID Mapping
_CRS_TO_SRID_2D: dict[str, int] = {
    "wgs-84": WGS_84_2D,
    "wgs84": WGS_84_2D,
    "epsg:4326": WGS_84_2D,
    "cartesian": CARTESIAN_2D,
    "cartesian-2d": CARTESIAN_2D,
    "epsg:7203": CARTESIAN_2D,
}

_CRS_TO_SRID_3D: dict[str, int] = {
    "wgs-84-3d": WGS_84_3D,
    "wgs84-3d": WGS_84_3D,
    "wgs-84": WGS_84_3D,
    "wgs84": WGS_84_3D,
    "epsg:4979": WGS_84_3D,
    "cartesian-3d": CARTESIAN_3D,
    "cartesian": CARTESIAN_3D,
    "epsg:9157": CARTESIAN_3D,
}

_SRID_TO_CRS: dict[int, str] = {
    WGS_84_2D: "wgs-84",
    WGS_84_3D: "wgs-84-3d",
    CARTESIAN_2D: "cartesian",
    CARTESIAN_3D: "cartesian-3d",
}


def _haversine_distance(
    lat1: float, lon1: float, lat2: float, lon2: float, radius: float = EARTH_RADIUS_METERS
) -> float:
    """Calculates spherical distance on Earth using the Haversine formula."""
    phi1 = math.radians(lat1)
    phi2 = math.radians(lat2)
    dphi = math.radians(lat2 - lat1)
    dlam = math.radians(lon2 - lon1)

    a = (math.sin(dphi / 2.0) ** 2) + math.cos(phi1) * math.cos(phi2) * (math.sin(dlam / 2.0) ** 2)
    # Clamp a within [0, 1] to prevent domain errors with floating-point drift
    a = min(1.0, max(0.0, a))
    c = 2.0 * math.atan2(math.sqrt(a), math.sqrt(max(0.0, 1.0 - a)))
    return radius * c


class Point(Expression):
    """Geospatial Point representation across Bolt, Cypher, ISO GQL, and SQL/PGQ.

    Supports both geographic WGS-84 coordinates (latitude, longitude, height) and
    Cartesian coordinates (x, y, z) in 2D and 3D.

    Attributes:
        srid: Spatial Reference System Identifier (e.g. 4326, 4979, 7203, 9157).
        crs: Coordinate Reference System string identifier (e.g. 'wgs-84', 'cartesian').
        x: Longitude for geographic points or X-coordinate for Cartesian points.
        y: Latitude for geographic points or Y-coordinate for Cartesian points.
        z: Height for geographic points or Z-coordinate for Cartesian points (or None if 2D).
        latitude: Latitude for geographic points (or None if Cartesian).
        longitude: Longitude for geographic points (or None if Cartesian).
        height: Height in meters above ellipsoid for 3D geographic points (or None).
        is_3d: Whether the point has three dimensions.
    """

    def __init__(
        self,
        *coords: float,
        latitude: float | None = None,
        longitude: float | None = None,
        height: float | None = None,
        x: float | None = None,
        y: float | None = None,
        z: float | None = None,
        srid: int | None = None,
        crs: str | None = None,
    ) -> None:
        """Initializes a geospatial Point.

        Args:
            *coords: Optional positional coordinates (x, y) or (x, y, z).
            latitude: Latitude in degrees (-90 to 90) for WGS-84 points.
            longitude: Longitude in degrees (-180 to 180) for WGS-84 points.
            height: Height in meters for 3D WGS-84 points.
            x: X-coordinate (or longitude) for Cartesian/geographic points.
            y: Y-coordinate (or latitude) for Cartesian/geographic points.
            z: Z-coordinate (or height) for 3D Cartesian/geographic points.
            srid: Explicit SRID (4326, 4979, 7203, 9157).
            crs: Explicit CRS name ('wgs-84', 'wgs-84-3d', 'cartesian', 'cartesian-3d').
        """
        # Handle positional coordinates
        if coords:
            if len(coords) == 2:
                if x is None:
                    x = coords[0]
                if y is None:
                    y = coords[1]
            elif len(coords) == 3:
                if x is None:
                    x = coords[0]
                if y is None:
                    y = coords[1]
                if z is None:
                    z = coords[2]
            else:
                raise ValueError(
                    f"Point accepts either 2 or 3 positional coordinates, got {len(coords)}"
                )

        # Normalize crs string
        norm_crs = crs.strip().lower() if isinstance(crs, str) else None

        # Determine if 3D from coordinates
        has_3d = (height is not None) or (z is not None)

        # Determine SRID
        resolved_srid: int
        if srid is not None:
            resolved_srid = int(srid)
            if resolved_srid not in SUPPORTED_SRIDS:
                raise ValueError(
                    f"Unsupported spatial reference system identifier (SRID): {resolved_srid}. "
                    f"Supported SRIDs are: {sorted(SUPPORTED_SRIDS)} "
                    f"({WGS_84_2D} for WGS-84 2D, {WGS_84_3D} for WGS-84 3D, "
                    f"{CARTESIAN_2D} for Cartesian 2D, {CARTESIAN_3D} for Cartesian 3D)."
                )
        elif norm_crs is not None:
            if has_3d and norm_crs in _CRS_TO_SRID_3D:
                resolved_srid = _CRS_TO_SRID_3D[norm_crs]
            elif not has_3d and norm_crs in _CRS_TO_SRID_2D:
                resolved_srid = _CRS_TO_SRID_2D[norm_crs]
            elif norm_crs in _CRS_TO_SRID_3D:
                resolved_srid = _CRS_TO_SRID_3D[norm_crs]
            elif norm_crs in _CRS_TO_SRID_2D:
                resolved_srid = _CRS_TO_SRID_2D[norm_crs]
            else:
                raise ValueError(f"Unrecognized coordinate reference system (CRS): '{crs}'")
        elif latitude is not None or longitude is not None or height is not None:
            resolved_srid = WGS_84_3D if has_3d else WGS_84_2D
        else:
            resolved_srid = CARTESIAN_3D if has_3d else CARTESIAN_2D

        is_geo = resolved_srid in (WGS_84_2D, WGS_84_3D)
        is_3d_point = resolved_srid in (WGS_84_3D, CARTESIAN_3D)

        # Coordinate resolution
        res_x: float
        res_y: float
        res_z: float | None = None

        if is_geo:
            # For WGS-84: x is longitude, y is latitude
            if longitude is not None:
                res_x = float(longitude)
            elif x is not None:
                res_x = float(x)
            else:
                raise ValueError("Geographic WGS-84 Point requires 'longitude' or 'x'")

            if latitude is not None:
                res_y = float(latitude)
            elif y is not None:
                res_y = float(y)
            else:
                raise ValueError("Geographic WGS-84 Point requires 'latitude' or 'y'")

            if is_3d_point:
                if height is not None:
                    res_z = float(height)
                elif z is not None:
                    res_z = float(z)
                else:
                    res_z = 0.0

            # Validate coordinate bounds for WGS-84
            if res_y < -90.0 or res_y > 90.0:
                raise ValueError(
                    f"Invalid latitude {res_y}: must be between -90.0 and 90.0 degrees"
                )
            if res_x < -180.0 or res_x > 180.0:
                raise ValueError(
                    f"Invalid longitude {res_x}: must be between -180.0 and 180.0 degrees"
                )
        else:
            # Cartesian
            if x is not None:
                res_x = float(x)
            elif longitude is not None:
                res_x = float(longitude)
            else:
                raise ValueError("Cartesian Point requires 'x'")

            if y is not None:
                res_y = float(y)
            elif latitude is not None:
                res_y = float(latitude)
            else:
                raise ValueError("Cartesian Point requires 'y'")

            if is_3d_point:
                if z is not None:
                    res_z = float(z)
                elif height is not None:
                    res_z = float(height)
                else:
                    res_z = 0.0

        self._srid = resolved_srid
        self._x = res_x
        self._y = res_y
        self._z = res_z
        self._is_3d = is_3d_point
        self._is_geo = is_geo
        self._crs = _SRID_TO_CRS.get(
            resolved_srid,
            "wgs-84-3d"
            if (is_geo and is_3d_point)
            else "wgs-84"
            if is_geo
            else "cartesian-3d"
            if is_3d_point
            else "cartesian",
        )

        if HAS_VOYAGER_RS and AstExpr is not None:
            try:
                self._native_expr = AstExpr.function(
                    "point", [AstExpr.literal(self.to_cypher_dict())]
                )
            except Exception:
                self._native_expr = None

    @property
    def srid(self) -> int:
        """Spatial Reference System Identifier."""
        return self._srid

    @property
    def crs(self) -> str:
        """Coordinate Reference System name."""
        return self._crs

    @property
    def is_3d(self) -> bool:
        """True if this point is three-dimensional."""
        return self._is_3d

    @property
    def is_geographic(self) -> bool:
        """True if this point uses WGS-84 geographic coordinates."""
        return self._is_geo

    @property
    def x(self) -> float:
        """X coordinate (longitude for WGS-84)."""
        return self._x

    @property
    def y(self) -> float:
        """Y coordinate (latitude for WGS-84)."""
        return self._y

    @property
    def z(self) -> float | None:
        """Z coordinate (height for WGS-84, or None if 2D)."""
        return self._z

    @property
    def latitude(self) -> float | None:
        """Latitude in degrees for WGS-84 points (None for Cartesian)."""
        return self._y if self._is_geo else None

    @property
    def longitude(self) -> float | None:
        """Longitude in degrees for WGS-84 points (None for Cartesian)."""
        return self._x if self._is_geo else None

    @property
    def height(self) -> float | None:
        """Height in meters for 3D WGS-84 points (None if 2D or Cartesian)."""
        return self._z if (self._is_geo and self._is_3d) else None

    def distance(self, other: Point) -> float:
        """Calculates in-memory distance to another Point.

        Uses the Haversine formula (Earth radius 6,371,008.8m) for WGS-84 geographic points,
        and Euclidean distance for Cartesian points. Strictly in-memory calculation.
        For AST query expression building, use `distance_to(other)`.

        Note:
            When calculating distance between mixed 2D and 3D points, the height (or z coordinate)
            is ignored and 2D surface distance is computed.

        Args:
            other: Another Point instance.

        Returns:
            Distance in meters (for WGS-84) or coordinate units (for Cartesian) as a float.

        Raises:
            TypeError: If other is not a Point.
            ValueError: If coordinate reference systems are incompatible.
        """
        if not isinstance(other, Point):
            raise TypeError(
                f"Point.distance() calculates in-memory distance and requires another Point instance, "
                f"got {type(other).__name__}. For query expressions, use Point.distance_to()."
            )

        if self._is_geo != other._is_geo:
            raise ValueError(
                f"Cannot calculate distance between incompatible CRSs: {self.crs} (SRID {self.srid}) "
                f"and {other.crs} (SRID {other.srid})"
            )

        if self._is_geo:
            assert self.latitude is not None and self.longitude is not None
            assert other.latitude is not None and other.longitude is not None
            d = _haversine_distance(self.latitude, self.longitude, other.latitude, other.longitude)
            if (
                self._is_3d
                and other._is_3d
                and self.height is not None
                and other.height is not None
            ):
                dh = other.height - self.height
                return math.sqrt(d * d + dh * dh)
            return d
        else:
            dx = other.x - self.x
            dy = other.y - self.y
            if self._is_3d and other._is_3d and self.z is not None and other.z is not None:
                dz = other.z - self.z
                return math.sqrt(dx * dx + dy * dy + dz * dz)
            return math.sqrt(dx * dx + dy * dy)

    def distance_to(self, other: Any) -> FunctionExpr:
        """Creates a spatial distance AST expression `point.distance(self, other)` for query building.

        Args:
            other: A Point, BoundField, or Expression.

        Returns:
            FunctionExpr representing the spatial distance calculation.
        """
        return FunctionExpr("point.distance", [self, to_expression(other)])

    def to_cypher_dict(self) -> dict[str, Any]:
        """Returns the dictionary representation recognized by openCypher `point({...})`."""
        if self._is_geo:
            d: dict[str, Any] = {
                "latitude": self._y,
                "longitude": self._x,
            }
            if self._is_3d and self._z is not None:
                d["height"] = self._z
            return d
        else:
            d = {
                "x": self._x,
                "y": self._y,
            }
            if self._is_3d and self._z is not None:
                d["z"] = self._z
            return d

    def to_dict(self) -> dict[str, Any]:
        """Serializes this Point to a standard dictionary."""
        d: dict[str, Any] = {
            "srid": self._srid,
            "crs": self._crs,
            "x": self._x,
            "y": self._y,
        }
        if self._is_3d:
            d["z"] = self._z
        if self._is_geo:
            d["latitude"] = self._y
            d["longitude"] = self._x
            if self._is_3d:
                d["height"] = self._z
        return d

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> Point:
        """Constructs a Point from a dictionary representation.

        Supports Bolt wire dictionaries (`srid`, `x`, `y`, `z`), geographic dictionaries
        (`latitude`, `longitude`, `height`), and Cartesian dictionaries (`x`, `y`, `z`).
        """
        srid = data.get("srid")
        crs = data.get("crs")

        lat = data.get("latitude")
        lon = data.get("longitude")
        height = data.get("height")

        x = data.get("x")
        y = data.get("y")
        z = data.get("z")

        return cls(
            latitude=lat,
            longitude=lon,
            height=height,
            x=x,
            y=y,
            z=z,
            srid=srid,
            crs=crs,
        )

    def to_arrow(self) -> pa.Scalar | dict[str, Any]:
        """Converts this Point into a PyArrow struct scalar (or dict fallback)."""
        try:
            import pyarrow as pa

            fields = [
                ("srid", pa.int64()),
                ("x", pa.float64()),
                ("y", pa.float64()),
            ]
            if self._is_3d:
                fields.append(("z", pa.float64()))
            if self._is_geo:
                fields.append(("latitude", pa.float64()))
                fields.append(("longitude", pa.float64()))
                if self._is_3d:
                    fields.append(("height", pa.float64()))
            struct_type = pa.struct(fields)
            return pa.scalar(self.to_dict(), type=struct_type)
        except ImportError:
            return self.to_dict()

    @classmethod
    def from_arrow(cls, scalar: Any) -> Point:
        """Constructs a Point from a PyArrow struct scalar."""
        if hasattr(scalar, "as_py"):
            d = scalar.as_py()
            if isinstance(d, dict):
                return cls.from_dict(d)
        if isinstance(scalar, dict):
            return cls.from_dict(scalar)
        raise TypeError(f"Cannot convert {type(scalar).__name__} to Point")

    def to_polars(self) -> pl.Series | dict[str, Any]:
        """Converts this Point into a Polars Series (struct representation)."""
        try:
            import polars as pl

            return pl.Series("point", [self.to_dict()])
        except ImportError:
            return self.to_dict()

    @classmethod
    def from_polars(cls, data: Any) -> Point:
        """Constructs a Point from a Polars Series, DataFrame, or struct element."""
        if hasattr(data, "to_list"):
            items = data.to_list()
            if items and isinstance(items[0], dict):
                return cls.from_dict(items[0])
        if hasattr(data, "to_dicts"):
            dicts = data.to_dicts()
            if dicts:
                return cls.from_dict(dicts[0])
        if isinstance(data, dict):
            return cls.from_dict(data)
        raise TypeError(f"Cannot convert {type(data).__name__} to Point")

    def to_spec(self) -> tuple[str, str, list[Any]]:
        """Converts this Point into an AST function specification tuple."""
        return ("fn", "point", [LiteralExpr(self.to_cypher_dict()).to_spec()])

    def __eq__(self, other: Any) -> Any:  # type: ignore[override]
        if isinstance(other, Point):
            if self._srid != other._srid:
                return False
            coords_match = math.isclose(self._x, other._x, abs_tol=1e-7) and math.isclose(
                self._y, other._y, abs_tol=1e-7
            )
            if not coords_match:
                return False
            if self._is_3d != other._is_3d:
                return False
            if self._is_3d:
                return math.isclose(self._z or 0.0, other._z or 0.0, abs_tol=1e-7)
            return True
        if isinstance(other, dict):
            try:
                coerced = Point.from_dict(other)
                return self == coerced
            except Exception:
                return False
        if isinstance(other, Expression):
            return super().__eq__(other)
        return False

    def __ne__(self, other: Any) -> Any:  # type: ignore[override]
        if isinstance(other, (Point, dict)):
            eq_res = self.__eq__(other)
            if isinstance(eq_res, bool):
                return not eq_res
        if isinstance(other, Expression):
            return super().__ne__(other)
        return True

    def __hash__(self) -> int:
        return hash(
            (
                self._srid,
                round(self._x, 5),
                round(self._y, 5),
                round(self._z, 5) if self._z is not None else None,
            )
        )

    def __repr__(self) -> str:
        if self._is_geo:
            if self._is_3d:
                return f"Point(latitude={self.latitude}, longitude={self.longitude}, height={self.height})"
            return f"Point(latitude={self.latitude}, longitude={self.longitude})"
        else:
            if self._is_3d:
                return f"Point(x={self.x}, y={self.y}, z={self.z}, crs='{self.crs}')"
            return f"Point(x={self.x}, y={self.y}, crs='{self.crs}')"


def distance(point1: Any, point2: Any) -> float:
    """Calculates in-memory spatial distance between two Point instances.

    Strictly in-memory calculation. For AST query expressions, use `fn.point.distance()`
    or `Point.distance_to()`.

    Args:
        point1: First Point instance.
        point2: Second Point instance.

    Returns:
        Float distance in meters (WGS-84) or coordinate units (Cartesian).

    Raises:
        TypeError: If either argument is not a Point instance.
    """
    if not (isinstance(point1, Point) and isinstance(point2, Point)):
        raise TypeError(
            "distance() calculates in-memory distance between two Point instances. "
            "For query expressions, use fn.point.distance() or Point.distance_to()."
        )
    return point1.distance(point2)


def serialize_param_value(val: Any) -> Any:
    """Recursively serializes rich types (such as Point) to database-compatible parameter values.

    Maps `Point` objects to their Cypher/Bolt map dictionary (`to_cypher_dict()`).
    """
    if hasattr(val, "to_cypher_dict"):
        return val.to_cypher_dict()
    if isinstance(val, dict):
        return {k: serialize_param_value(v) for k, v in val.items()}
    if isinstance(val, (list, tuple)):
        return [serialize_param_value(item) for item in val]
    return val
