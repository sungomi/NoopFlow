# Captures README screenshots of the UI (sample data) with headless Edge.
# Start scripts/serve-ui.ps1 first, then run:  powershell -File scripts/screenshots.ps1
param([string]$Base = "http://localhost:5178/", [string]$Out = "docs")
$edge = "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
New-Item -ItemType Directory -Force $Out | Out-Null
$outDir = (Resolve-Path $Out).Path
$profile = Join-Path $env:TEMP "noopflow-shots"
$shots = @(
  @{ Name = "layout-dark.png";   Url = "$Base#layout";        Dark = $true },
  @{ Name = "layout-light.png";  Url = "$Base#layout";        Dark = $false },
  @{ Name = "devices.png";       Url = "$Base#devices";       Dark = $true },
  @{ Name = "settings-mac.png";  Url = "${Base}?os=mac#settings"; Dark = $true },
  @{ Name = "pairing.png";       Url = "$Base#pair";          Dark = $true }
)
foreach ($s in $shots) {
  $args = @("--headless=new", "--disable-gpu", "--hide-scrollbars", "--user-data-dir=$profile",
            "--force-device-scale-factor=2", "--window-size=1100,720", "--virtual-time-budget=3000",
            "--screenshot=$(Join-Path $outDir $s.Name)")
  if ($s.Dark) { $args += "--force-dark-mode", "--blink-settings=preferredColorScheme=0" }
  else { $args += "--blink-settings=preferredColorScheme=1" }
  $args += $s.Url
  Start-Process $edge -ArgumentList $args -Wait -WindowStyle Hidden
  Write-Host "saved $($s.Name)"
}
