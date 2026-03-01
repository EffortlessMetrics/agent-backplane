Feature: Work Order Routing
  The runtime routes work orders to the appropriate backend based on
  the backend name and validates pre-conditions before execution.

  Background:
    Given a runtime with the mock backend registered

  Scenario: Route a simple work order to the mock backend
    Given a work order with task "Say hello"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt outcome is "complete"

  Scenario: Route produces a non-empty event trace
    Given a work order with task "Trace me"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt contains a non-empty trace

  Scenario: Trace includes RunStarted and RunCompleted events
    Given a work order with task "Bookend events"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the trace starts with a RunStarted event
    And the trace ends with a RunCompleted event

  Scenario: Receipt records the correct work order id
    Given a work order with task "ID round-trip"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt work_order_id matches the submitted work order

  Scenario: Receipt records the mock backend identity
    Given a work order with task "Identity check"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt backend id is "mock"

  Scenario: Submitting to an unknown backend fails
    Given a work order with task "Missing backend"
    When the work order is submitted to the "nonexistent" backend
    Then the run fails with an error containing "unknown backend"

  Scenario: Multiple sequential work orders each get unique receipts
    Given a work order with task "First run"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt is saved as "first"
    Given a work order with task "Second run"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt is saved as "second"
    And the saved receipts "first" and "second" have different run ids
