# Modern Cypher 25 & GQL-Aligned Syntax Acceptance Test
# Feature: Cypher25ModernSyntax
# Reference: Neo4j Cypher 25 & ISO/IEC 39075:2024 GQL Alignment

Feature: Cypher25ModernSyntax

  Background:
    Given an empty graph
    And having executed:
      """
      CREATE (a:Person:Developer {name: 'Alice', seniority: 'Senior', salary: 150000})
      CREATE (b:Person:Manager {name: 'Bob', seniority: 'Lead', salary: 160000})
      CREATE (c:Person:Intern {name: 'Charlie', seniority: 'Junior', salary: 60000})
      CREATE (d:Bot:ServiceAccount {name: 'DeployBot', active: true})
      CREATE (a)-[:REPORTS_TO]->(b)
      CREATE (c)-[:REPORTS_TO]->(a)
      """

  Scenario: [1] Label Conjunction and Disjunction (Cypher 25 / GQL Label Expressions)
    When executing query:
      """
      MATCH (p:Person & (Developer | Manager))
      WHERE p.salary >= 100000
      RETURN p.name, p.seniority
      ORDER BY p.salary DESC
      """
    Then the result should be:
      | p.name  | p.seniority |
      | 'Bob'   | 'Lead'      |
      | 'Alice' | 'Senior'    |
    And no side effects

  Scenario: [2] Quantified Path Pattern (QPP) Repetition syntax
    When executing query:
      """
      MATCH ((sub:Person)-[:REPORTS_TO]->(mgr:Person)){1, 2}
      RETURN sub.name AS subordinate, mgr.name AS manager
      ORDER BY subordinate ASC, manager ASC
      """
    Then the result should be:
      | subordinate | manager |
      | 'Alice'     | 'Bob'   |
      | 'Charlie'   | 'Alice' |
      | 'Charlie'   | 'Bob'   |
    And no side effects

  Scenario: [3] SHORTEST Path Group Search
    When executing query:
      """
      MATCH SHORTEST 1 (start:Person {name: 'Charlie'})-[:REPORTS_TO]->+(boss:Person {name: 'Bob'})
      RETURN boss.name
      """
    Then the result should be:
      | boss.name |
      | 'Bob'     |
    And no side effects
