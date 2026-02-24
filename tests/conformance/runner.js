#!/usr/bin/env node

/**
 * Conformance Test Runner
 *
 * Main entry point for running the dialect×engine matrix conformance tests.
 * Supports filtering by dialect, engine, and mode.
 *
 * Usage:
 *   node runner.js                           # Run all tests
 *   node runner.js --dialect=claude          # Run Claude dialect tests only
 *   node runner.js --engine=gemini           # Run Gemini engine tests only
 *   node runner.js --mode=passthrough        # Run passthrough tests only
 *   node runner.js --dialect=claude --engine=gemini  # Run specific matrix cell
 */

const path = require("node:path");
const { spawn } = require("node:child_process");
const crypto = require("node:crypto");

// Matrix configuration
const MATRIX = [
  { dialect: "claude", engine: "claude", mode: "passthrough" },
  { dialect: "claude", engine: "gemini", mode: "mapped" },
  { dialect: "codex", engine: "claude", mode: "mapped" },
  { dialect: "codex", engine: "codex", mode: "passthrough" },
];

// Host paths for each dialect
const HOST_PATHS = {
  claude: path.resolve(__dirname, "../../hosts/claude/host.js"),
  gemini: path.resolve(__dirname, "../../hosts/gemini/host.js"),
  codex: path.resolve(__dirname, "../../hosts/codex/host.js"),
};

// Mock adapter paths for testing
const MOCK_ADAPTER_PATHS = {
  claude: path.resolve(__dirname, "../../hosts/claude/test/mock-adapter.js"),
  gemini: path.resolve(__dirname, "../../hosts/gemini/test/mock-adapter.js"),
  codex: path.resolve(__dirname, "../../hosts/codex/test/mock-adapter.js"),
};

/**
 * Parse command line arguments
 */
function parseArgs(args) {
  const options = {
    dialect: null,
    engine: null,
    mode: null,
    verbose: false,
    help: false,
  };

  for (const arg of args) {
    if (arg.startsWith("--dialect=")) {
      options.dialect = arg.split("=")[1];
    } else if (arg.startsWith("--engine=")) {
      options.engine = arg.split("=")[1];
    } else if (arg.startsWith("--mode=")) {
      options.mode = arg.split("=")[1];
    } else if (arg === "--verbose" || arg === "-v") {
      options.verbose = true;
    } else if (arg === "--help" || arg === "-h") {
      options.help = true;
    }
  }

  return options;
}

/**
 * Filter matrix cells based on options
 */
function filterMatrix(options) {
  return MATRIX.filter((cell) => {
    if (options.dialect && cell.dialect !== options.dialect) return false;
    if (options.engine && cell.engine !== options.engine) return false;
    if (options.mode && cell.mode !== options.mode) return false;
    return true;
  });
}

/**
 * Run a sidecar test and collect responses
 */
function runSidecarTest(dialect, workOrder, options = {}) {
  return new Promise((resolve, reject) => {
    const hostPath = HOST_PATHS[dialect];
    const mockAdapterPath = MOCK_ADAPTER_PATHS[dialect];

    const envVar = `ABP_${dialect.toUpperCase()}_ADAPTER_MODULE`;

    const proc = spawn("node", [hostPath], {
      stdio: ["pipe", "pipe", "pipe"],
      env: {
        ...process.env,
        [envVar]: mockAdapterPath,
        ...options.env,
      },
    });

    let stdout = "";
    let stderr = "";

    proc.stdout.on("data", (data) => {
      stdout += data.toString();
    });

    proc.stderr.on("data", (data) => {
      stderr += data.toString();
    });

    // Send the run message
    const runMsg = JSON.stringify({
      t: "run",
      id: crypto.randomUUID(),
      work_order: workOrder,
    });
    proc.stdin.write(runMsg + "\n");
    proc.stdin.end();

    proc.on("close", (code) => {
      // Parse JSONL output
      const lines = stdout.trim().split("\n").filter(Boolean);
      const messages = lines
        .map((line) => {
          try {
            return JSON.parse(line);
          } catch (e) {
            return null;
          }
        })
        .filter(Boolean);

      resolve({
        messages,
        stderr,
        exitCode: code,
        rawStdout: stdout,
      });
    });

    proc.on("error", reject);
  });
}

/**
 * Create a basic work order for testing
 */
