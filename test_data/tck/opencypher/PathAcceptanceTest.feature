# openCypher TCK - Variable Length & Path Pattern Acceptance Test
# Official Feature: PathAcceptanceTest
# Source: opencypher/openCypher Technology Compatibility Kit (TCK)

Feature: PathAcceptanceTest

  Background:
    Given an empty graph
    And having executed:
      """
      CREATE (n1:Station {name: 'Station A'})
      CREATE (n2:Station {name: 'Station B'})
      CREATE (n3:Station {name: 'Station C'})
      CREATE (n4:Station {name: 'Station D'})
      CREATE (n1)-[:CONNECTED_TO {distance: 5}]->(n2)
      CREATE (n2)-[:CONNECTED_TO {distance: 8}]->(n3)
      CREATE (n3)-[:CONNECTED_TO {distance: 12}]->(n4)
      """

  Scenario: [1] Fixed 1-hop path traversal
    When executing query:
      """
      MATCH (s:Station {name: 'Station A'})-[:CONNECTED_TO]->(next:Station)
      RETURN next.name AS destination
      """
    Then the result should be:
      | destination |
      | 'Station B' |
    And no side effects

  Scenario: [2] Bounded variable length path 1 to 3 hops
    When executing query:
      """
      MATCH (s:Station {name: 'Station A'})-[:CONNECTED_TO*1..3]->(dest:Station)
      RETURN dest.name AS reachable_station
      ORDER BY reachable_station ASC
      """
    Then the result should be:
      | reachable_station |
      | 'Station B'       |
      | 'Station C'       |
      | 'Station D'       |
    And no side effects

  Scenario: [3] Reverse direction traversal
    When executing query:
      """
      MATCH (dest:Station {name: 'Station D'})<-[:CONNECTED_TO]-(prev:Station)
      RETURN prev.name AS previous_station
      """
    Then the result should be:
      | previous_station |
      | 'Station C'      |
    And no side effects
