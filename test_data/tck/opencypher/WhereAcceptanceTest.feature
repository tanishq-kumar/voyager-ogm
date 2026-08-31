# openCypher TCK - WHERE Clause & Predicates Acceptance Test
# Official Feature: WhereAcceptanceTest
# Source: opencypher/openCypher Technology Compatibility Kit (TCK)

Feature: WhereAcceptanceTest

  Background:
    Given an empty graph
    And having executed:
      """
      CREATE (:Person {name: 'Alice', age: 38, active: true, email: 'alice@example.com'})
      CREATE (:Person {name: 'Bob', age: 25, active: false, email: 'bob@corp.org'})
      CREATE (:Person {name: 'Charlie', age: null, active: true, email: null})
      """

  Scenario: [1] Filtering with IS NOT NULL and boolean conditions
    When executing query:
      """
      MATCH (p:Person)
      WHERE p.age IS NOT NULL AND p.active = true
      RETURN p.name
      ORDER BY p.name ASC
      """
    Then the result should be:
      | p.name  |
      | 'Alice' |
    And no side effects

  Scenario: [2] Filtering with String CONTAINS and ENDS WITH
    When executing query:
      """
      MATCH (p:Person)
      WHERE p.email ENDS WITH 'example.com' OR p.name CONTAINS 'Bob'
      RETURN p.name, p.email
      ORDER BY p.name ASC
      """
    Then the result should be:
      | p.name  | p.email             |
      | 'Alice' | 'alice@example.com' |
      | 'Bob'   | 'bob@corp.org'      |
    And no side effects

  Scenario: [3] Filtering with IN list predicate
    When executing query:
      """
      MATCH (p:Person)
      WHERE p.name IN ['Alice', 'Charlie', 'NonExistent']
      RETURN p.name
      ORDER BY p.name ASC
      """
    Then the result should be:
      | p.name    |
      | 'Alice'   |
      | 'Charlie' |
    And no side effects
