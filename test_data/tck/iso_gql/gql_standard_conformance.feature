# ISO/IEC 39075:2024 GQL Standard Conformance Suite
# Standard: Information technology — Database languages — GQL (Graph Query Language)
# Source: LDBC GQL Standards & ISO/IEC JTC 1/SC 32 Working Group

Feature: GqlStandardConformance

  Background:
    Given an empty graph
    And having executed:
      """
      INSERT (:Employee {empId: 'E101', name: 'Diana', salary: 120000, dept: 'Engineering'})
      INSERT (:Employee {empId: 'E102', name: 'Evan', salary: 95000, dept: 'Design'})
      INSERT (:Employee {empId: 'E103', name: 'Fiona', salary: 140000, dept: 'Engineering'})
      INSERT (:Department {code: 'ENG', name: 'Engineering'})
      INSERT (:Department {code: 'DSG', name: 'Design'})
      """

  Scenario: [1] ISO GQL Standard MATCH statement with property condition
    When executing query:
      """
      MATCH (e:Employee WHERE e.salary >= 100000)
      RETURN e.name AS emp_name, e.salary AS emp_salary
      ORDER BY e.salary DESC
      """
    Then the result should be:
      | emp_name | emp_salary |
      | 'Fiona'  | 140000     |
      | 'Diana'  | 120000     |
    And no side effects

  Scenario: [2] ISO GQL Standard Path Pattern with Edge Labels
    When executing query:
      """
      MATCH (e:Employee) -[:WORKS_IN]-> (d:Department WHERE d.code = 'ENG')
      RETURN e.name, d.name
      ORDER BY e.name ASC
      """
    Then the result should be:
      | e.name  | d.name        |
      | 'Diana' | 'Engineering' |
      | 'Fiona' | 'Engineering' |
    And no side effects

  Scenario: [3] ISO GQL Standard Variable Length Path with Parenthesized Path Pattern
    When executing query:
      """
      MATCH (e1:Employee {name: 'Diana'}) -[:REPORTS_TO]->{1, 3} (manager:Employee)
      RETURN manager.name AS manager_name
      ORDER BY manager_name ASC
      """
    Then the result should be:
      | manager_name |
      | 'Fiona'      |
    And no side effects
