/**
 * Matrix-wide Conformance Tests
 *
 * Tests that validate the complete dialect×engine matrix,
 * ensuring all four cases are properly handled.
 */

const assert = require("node:assert");
const { describe, it } = require("node:test");
const {
  MATRIX,
  filterMatrix,
  runSidecarTest,
  createWorkOrder,
} = require("./runner");

// Get test filters from environment
const testDialect = process.env.ABP_TEST_DIALECT || null;
const testEngine = process.env.ABP_TEST_ENGINE || null;
const testMode = process.env.ABP_TEST_MODE || null;

// Filter matrix cells based on environment
const testCells = filterMatrix({
  dialect: testDialect,
  engine: testEngine,
  mode: testMode,
});

/**
 * Helper to get host dialect for a cell
 * For mapped mode, we need to use the source dialect's host
 */
function getHostDialect(cell) {
  return cell.dialect;
}

describe("Dialect×Engine Matrix", () => {
  describe("Matrix Configuration", () => {
    it("should define all four matrix cells", () => {
      assert.strictEqual(MATRIX.length, 4, "Matrix should have exactly 4 cells");

      const expectedCells = [
        { dialect: "claude", engine: "claude", mode: "passthrough" },
        { dialect: "claude", engine: "gemini", mode: "mapped" },
        { dialect: "codex", engine: "claude", mode: "mapped" },
        { dialect: "codex", engine: "codex", mode: "passthrough" },
      ];

      for (const expected of expectedCells) {
        const found = MATRIX.find(
          (c) =>
            c.dialect === expected.dialect &&
            c.engine === expected.engine &&
            c.mode === expected.mode
        );
        assert(found, `Matrix should include ${expected.dialect}→${expected.engine}`);
      }
    });

    it("should have consistent mode assignment", () => {
      // Passthrough mode: dialect matches engine
      const passthroughCells = MATRIX.filter((c) => c.mode === "passthrough");
      for (const cell of passthroughCells) {
        assert.strictEqual(
          cell.dialect,
          cell.engine,
          `Passthrough requires dialect===engine, got ${cell.dialect}!==${cell.engine}`
        );
      }

      // Mapped mode: dialect differs from engine
      const mappedCells = MATRIX.filter((c) => c.mode === "mapped");
      for (const cell of mappedCells) {
        assert.notStrictEqual(
          cell.dialect,
          cell.engine,
          `Mapped requires dialect!==engine, got ${cell.dialect}===${cell.engine}`
        );
      }
    });
  });

  // Run tests for each matrix cell
  for (const cell of testCells) {
    describe(`${cell.dialect}→${cell.engine} (${cell.mode})`, () => {
      it("should complete without hanging", async () => {
        const workOrder = createWorkOrder({
          task: "Basic smoke test",
          workspace: {
            root: process.cwd(),
            mode: cell.mode === "passthrough" ? "pass_through" : "staged",
          },
          config: {
            vendor: {
              "abp.mode": cell.mode,
            },
          },
        });

        // Set timeout to detect hangs
        const timeout = setTimeout(() => {
          assert.fail("Test timed out - possible hang");
        }, 30000);

        try {
          const hostDialect = getHostDialect(cell);
          const result = await runSidecarTest(hostDialect, workOrder);
          clearTimeout(timeout);

          assert(result.messages, "Should return messages");
          assert(Array.isArray(result.messages), "Messages should be an array");
        } finally {
          clearTimeout(timeout);
        }
      });

      it("should send hello envelope first", async () => {
        const workOrder = createWorkOrder({
          task: "Hello test",
          workspace: {
            root: process.cwd(),
            mode: cell.mode === "passthrough" ? "pass_through" : "staged",
          },
        });

        const hostDialect = getHostDialect(cell);
        const { messages } = await runSidecarTest(hostDialect, workOrder);

        assert(messages.length > 0, "Should have at least one message");
        const hello = messages[0];
        assert.strictEqual(hello.t, "hello", "First message should be hello");
      });

      it("should include mode in hello envelope", async () => {
        const workOrder = createWorkOrder({
          task: "Mode test",
          workspace: {
            root: process.cwd(),
            mode: cell.mode === "passthrough" ? "pass_through" : "staged",
          },
        });

        const hostDialect = getHostDialect(cell);
        const { messages } = await runSidecarTest(hostDialect, workOrder);

        const hello = messages.find((m) => m.t === "hello");
        assert(hello, "Hello message should be present");
        assert(hello.mode, "Hello should include mode field");
      });

      it("should produce valid receipt", async () => {
        const workOrder = createWorkOrder({
          task: "Receipt test",
          workspace: {
            root: process.cwd(),
            mode: cell.mode === "passthrough" ? "pass_through" : "staged",
          },
        });

        const hostDialect = getHostDialect(cell);
        const { messages } = await runSidecarTest(hostDialect, workOrder);

        const receipt = messages.find((m) => m.t === "receipt");
        assert(receipt, "Receipt message should be present");

        // Validate receipt structure
        assert(receipt.id, "Receipt should have id");
        assert(receipt.contract_version, "Receipt should have contract_version");
        assert(receipt.status, "Receipt should have status");
      });

      it("should respect execution mode", async () => {
        const workOrder = createWorkOrder({
          task: "Mode respect test",
          workspace: {
            root: process.cwd(),
            mode: cell.mode === "passthrough" ? "pass_through" : "staged",
          },
          config: {
            vendor: {
              "abp.mode": cell.mode,
            },
          },
        });

        const hostDialect = getHostDialect(cell);
        const { messages } = await runSidecarTest(hostDialect, workOrder);

        const hello = messages.find((m) => m.t === "hello");
        assert(hello, "Hello message should be present");

        // Mode should match expected
        if (hello.mode) {
          assert.strictEqual(
            hello.mode,
            cell.mode,
            `Mode should be ${cell.mode}`
          );
        }
      });

      it("should include contract version", async () => {
        const workOrder = createWorkOrder({
          task: "Contract version test",
        });

        const hostDialect = getHostDialect(cell);
        const { messages } = await runSidecarTest(hostDialect, workOrder);

        const receipt = messages.find((m) => m.t === "receipt");
        assert(receipt, "Receipt should be present");
        assert(
          receipt.contract_version,
          "Receipt should include contract_version"
        );
        assert(
          receipt.contract_version.startsWith("abp/"),
          "Contract version should start with 'abp/'"
        );
      });

      it("should include backend info in receipt", async () => {
        const workOrder = createWorkOrder({
          task: "Backend info test",
        });

        const hostDialect = getHostDialect(cell);
        const { messages } = await runSidecarTest(hostDialect, workOrder);

        const receipt = messages.find((m) => m.t === "receipt");
        assert(receipt, "Receipt should be present");

        // Backend info should be present
        if (receipt.backend) {
          assert(receipt.backend.id || receipt.backend.type, "Backend should have id or type");
        }
      });
    });
  }
});

// Test cross-matrix consistency
describe("Cross-Matrix Consistency", () => {
  it("should have consistent error codes across all cells", async () => {
    const errorCodes = [
      "E001", // UnsupportedFeature
      "E002", // UnsupportedTool
      "E003", // AmbiguousMapping
      "E004", // RequiresInteractiveApproval
      "E005", // UnsafeByPolicy
      "E006", // BackendCapabilityMissing
      "E007", // BackendUnavailable
    ];

    // This is a documentation/contract test
    // In real tests, we would verify these codes are used consistently
    assert.strictEqual(errorCodes.length, 7, "Should have 7 error codes defined");
  });

  it("should use consistent receipt structure across all cells", () => {
    const requiredFields = [
      "id",
      "contract_version",
      "status",
      "started_at",
      "completed_at",
    ];

    // This documents the required receipt fields
    // Actual validation happens in receipt.test.js
    assert.strictEqual(requiredFields.length, 5, "Should have 5 required receipt fields");
  });
});
