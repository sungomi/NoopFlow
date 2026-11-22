# Draws the NoopFlow icon: the NooP character (eyes peeking over a warm blanket under a
# night sky) with a flowing blanket and eyes following a cursor gliding across.
#   powershell -File scripts/make-icon.ps1                 -> src-tauri/icons/app-icon.png (macOS, Apple margin)
#   cd src-tauri; cargo tauri icon icons/app-icon.png      -> all sizes
#   powershell -File scripts/make-icon.ps1 -WindowsOnly    -> icon.ico + 32x32.png tuned for Windows
param([string]$Dir = "src-tauri/icons", [switch]$WindowsOnly)
Add-Type -AssemblyName System.Drawing

function C($a, $r, $g, $b) { [System.Drawing.Color]::FromArgb($a, $r, $g, $b) }
function P($x, $y) { [System.Drawing.PointF]::new($x, $y) }

function Squircle($cx, $cy, $half, $n) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $pts = for ($i = 0; $i -lt 360; $i++) {
    $t = $i * [math]::PI / 180
    $c = [math]::Cos($t); $s = [math]::Sin($t)
    P ($cx + $half * [math]::Sign($c) * [math]::Pow([math]::Abs($c), 2 / $n)) ($cy + $half * [math]::Sign($s) * [math]::Pow([math]::Abs($s), 2 / $n))
  }
  $p.AddPolygon($pts)
  $p
}
function Radial($cx, $cy, $rx, $ry, $inner, $outer) {
  $e = New-Object System.Drawing.Drawing2D.GraphicsPath
  $e.AddEllipse($cx - $rx, $cy - $ry, 2 * $rx, 2 * $ry)
  $b = New-Object System.Drawing.Drawing2D.PathGradientBrush $e
  $b.CenterColor = $inner
  $b.SurroundColors = @($outer)
  @($e, $b)
}
function Cursor($ox, $oy, $k) {
  $shape = @(@(0,0), @(0,300), @(75,232), @(128,346), @(180,322), @(128,212), @(226,210))
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $p.AddPolygon(($shape | ForEach-Object { P ($ox + $_[0]*$k) ($oy + $_[1]*$k) }))
  $p
}
# Blanket wave: top edge through the given points (Catmull-Rom curve), closed down to the bottom.
function Wave($pts) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $p.AddCurve([System.Drawing.PointF[]]($pts | ForEach-Object { P $_[0] $_[1] }), 0.5)
  $last = $pts[-1]; $first = $pts[0]
  $p.AddLine((P $last[0] $last[1]), (P $last[0] 1100))
  $p.AddLine((P $last[0] 1100), (P $first[0] 1100))
  $p.CloseFigure()
  $p
}
function Edge($pts) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $p.AddCurve([System.Drawing.PointF[]]($pts | ForEach-Object { P $_[0] $_[1] }), 0.5)
  $p
}

function Eye($g, $cx, $cy, $look) {
  $rx = 94; $ry = 124
  $e = Radial ($cx - 18) ($cy - 30) ($rx * 1.6) ($ry * 1.6) (C 255 255 251 244) (C 255 236 222 206)
  $eye = New-Object System.Drawing.Drawing2D.GraphicsPath
  $eye.AddEllipse($cx - $rx, $cy - $ry, 2 * $rx, 2 * $ry)
  $g.FillPath($e[1], $eye)
  $px = $cx + $look[0]; $py = $cy + $look[1]
  $g.FillEllipse((New-Object System.Drawing.SolidBrush (C 255 24 29 66)), $px - 46, $py - 62, 92, 124)
  $g.FillEllipse((New-Object System.Drawing.SolidBrush (C 255 255 255 255)), $px - 26, $py - 46, 28, 28)
}

