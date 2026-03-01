#!/usr/bin/env pwsh

Set-Location 'H:\Code\Rust\agent-backplane'

Write-Host "Closing PR #11..."
gh pr close 11 -c "Closing as superseded by #6 (Split integrations into SRP backend microcrates) which was already merged to main."

Write-Host "Closing PR #12..."
gh pr close 12 -c "Closing as superseded by #9 (Extract shared sidecar registration into SRP microcrate) which was already merged to main."

Write-Host "Closing PR #13..."
gh pr close 13 -c "Closing as superseded by #6 (Split integrations into SRP backend microcrates) which was already merged to main."

Write-Host "Closing PR #14..."
gh pr close 14 -c "Closing as superseded by #9 (Extract shared sidecar registration into SRP microcrate) which was already merged to main."

Write-Host "Closing PR #8..."
gh pr close 8 -c "Closing as superseded by #15 which implements the same abp-which extraction."

Write-Host "All PRs closed successfully!"
