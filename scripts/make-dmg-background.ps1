# Draws the macOS .dmg window background (660x400).
param([string]$Out = "src-tauri/dmg/background.png")
Add-Type -AssemblyName System.Drawing
$W = 660; $H = 400
$bmp = New-Object System.Drawing.Bitmap $W, $H
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = 'AntiAlias'
$g.TextRenderingHint = 'AntiAliasGridFit'
function C($a, $r, $gr, $b) { [System.Drawing.Color]::FromArgb($a, $r, $gr, $b) }

$bg = New-Object System.Drawing.Drawing2D.LinearGradientBrush ([System.Drawing.Point]::new(0, 0)), ([System.Drawing.Point]::new($W, $H)), (C 255 246 245 255), (C 255 234 246 252)
$g.FillRectangle($bg, 0, 0, $W, $H)

# soft color blobs
foreach ($b in @(@(-120, -160, 420, (C 40 118 82 255)), @(470, 230, 380, (C 40 20 200 232)))) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $p.AddEllipse($b[0], $b[1], $b[2], $b[2])
  $pgb = New-Object System.Drawing.Drawing2D.PathGradientBrush $p
  $pgb.CenterColor = $b[3]
  $pgb.SurroundColors = @((C 0 255 255 255))
  $g.FillPath($pgb, $p)
}

$title = New-Object System.Drawing.Font 'Segoe UI Semibold', 22
$sub = New-Object System.Drawing.Font 'Malgun Gothic', 11
$sf = New-Object System.Drawing.StringFormat
$sf.Alignment = 'Center'
$g.DrawString('NoopFlow', $title, (New-Object System.Drawing.SolidBrush (C 255 23 26 43)), [System.Drawing.RectangleF]::new(0, 34, $W, 40), $sf)
$g.DrawString('NoopFlow를 Applications 폴더로 끌어다 놓으세요', $sub, (New-Object System.Drawing.SolidBrush (C 255 91 96 120)), [System.Drawing.RectangleF]::new(0, 76, $W, 24), $sf)

# arrow between the app (x=180) and Applications (x=480), icons centered at y=210
$pen = New-Object System.Drawing.Pen (C 255 99 91 255), 5
$pen.StartCap = 'Round'
$pen.DashStyle = 'Dot'
$g.DrawLine($pen, 268, 210, 372, 210)
$head = New-Object System.Drawing.Drawing2D.GraphicsPath
$head.AddPolygon(@([System.Drawing.PointF]::new(372, 198), [System.Drawing.PointF]::new(394, 210), [System.Drawing.PointF]::new(372, 222)))
$g.FillPath((New-Object System.Drawing.SolidBrush (C 255 99 91 255)), $head)

$note = New-Object System.Drawing.Font 'Malgun Gothic', 9
$g.DrawString('처음 열 때: 우클릭 → 열기, 그리고 손쉬운 사용 권한을 켜 주세요', $note, (New-Object System.Drawing.SolidBrush (C 255 144 149 171)), [System.Drawing.RectangleF]::new(0, 352, $W, 20), $sf)

$g.Dispose()
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host "saved $Out"
