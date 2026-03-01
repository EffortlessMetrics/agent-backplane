Feature: Receipt Verification
  Receipts provide tamper-evident proof of agent execution. These
  scenarios verify integrity properties beyond basic field presence,
  including deterministic hashing, trace consistency, and backend
  identity recording.

  Background:
    Given a runtime with the mock backend registered

  Scenario: Successful run produces a receipt with valid hash
    Given a work order with task "Verify hash"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt has a SHA-256 hash
    And the hash is a 64-character hex string

  Scenario: Receipt hash is deterministic for same receipt content
    Given a work order with task "Determinism check"
    When the work order is submitted to the "mock" backend
    Then the receipt has a SHA-256 hash
    And recomputing the hash produces the same value

  Scenario: Receipt contains the correct backend identity
    Given a work order with task "Backend identity"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt backend id is "mock"

  Scenario: Receipt trace has at least the bookend events
    Given a work order with task "Trace count"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt trace contains at least 2 events

  Scenario: Receipt trace length matches mock output
    Given a work order with task "Exact trace"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt trace contains at least 4 events

  Scenario: Submitting to nonexistent backend yields error not receipt
    Given a work order with task "No backend"
    When the work order is submitted to the "nonexistent" backend
    Then the run fails with an error containing "unknown backend"

  Scenario: Receipt capabilities snapshot includes streaming
    Given a work order with task "Cap snapshot"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt capabilities include "streaming"
