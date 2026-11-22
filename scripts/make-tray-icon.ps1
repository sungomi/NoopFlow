# Draws the macOS menu bar "template" icon (black glyph on transparent; macOS tints it):
# the NooP eyes over two flowing waves.
#   powershell -File scripts/make-tray-icon.ps1
param([string]$Out = "src-tauri/icons/tray-template.png")
Add-Type -AssemblyName System.Drawing
$S = 44  # 22pt @2x
$bmp = New-Object System.Drawing.Bitmap $S, $S
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = 'AntiAlias'
$g.Clear([System.Drawing.Color]::Transparent)
$ink = [System.Drawing.Color]::FromArgb(255, 0, 0, 0)

# two eyes (outlined, pupils looking up-right)
$pen = New-Object System.Drawing.Pen $ink, 3.0
foreach ($cx in 14.5, 29.5) {
  $g.DrawEllipse($pen, $cx - 6.2, 3, 12.4, 16)
  $g.FillEllipse((New-Object System.Drawing.SolidBrush $ink), $cx - 2, 7, 5.6, 7.6)
}

# two flowing waves below
$wave = New-Object System.Drawing.Pen $ink, 3.2
$wave.StartCap = 'Round'; $wave.EndCap = 'Round'
foreach ($y in 26, 35) {
  $g.DrawBezier($wave, 4, $y + 1, 12, $y - 5, 20, $y + 6, 28, $y)
  $g.DrawBezier($wave, 28, $y, 33, $y - 4, 37, $y - 3, 40, $y - 1)
}

$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
Write-Host "saved $Out"
