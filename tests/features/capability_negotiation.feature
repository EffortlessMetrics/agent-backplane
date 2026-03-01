Feature: Capability Negotiation
  Capability negotiation ensures that the runtime can inspect a backend's
  capability manifest and match it against work order requirements with
  the correct support-level semantics: native beats emulated beats
  unsupported.

  Background:
    Given a runtime with the mock backend registered

  Scenario: Backend with all capabilities satisfies any single requirement
    Given a work order with task "Single cap" that requires emulated "streaming" capability
    When the capability check is performed against the "mock" backend
    Then the capability check passes

  Scenario: Multiple required capabilities all satisfied
    Given a work order with task "Multi cap" that requires native "streaming" and emulated "tool_read"
    When the capability check is performed against the "mock" backend
    Then the capability check passes

  Scenario: One unsatisfied capability in a multi-requirement fails
    Given a work order with task "Partial cap" that requires native "streaming" and native "mcp_client"
    When the capability check is performed against the "mock" backend
    Then the capability check fails

  Scenario: Emulated satisfies emulated requirement
    Given a work order with task "Emulated ok" that requires emulated "tool_write" capability
    When the capability check is performed against the "mock" backend
    Then the capability check passes

  Scenario: Emulated does not satisfy native requirement
    Given a work order with task "Emulated fail" that requires native "tool_write" capability
    When the capability check is performed against the "mock" backend
    Then the capability check fails

  Scenario: Manifest reports streaming as native
    When the capability manifest is fetched for the "mock" backend
    Then the manifest reports "streaming" as "native"

  Scenario: Manifest reports tool_read as emulated
    When the capability manifest is fetched for the "mock" backend
    Then the manifest reports "tool_read" as "emulated"

  Scenario: Manifest does not include mcp_client
    When the capability manifest is fetched for the "mock" backend
    Then the manifest does not include "mcp_client"
