Feature: Capability Checking
  Before routing a work order the runtime can check whether a backend
  satisfies the required capabilities. The mock backend supports
  Streaming (native) and several tool capabilities (emulated).

  Background:
    Given a runtime with the mock backend registered

  Scenario: Mock backend satisfies emulated tool_read requirement
    Given a work order with task "Read file" that requires emulated "tool_read" capability
    When the capability check is performed against the "mock" backend
    Then the capability check passes

  Scenario: Mock backend satisfies native streaming requirement
    Given a work order with task "Stream me" that requires native "streaming" capability
    When the capability check is performed against the "mock" backend
    Then the capability check passes

  Scenario: Native requirement fails for emulated capability
    Given a work order with task "Need native read" that requires native "tool_read" capability
    When the capability check is performed against the "mock" backend
    Then the capability check fails

  Scenario: Unsupported capability fails check
    Given a work order with task "Need MCP" that requires native "mcp_client" capability
    When the capability check is performed against the "mock" backend
    Then the capability check fails

  Scenario: Session resume is not supported by mock
    Given a work order with task "Resume session" that requires native "session_resume" capability
    When the capability check is performed against the "mock" backend
    Then the capability check fails

  Scenario: Capability check against unknown backend fails
    Given a work order with task "Ghost backend"
    When the capability check is performed against the "nonexistent" backend
    Then the capability check fails
