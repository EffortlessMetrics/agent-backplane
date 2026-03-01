Feature: Policy Enforcement
  The PolicyEngine compiles a PolicyProfile into allow/deny glob
  rules for tools, read paths, and write paths. Deny rules always
  take precedence over allow rules.

  Scenario: Empty policy allows all tools
    Given an empty policy profile
    When the policy engine is compiled
    Then the tool "Bash" is allowed
    And the tool "Read" is allowed
    And the tool "Write" is allowed

  Scenario: Disallowed tool is denied
    Given a policy that disallows tool "Bash"
    When the policy engine is compiled
    Then the tool "Bash" is denied
    And the tool "Read" is allowed

  Scenario: Allowlist restricts to listed tools only
    Given a policy that allows only tools "Read,Grep"
    When the policy engine is compiled
    Then the tool "Read" is allowed
    And the tool "Grep" is allowed
    And the tool "Bash" is denied

  Scenario: Deny overrides allow for the same tool
    Given a policy that allows only tools "Read,Bash" and disallows tool "Bash"
    When the policy engine is compiled
    Then the tool "Bash" is denied
    And the tool "Read" is allowed

  Scenario: Wildcard allow with specific deny
    Given a policy that allows only tools "*" and disallows tool "Bash"
    When the policy engine is compiled
    Then the tool "Read" is allowed
    And the tool "Write" is allowed
    And the tool "Bash" is denied

  Scenario: Deny read path blocks matching files
    Given a policy that denies reading "secret*"
    When the policy engine is compiled
    Then reading path "secret.txt" is denied
    And reading path "secrets.yaml" is denied
    And reading path "src/lib.rs" is allowed

  Scenario: Deny write path blocks matching files
    Given a policy that denies writing "**/.git/**"
    When the policy engine is compiled
    Then writing path ".git/config" is denied
    And writing path "src/main.rs" is allowed

  Scenario: Multiple deny-read patterns
    Given a policy that denies reading "**/.env" and "**/.env.*"
    When the policy engine is compiled
    Then reading path ".env" is denied
    And reading path ".env.production" is denied
    And reading path "src/lib.rs" is allowed

  Scenario: Empty policy allows all paths
    Given an empty policy profile
    When the policy engine is compiled
    Then reading path "any/file.txt" is allowed
    And writing path "any/file.txt" is allowed

  Scenario: Deep nested path deny
    Given a policy that denies writing "secret/**"
    When the policy engine is compiled
    Then writing path "secret/a/b/c.txt" is denied
    And writing path "secret/x.txt" is denied
    And writing path "public/data.txt" is allowed

  Scenario: Denied tool produces a denial reason
    Given a policy that disallows tool "Bash"
    When the policy engine is compiled
    Then the tool "Bash" is denied
    And the tool "Bash" denial reason contains "disallowed"

  Scenario: Combined tool deny with read and write deny
    Given a policy that denies tool "Bash", reading "secret/**", and writing "*.lock"
    When the policy engine is compiled
    Then the tool "Bash" is denied
    And the tool "Read" is allowed
    And reading path "secret/key.pem" is denied
    And reading path "src/main.rs" is allowed
    And writing path "Cargo.lock" is denied
    And writing path "src/lib.rs" is allowed

  Scenario: Write deny does not affect read on same pattern
    Given a policy that denies writing "config/**"
    When the policy engine is compiled
    Then writing path "config/app.toml" is denied
    And reading path "config/app.toml" is allowed

  Scenario: Multiple deny-write patterns
    Given a policy that denies writing "*.lock" and "dist/**"
    When the policy engine is compiled
    Then writing path "Cargo.lock" is denied
    And writing path "dist/bundle.js" is denied
    And writing path "src/main.rs" is allowed

  Scenario: Glob tool allowlist with pattern matching
    Given a policy that allows only tools "Tool*"
    When the policy engine is compiled
    Then the tool "ToolRead" is allowed
    And the tool "ToolBash" is allowed
    And the tool "Grep" is denied
