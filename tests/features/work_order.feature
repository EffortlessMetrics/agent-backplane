Feature: Work Order Execution

  Scenario: Submit a work order with mock backend
    Given a runtime with the mock backend registered
    And a work order with task "Say hello"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt outcome is "complete"
    And the receipt contains a non-empty trace

  Scenario: Work order produces receipt with hash
    Given a runtime with the mock backend registered
    And a work order with task "Hash me"
    When the work order is submitted to the "mock" backend
    Then the receipt has a SHA-256 hash
    And the hash is a 64-character hex string
    And recomputing the hash produces the same value

  Scenario: Work order with unsatisfiable capabilities fails
    Given a runtime with the mock backend registered
    And a work order with task "Need MCP" that requires native "mcp_client" capability
    When the capability check is performed against the "mock" backend
    Then the capability check fails