# The scene in a 1024 canvas. $simple drops the cursor and zooms in for 16-32 px.
function DrawScene($g, $simple) {
  $sky = New-Object System.Drawing.Drawing2D.LinearGradientBrush (P 0 0), (P 0 760), (C 255 15 19 46), (C 255 46 56 110)
  $g.FillRectangle($sky, 0, 0, 1024, 1024)

  if ($simple) {
    $g.TranslateTransform(512, 560); $g.ScaleTransform(1.32, 1.32); $g.TranslateTransform(-470, -560)
  }

  # warm glow behind the eyes
  $gl = Radial 480 600 360 270 (C 200 255 190 110) (C 0 255 170 90)
  $g.FillPath($gl[1], $gl[0])

  $look = if ($simple) { @(14, -4) } else { @(22, -18) }
  Eye $g 382 530 $look
  Eye $g 578 530 $look

  # blanket: back wave, then front wave, each with a soft lit rim
  $backPts = @(@(-40, 640), @(160, 600), @(360, 640), @(560, 610), @(780, 560), @(1064, 600))
  $back = Wave $backPts
  $bb = New-Object System.Drawing.Drawing2D.LinearGradientBrush (P 0 560), (P 0 1000), (C 255 236 140 92), (C 255 205 104 66)
  $g.FillPath($bb, $back)
  $g.DrawPath((New-Object System.Drawing.Pen (C 150 255 200 150), 7), (Edge $backPts))

  $frontPts = @(@(-40, 700), @(200, 660), @(430, 720), @(660, 690), @(860, 740), @(1064, 700))
  $front = Wave $frontPts
  $fb = New-Object System.Drawing.Drawing2D.LinearGradientBrush (P 0 650), (P 0 1024), (C 255 247 165 110), (C 255 232 128 82)
  $g.FillPath($fb, $front)
  $g.DrawPath((New-Object System.Drawing.Pen (C 170 255 214 170), 8), (Edge $frontPts))

  if (-not $simple) {
    # cursor gliding across the sky, with a little dotted trail
    $dot = New-Object System.Drawing.SolidBrush (C 255 255 255 255)
    foreach ($d in @(@(600, 380, 10, 90), @(650, 345, 13, 150), @(705, 318, 16, 210))) {
      $dot.Color = C $d[3] 255 236 214
      $g.FillEllipse($dot, $d[0] - $d[2], $d[1] - $d[2], 2 * $d[2], 2 * $d[2])
    }
    $cg = Radial 790 330 120 120 (C 90 255 220 170) (C 0 255 220 170)
    $g.FillPath($cg[1], $cg[0])
    $cp = Cursor 748 258 0.56
    $g.FillPath((New-Object System.Drawing.SolidBrush (C 255 255 255 255)), $cp)
    $ol = New-Object System.Drawing.Pen (C 255 24 29 66), 10; $ol.LineJoin = 'Round'
    $g.DrawPath($ol, $cp)
  }
  $g.ResetTransform()
}

function NewCanvas($S) {
  $bmp = New-Object System.Drawing.Bitmap $S, $S
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = 'AntiAlias'; $g.InterpolationMode = 'HighQualityBicubic'; $g.PixelOffsetMode = 'HighQuality'
  $g.Clear([System.Drawing.Color]::Transparent)
  @($bmp, $g)
}

