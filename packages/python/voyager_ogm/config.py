"""Global configuration and lifecycle settings for Voyager OGM."""

from __future__ import annotations

import os
from dataclasses import dataclass


@dataclass
class VoyagerConfig:
    """Global configuration settings for Voyager OGM."""

    default_dialect: str = "cypher"
    optimize: bool = False
    optimization_level: str = "standard"
    enable_identity_map: bool = False

    @classmethod
    def from_env(cls) -> VoyagerConfig:
        """Initializes configuration resolving defaults from environment variables."""
        cfg = cls()

        # VOYAGER_DIALECT
        if env_dialect := os.getenv("VOYAGER_DIALECT"):
            cfg.default_dialect = env_dialect.lower()

        # VOYAGER_OPTIMIZE
        if env_opt := os.getenv("VOYAGER_OPTIMIZE"):
            opt_str = env_opt.strip().lower()
            if opt_str in ("1", "true", "yes", "on"):
                cfg.optimize = True
                cfg.optimization_level = "standard"
            elif opt_str in ("0", "false", "no", "off", "none"):
                cfg.optimize = False
                cfg.optimization_level = "none"
            elif opt_str in ("standard", "aggressive"):
                cfg.optimize = True
                cfg.optimization_level = opt_str

        # VOYAGER_IDENTITY_MAP
        if env_id_map := os.getenv("VOYAGER_IDENTITY_MAP"):
            cfg.enable_identity_map = env_id_map.strip().lower() in ("1", "true", "yes", "on")

        return cfg


_GLOBAL_CONFIG: VoyagerConfig = VoyagerConfig.from_env()


def get_config() -> VoyagerConfig:
    """Returns the active global Voyager configuration."""
    return _GLOBAL_CONFIG


def configure(
    *,
    default_dialect: str | None = None,
    optimize: bool | str | None = None,
    optimization_level: str | None = None,
    enable_identity_map: bool | None = None,
) -> VoyagerConfig:
    """Configures global Voyager OGM lifecycle settings.

    Args:
        default_dialect: Default dialect ('cypher', 'iso_gql', 'sql_pgq').
        optimize: Enable/disable AST optimization, or specify level ('standard', 'aggressive', 'none', True, False).
        optimization_level: Explicit optimization level ('standard', 'aggressive', 'none').
        enable_identity_map: Enable/disable global Identity Map default.

    Returns:
        The updated VoyagerConfig instance.
    """
    global _GLOBAL_CONFIG

    if default_dialect is not None:
        _GLOBAL_CONFIG.default_dialect = default_dialect.lower()

    if isinstance(optimize, str):
        opt_str = optimize.strip().lower()
        if opt_str in ("none", "off", "false", "0"):
            _GLOBAL_CONFIG.optimize = False
            _GLOBAL_CONFIG.optimization_level = "none"
        else:
            _GLOBAL_CONFIG.optimize = True
            _GLOBAL_CONFIG.optimization_level = opt_str
    elif isinstance(optimize, bool):
        _GLOBAL_CONFIG.optimize = optimize
        if optimization_level:
            _GLOBAL_CONFIG.optimization_level = optimization_level

    if optimization_level is not None and not isinstance(optimize, str):
        _GLOBAL_CONFIG.optimization_level = optimization_level

    if enable_identity_map is not None:
        _GLOBAL_CONFIG.enable_identity_map = enable_identity_map

    return _GLOBAL_CONFIG


def reset_config() -> VoyagerConfig:
    """Resets the global configuration to defaults (re-reading environment variables)."""
    global _GLOBAL_CONFIG
    _GLOBAL_CONFIG = VoyagerConfig.from_env()
    return _GLOBAL_CONFIG
