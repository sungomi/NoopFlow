# Uploads the Mac signing certificate in .signing/ as GitHub Actions secrets
# (MAC_CERT_P12, MAC_CERT_PASSWORD). Run once from the project root:
#   powershell -ExecutionPolicy Bypass -File scripts/set-signing-secrets.ps1
$ErrorActionPreference = 'Stop'
$env:Path += ";C:\Program Files\GitHub CLI"
$root = Split-Path $PSScriptRoot -Parent
$p12 = Join-Path $root '.signing\noopflow-signing.p12'
$pw = Join-Path $root '.signing\password.txt'
if (-not (Test-Path $p12) -or -not (Test-Path $pw)) { throw "Missing $p12 or $pw" }

[Convert]::ToBase64String([IO.File]::ReadAllBytes($p12)) | gh secret set MAC_CERT_P12 --repo sungomi/NoopFlow
(Get-Content $pw -Raw).Trim() | gh secret set MAC_CERT_PASSWORD --repo sungomi/NoopFlow
gh secret list --repo sungomi/NoopFlow
Write-Host "Done. Mac builds will now be signed with 'NoopFlow Developer'."
