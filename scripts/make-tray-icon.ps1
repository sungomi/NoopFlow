# Draws the macOS menu bar "template" icon (black glyph on transparent; macOS tints it).
#   powershell -File scripts/make-tray-icon.ps1
param([string]$Out = "src-tauri/icons/tray-template.png")
Add-Type -AssemblyName System.Drawing
$S = 44  # 22pt @2x
$bmp = New-Object System.Drawing.Bitmap $S, $S
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = 'AntiAlias'
$g.Clear([System.Drawing.Color]::Transparent)
$ink = [System.Drawing.Color]::FromArgb(255, 0, 0, 0)

# two eyes (outlined, with pupils looking up-right)
$pen = New-Object System.Drawing.Pen $ink, 3.2
foreach ($cx in 14.5, 29.5) {
  $g.DrawEllipse($pen, $cx - 6.5, 6, 13, 17)
  $g.FillEllipse((New-Object System.Drawing.SolidBrush $ink), $cx - 2.2, 10.5, 6, 8)
}
# blanket wave below
$wave = New-Object System.Drawing.Pen $ink, 3.6
$wave.StartCap = 'Round'; $wave.EndCap = 'Round'
$g.DrawBezier($wave, 4, 31, 13, 25, 21, 36, 30, 30)
$g.DrawBezier($wave, 30, 30, 34, 27, 37, 28, 40, 30)

$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
Write-Host "saved $Out"
