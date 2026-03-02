# Run rustfmt on specific test files
cd "H:\Code\Rust\agent-backplane"

$files = @(
    "tests/api_surface_comprehensive.rs",
    "tests/serde_canonical_comprehensive.rs",
    "tests/bdd_scenarios_comprehensive.rs",
    "tests/fuzz_harness_comprehensive.rs",
    "tests/contract_version_comprehensive.rs",
    "tests/daemon_comprehensive.rs",
    "tests/sdk_adapter_comprehensive.rs",
    "tests/cross_crate_comprehensive.rs",
    "tests/policy_enforcement_comprehensive.rs"
)

$successCount = 0
$failureCount = 0
$results = @()

Write-Host "Starting rustfmt on test files..." -ForegroundColor Cyan
Write-Host ""

for ($i = 0; $i -lt $files.Count; $i++) {
    $file = $files[$i]
    $fileNumber = $i + 1
    
    Write-Host "[$fileNumber/9] Formatting $file..." -ForegroundColor Yellow
    
    $output = rustfmt $file 2>&1
    $exitCode = $LASTEXITCODE
    
    if ($exitCode -eq 0) {
        Write-Host "       ✓ SUCCESS" -ForegroundColor Green
        $results += "Test $fileNumber ($file): ✓ SUCCESS"
        $successCount++
    } else {
        Write-Host "       ✗ FAILED (exit code: $exitCode)" -ForegroundColor Red
        $results += "Test $fileNumber ($file): ✗ FAILED (exit code: $exitCode)"
        if ($output) {
            Write-Host "       Error: $output" -ForegroundColor Red
        }
        $failureCount++
    }
}

Write-Host ""
Write-Host "=== SUMMARY ===" -ForegroundColor Cyan
Write-Host "Successes: $successCount" -ForegroundColor Green
Write-Host "Failures: $failureCount" -ForegroundColor Red
Write-Host ""
$results | ForEach-Object { Write-Host $_ }

Write-Host ""
Write-Host "=== Git Diff ===" -ForegroundColor Cyan
git diff --stat

Write-Host ""
Write-Host "Done." -ForegroundColor Green
