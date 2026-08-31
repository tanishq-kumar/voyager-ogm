# openCypher TCK - Match Clause Acceptance Test
# Official Feature: MatchAcceptanceTest
# Source: opencypher/openCypher Technology Compatibility Kit (TCK)
# Standard: openCypher Specification 9

Feature: MatchAcceptanceTest

  Background:
    Given an empty graph
    And having executed:
      """
      CREATE (a:Person {name: 'Alice', age: 38, city: 'London'})
      CREATE (b:Person {name: 'Bob', age: 25, city: 'London'})
      CREATE (c:Person {name: 'Charlie', age: 53, city: 'New York'})
      CREATE (d:Person {name: 'Dan', age: 44, city: 'London'})
      CREATE (a)-[:KNOWS {since: 1999}]->(b)
      CREATE (b)-[:KNOWS {since: 2010}]->(c)
      CREATE (c)-[:KNOWS {since: 2015}]->(d)
      CREATE (a)-[:KNOWS {since: 2005}]->(c)
      """

  Scenario: [1] Matching simple node pattern with label and property filter
    When executing query:
      """
      MATCH (p:Person {city: 'London'})
      WHERE p.age > 30
      RETURN p.name, p.age
      ORDER BY p.age ASC
      """
    Then the result should be:
      | p.name  | p.age |
      | 'Alice' | 38    |
      | 'Dan'   | 44    |
    And no side effects

  Scenario: [2] Multi-hop directed traversal with relationship property filter
    When executing query:
      """
      MATCH (a:Person)-[r:KNOWS]->(b:Person)
      WHERE r.since >= 2005
      RETURN a.name AS source, b.name AS target, r.since AS year
      ORDER BY r.since ASC, a.name ASC
      """
    Then the result should be:
      | source    | target    | year |
      | 'Alice'   | 'Charlie' | 2005 |
      | 'Bob'     | 'Charlie' | 2010 |
      | 'Charlie' | 'Dan'     | 2015 |
    And no side effects

  Scenario: [3] Variable length relationship path matching
    When executing query:
      """
      MATCH (a:Person {name: 'Alice'})-[:KNOWS*1..2]->(target:Person)
      RETURN DISTINCT target.name
      ORDER BY target.name ASC
      """
    Then the result should be:
      | target.name |
      | 'Bob'       |
      | 'Charlie'   |
      | 'Dan'       |
    And no side effects

  Scenario: [4] Aggregation with COUNT, AVG, and GROUP BY in RETURN
    When executing query:
      """
      MATCH (p:Person)
      RETURN p.city AS city, count(p) AS count, avg(p.age) AS avg_age
      ORDER BY city ASC
      """
    Then the result should be:
      | city       | count | avg_age |
      | 'London'   | 3     | 35.666  |
      | 'New York' | 1     | 53.0    |
    And no side effects

  Scenario: [5] Undirected relationship pattern matching
    When executing query:
      """
      MATCH (b:Person {name: 'Bob'})-[:KNOWS]-(other:Person)
      RETURN other.name
      ORDER BY other.name ASC
      """
    Then the result should be:
      | other.name |
      | 'Alice'    |
      | 'Charlie'  |
    And no side effects