function createWorkOrder(overrides = {}) {
  return {
    id: crypto.randomUUID(),
    task: overrides.task || "test task",
    lane: overrides.lane || "patch_first",
    workspace: {
      root: overrides.workspace?.root || process.cwd(),
      mode: overrides.workspace?.mode || "pass_through",
    },
    context: overrides.context || {},
    policy: overrides.policy || {},
    requirements: overrides.requirements || { required: [] },
    config: overrides.config || { vendor: {} },
    ...overrides,
  };
}

/**
 * Load a fixture file
 */
function loadFixture(fixtureName) {
  const fixturePath = path.resolve(
    __dirname,
    "fixtures/work-orders",
    `${fixtureName}.json`
  );
  try {
    return require(fixturePath);
  } catch (e) {
    console.error(`Failed to load fixture: ${fixtureName}`);
    throw e;
  }
}

/**
 * Print help message
 */
function printHelp() {
  console.log(`
Conformance Test Runner

Usage:
  node runner.js [options]

Options:
  --dialect=<dialect>    Filter by dialect (claude, codex)
  --engine=<engine>      Filter by engine (claude, gemini, codex)
  --mode=<mode>          Filter by mode (passthrough, mapped)
  --verbose, -v          Enable verbose output
  --help, -h             Show this help message

Examples:
  node runner.js                                    # Run all tests
  node runner.js --dialect=claude                   # Claude dialect only
  node runner.js --engine=gemini --mode=mapped      # Gemini mapped only
  node runner.js --dialect=claude --engine=claude   # Claude→Claude passthrough

Matrix Cells:
  Claude→Claude    (passthrough)
  Claude→Gemini    (mapped)
  Codex→Claude     (mapped)
  Codex→Codex      (passthrough)
`);
}

/**
 * Validate options
 */
function validateOptions(options) {
  const validDialects = ["claude", "codex"];
  const validEngines = ["claude", "gemini", "codex"];
  const validModes = ["passthrough", "mapped"];

  if (options.dialect && !validDialects.includes(options.dialect)) {
    console.error(
      `Invalid dialect: ${options.dialect}. Valid: ${validDialects.join(", ")}`
    );
    process.exit(1);
  }

  if (options.engine && !validEngines.includes(options.engine)) {
    console.error(
      `Invalid engine: ${options.engine}. Valid: ${validEngines.join(", ")}`
    );
    process.exit(1);
  }

  if (options.mode && !validModes.includes(options.mode)) {
    console.error(
      `Invalid mode: ${options.mode}. Valid: ${validModes.join(", ")}`
    );
    process.exit(1);
  }
}

// Export utilities for use by individual test files
module.exports = {
  MATRIX,
  HOST_PATHS,
  MOCK_ADAPTER_PATHS,
  parseArgs,
  filterMatrix,
  runSidecarTest,
  createWorkOrder,
  loadFixture,
  validateOptions,
  printHelp,
};

// Run as CLI if executed directly
if (require.main === module) {
  const options = parseArgs(process.argv.slice(2));

  if (options.help) {
    printHelp();
    process.exit(0);
  }

  validateOptions(options);

  const cells = filterMatrix(options);

  console.log("Conformance Test Runner");
  console.log("=======================");
  console.log(`Running ${cells.length} matrix cell(s):`);
  cells.forEach((cell) => {
    console.log(`  - ${cell.dialect}→${cell.engine} (${cell.mode})`);
  });
  console.log("");

  // Run tests via Node.js test runner
  const testArgs = [
    "--test",
    path.resolve(__dirname, "matrix.test.js"),
    path.resolve(__dirname, "passthrough.test.js"),
    path.resolve(__dirname, "mapped.test.js"),
    path.resolve(__dirname, "receipt.test.js"),
    path.resolve(__dirname, "error.test.js"),
  ];

  // Pass options as environment variables
  const testEnv = {
    ...process.env,
    ABP_TEST_DIALECT: options.dialect || "",
    ABP_TEST_ENGINE: options.engine || "",
    ABP_TEST_MODE: options.mode || "",
    ABP_TEST_VERBOSE: options.verbose ? "1" : "0",
  };

  const proc = spawn("node", testArgs, {
    stdio: "inherit",
    env: testEnv,
  });

  proc.on("close", (code) => {
    process.exit(code || 0);
  });
}