# Render the scene clipped to a squircle tile of half-size $half (1024 canvas).
function Render($half, $simple, $shadow) {
  $scene = NewCanvas 1024
  DrawScene $scene[1] $simple
  $scene[1].Dispose()

  $c = NewCanvas 1024; $bmp = $c[0]; $g = $c[1]
  if ($shadow) {
    for ($k = 1; $k -le 10; $k++) { $g.FillPath((New-Object System.Drawing.SolidBrush (C 8 0 0 20)), (Squircle 512 (522 + $k) ($half + $k) 5)) }
  }
  $tile = Squircle 512 512 $half 5
  $tb = New-Object System.Drawing.TextureBrush $scene[0]
  $s = (2 * $half) / 1024.0
  $tb.TranslateTransform(512 - $half, 512 - $half); $tb.ScaleTransform($s, $s)
  $g.FillPath($tb, $tile)
  $g.DrawPath((New-Object System.Drawing.Pen (C 36 255 255 255), 3), (Squircle 512 512 ($half - 2) 5))
  $g.Dispose()
  $bmp
}
function Scale($src, $S) {
  $cur = $src
  while ($cur.Width / 2 -ge $S * 2) {
    $n = [int]($cur.Width / 2)
    $c = NewCanvas $n; $c[1].DrawImage($cur, 0, 0, $n, $n); $c[1].Dispose(); $cur = $c[0]
  }
  $c = NewCanvas $S; $c[1].DrawImage($cur, 0, 0, $S, $S); $c[1].Dispose()
  $c[0]
}
function PngBytes($bmp) {
  $ms = New-Object System.IO.MemoryStream
  $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
  , $ms.ToArray()
}
# Classic 32-bit DIB icon entry; every part of Windows reads these.
function DibBytes($bmp) {
  $S = $bmp.Width
  $ms = New-Object System.IO.MemoryStream
  $w = New-Object System.IO.BinaryWriter $ms
  $w.Write([uint32]40); $w.Write([int32]$S); $w.Write([int32]($S * 2))
  $w.Write([uint16]1); $w.Write([uint16]32); $w.Write([uint32]0)
  $maskStride = [int]([math]::Ceiling($S / 32) * 4)
  $w.Write([uint32]($S * $S * 4 + $maskStride * $S))
  $w.Write([int32]0); $w.Write([int32]0); $w.Write([uint32]0); $w.Write([uint32]0)
  for ($y = $S - 1; $y -ge 0; $y--) {
    for ($x = 0; $x -lt $S; $x++) {
      $c = $bmp.GetPixel($x, $y)
      $w.Write([byte]$c.B); $w.Write([byte]$c.G); $w.Write([byte]$c.R); $w.Write([byte]$c.A)
    }
  }
  for ($y = $S - 1; $y -ge 0; $y--) {
    $row = New-Object byte[] $maskStride
    for ($x = 0; $x -lt $S; $x++) {
      if ($bmp.GetPixel($x, $y).A -eq 0) { $row[[int][math]::Floor($x / 8)] = $row[[int][math]::Floor($x / 8)] -bor (0x80 -shr ($x % 8)) }
    }
    $w.Write($row)
  }
  $w.Flush()
  , $ms.ToArray()
}

New-Item -ItemType Directory -Force $Dir | Out-Null

if (-not $WindowsOnly) {
  (Render 412 $false $true).Save((Join-Path $Dir "app-icon.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  Write-Host "saved app-icon.png"
  exit 0
}

$full = Render 496 $false $false
$simple = Render 496 $true $false
$entries = foreach ($S in 16, 24, 32, 48, 64, 128, 256) {
  $img = Scale $(if ($S -le 32) { $simple } else { $full }) $S
  @{ Size = $S; Data = $(if ($S -ge 256) { PngBytes $img } else { DibBytes $img }) }
}
(Scale $simple 32).Save((Join-Path $Dir "32x32.png"), [System.Drawing.Imaging.ImageFormat]::Png)

$ms = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $ms
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$entries.Count)
$offset = 6 + 16 * $entries.Count
foreach ($e in $entries) {
  $d = if ($e.Size -ge 256) { 0 } else { $e.Size }
  $w.Write([byte]$d); $w.Write([byte]$d); $w.Write([byte]0); $w.Write([byte]0)
  $w.Write([uint16]1); $w.Write([uint16]32)
  $w.Write([uint32]$e.Data.Length); $w.Write([uint32]$offset)
  $offset += $e.Data.Length
}
foreach ($e in $entries) { $w.Write([byte[]]$e.Data) }
$w.Flush()
[System.IO.File]::WriteAllBytes((Join-Path (Resolve-Path $Dir) "icon.ico"), $ms.ToArray())
Write-Host "saved icon.ico (16-256) and 32x32.png"
