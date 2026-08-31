# openCypher TCK - RETURN Clause & Aggregation Acceptance Test
# Official Feature: ReturnAcceptanceTest
# Source: opencypher/openCypher Technology Compatibility Kit (TCK)

Feature: ReturnAcceptanceTest

  Background:
    Given an empty graph
    And having executed:
      """
      CREATE (:Product {id: 'p1', name: 'Laptop', price: 1200.0, category: 'Electronics'})
      CREATE (:Product {id: 'p2', name: 'Mouse', price: 25.0, category: 'Electronics'})
      CREATE (:Product {id: 'p3', name: 'Keyboard', price: 75.0, category: 'Electronics'})
      CREATE (:Product {id: 'p4', name: 'Desk', price: 350.0, category: 'Furniture'})
      CREATE (:Product {id: 'p5', name: 'Chair', price: 150.0, category: 'Furniture'})
      """

  Scenario: [1] Simple projection with column aliasing and sorting
    When executing query:
      """
      MATCH (p:Product)
      RETURN p.name AS product_name, p.price AS unit_price
      ORDER BY p.price DESC
      LIMIT 3
      """
    Then the result should be:
      | product_name | unit_price |
      | 'Laptop'     | 1200.0     |
      | 'Desk'       | 350.0      |
      | 'Chair'      | 150.0      |
    And no side effects

  Scenario: [2] Aggregations with COUNT, SUM, MIN, MAX and implicit GROUP BY
    When executing query:
      """
      MATCH (p:Product)
      RETURN p.category AS category, count(p) AS total_items, sum(p.price) AS total_value, min(p.price) AS min_price, max(p.price) AS max_price
      ORDER BY total_value DESC
      """
    Then the result should be:
      | category      | total_items | total_value | min_price | max_price |
      | 'Electronics' | 3           | 1300.0      | 25.0      | 1200.0    |
      | 'Furniture'   | 2           | 500.0       | 150.0     | 350.0     |
    And no side effects

  Scenario: [3] DISTINCT projection in RETURN
    When executing query:
      """
      MATCH (p:Product)
      RETURN DISTINCT p.category AS unique_category
      ORDER BY unique_category ASC
      """
    Then the result should be:
      | unique_category |
      | 'Electronics'   |
      | 'Furniture'     |
    And no side effects
