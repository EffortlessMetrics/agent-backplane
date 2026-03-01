Feature: Receipt Validation
  Receipts are the canonical proof of an agent run. They include a
  SHA-256 hash computed over the canonical JSON form of the receipt
  with the hash field set to null.

  Background:
    Given a runtime with the mock backend registered

  Scenario: Receipt contains a SHA-256 hash
    Given a work order with task "Hash me"
    When the work order is submitted to the "mock" backend
    Then the receipt has a SHA-256 hash

  Scenario: Hash is a valid 64-character hex digest
    Given a work order with task "Hex check"
    When the work order is submitted to the "mock" backend
    Then the receipt has a SHA-256 hash
    And the hash is a 64-character hex string

  Scenario: Recomputing the hash is deterministic
    Given a work order with task "Deterministic hash"
    When the work order is submitted to the "mock" backend
    Then the receipt has a SHA-256 hash
    And recomputing the hash produces the same value

  Scenario: Receipt includes the contract version
    Given a work order with task "Version stamp"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt contract version is "abp/v0.1"

  Scenario: Receipt timestamps are ordered
    Given a work order with task "Time ordering"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt started_at is before or equal to finished_at

  Scenario: Receipt serializes to valid JSON
    Given a work order with task "JSON round-trip"
    When the work order is submitted to the "mock" backend
    Then the run completes successfully
    And the receipt can be serialized to JSON and back
