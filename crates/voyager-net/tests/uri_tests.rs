//! Comprehensive unit and regression tests for voyager-net URI parsing.

use voyager_net::config::{Auth, TlsMode};
use voyager_net::uri::{DatabaseProtocol, ParsedUri};

#[test]
fn test_parse_bolt_standard_uri() {
    let uri = ParsedUri::parse("bolt://localhost:7687").expect("Failed to parse bolt URI");
    assert_eq!(uri.protocol, DatabaseProtocol::Bolt);
    assert_eq!(uri.host, "localhost");
    assert_eq!(uri.port, Some(7687));
    assert_eq!(uri.auth, Auth::None);
    assert_eq!(uri.tls, TlsMode::Disabled);
    assert_eq!(uri.database, None);
    assert_eq!(uri.socket_addr(), "localhost:7687");
}

#[test]
fn test_parse_bolt_with_auth_and_database() {
    let uri = ParsedUri::parse("neo4j://neo4j:secretPassword123@graph.prod.internal:7687/movies")
        .expect("Failed to parse neo4j URI");
    assert_eq!(uri.protocol, DatabaseProtocol::Bolt);
    assert_eq!(uri.host, "graph.prod.internal");
    assert_eq!(uri.port, Some(7687));
    assert_eq!(
        uri.auth,
        Auth::Basic {
            username: "neo4j".to_string(),
            password: "secretPassword123".to_string(),
            realm: None,
        }
    );
    assert_eq!(uri.database, Some("movies".to_string()));
    assert_eq!(uri.tls, TlsMode::Disabled);
}

#[test]
fn test_parse_bolt_tls_modes() {
    let uri_s = ParsedUri::parse("bolt+s://db.cloud.com").expect("Failed to parse bolt+s URI");
    assert_eq!(uri_s.protocol, DatabaseProtocol::Bolt);
    assert_eq!(uri_s.port, Some(7687)); // Default port fallback
    assert_eq!(uri_s.tls, TlsMode::Required);

    let uri_ssc =
        ParsedUri::parse("neo4j+ssc://localhost:7687").expect("Failed to parse neo4j+ssc URI");
    assert_eq!(uri_ssc.tls, TlsMode::SelfSigned);
}

#[test]
fn test_parse_memgraph_uri() {
    let uri = ParsedUri::parse("memgraph://localhost:7687").expect("Failed to parse memgraph URI");
    assert_eq!(uri.protocol, DatabaseProtocol::Bolt);
    assert_eq!(uri.host, "localhost");
    assert_eq!(uri.port, Some(7687));
    assert_eq!(uri.tls, TlsMode::Disabled);

    let uri_s =
        ParsedUri::parse("memgraph+s://mg.prod.com:7687").expect("Failed to parse memgraph+s URI");
    assert_eq!(uri_s.protocol, DatabaseProtocol::Bolt);
    assert_eq!(uri_s.tls, TlsMode::Required);
}

#[test]
fn test_parse_postgresql_uri() {
    let uri =
        ParsedUri::parse("postgresql://postgres:mysecret@127.0.0.1:5432/age_graph?sslmode=require")
            .expect("Failed to parse postgresql URI");
    assert_eq!(uri.protocol, DatabaseProtocol::Postgres);
    assert_eq!(uri.host, "127.0.0.1");
    assert_eq!(uri.port, Some(5432));
    assert_eq!(
        uri.auth,
        Auth::Basic {
            username: "postgres".to_string(),
            password: "mysecret".to_string(),
            realm: None,
        }
    );
    assert_eq!(uri.database, Some("age_graph".to_string()));
    assert_eq!(uri.tls, TlsMode::Required);
}

#[test]
fn test_parse_age_uri() {
    let uri = ParsedUri::parse("age://postgres:mysecret@localhost:5455/demo_graph")
        .expect("Failed to parse age URI");
    assert_eq!(uri.protocol, DatabaseProtocol::Postgres);
    assert_eq!(uri.host, "localhost");
    assert_eq!(uri.port, Some(5455));
    assert_eq!(uri.database, Some("demo_graph".to_string()));
}

#[test]
fn test_parse_redis_and_falkordb_uri() {
    let uri = ParsedUri::parse("falkordb://:redispass@localhost:6379?graph=social_network")
        .expect("Failed to parse falkordb URI");
    assert_eq!(uri.protocol, DatabaseProtocol::Redis);
    assert_eq!(uri.host, "localhost");
    assert_eq!(uri.port, Some(6379));
    assert_eq!(
        uri.auth,
        Auth::Basic {
            username: "".to_string(),
            password: "redispass".to_string(),
            realm: None,
        }
    );
    assert_eq!(uri.database, Some("social_network".to_string()));
}

#[test]
fn test_parse_rediss_tls_uri() {
    let uri = ParsedUri::parse("rediss://redis.cloud.io:6380").expect("Failed to parse rediss URI");
    assert_eq!(uri.protocol, DatabaseProtocol::Redis);
    assert_eq!(uri.tls, TlsMode::Required);
    assert_eq!(uri.port, Some(6380));
}

#[test]
fn test_parse_duckdb_uri() {
    let uri_mem = ParsedUri::parse("duckdb://").expect("Failed to parse duckdb memory URI");
    assert_eq!(uri_mem.protocol, DatabaseProtocol::DuckDb);
    assert_eq!(uri_mem.database, None);

    let uri_file =
        ParsedUri::parse("duckdb://analytics.duckdb").expect("Failed to parse duckdb file URI");
    assert_eq!(uri_file.protocol, DatabaseProtocol::DuckDb);
    assert_eq!(uri_file.database, Some("analytics.duckdb".to_string()));
}

#[test]
fn test_parse_optional_prefixes() {
    let uri_voyager = ParsedUri::parse("voyager:bolt://localhost:7687")
        .expect("Failed to parse voyager:bolt URI");
    assert_eq!(uri_voyager.protocol, DatabaseProtocol::Bolt);
    assert_eq!(uri_voyager.backend_hint, None);

    let uri_native = ParsedUri::parse("voyager+native:bolt://localhost:7687")
        .expect("Failed to parse voyager+native URI");
    assert_eq!(uri_native.protocol, DatabaseProtocol::Bolt);
    assert_eq!(uri_native.backend_hint, Some("native".to_string()));

    let uri_bridge =
        ParsedUri::parse("voyager+bridge:postgresql://postgres:pass@localhost:5432/db")
            .expect("Failed to parse voyager+bridge URI");
    assert_eq!(uri_bridge.protocol, DatabaseProtocol::Postgres);
    assert_eq!(uri_bridge.backend_hint, Some("bridge".to_string()));

    let uri_vn = ParsedUri::parse("vn:falkordb://localhost:6379").expect("Failed to parse vn: URI");
    assert_eq!(uri_vn.protocol, DatabaseProtocol::Redis);
    assert_eq!(uri_vn.backend_hint, Some("native".to_string()));
}

#[test]
fn test_parse_invalid_uris() {
    assert!(ParsedUri::parse("").is_err());
    assert!(ParsedUri::parse("http://localhost:8080").is_err());
    assert!(ParsedUri::parse("mysql://root@localhost").is_err());
}
