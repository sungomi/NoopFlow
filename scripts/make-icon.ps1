# Draws the NoopFlow icons in the NooP family style (navy tile, periwinkle accent).
#   powershell -File scripts/make-icon.ps1
# Writes src-tauri/icons/app-icon.png (macOS, 1024 with Apple margin), then run
#   cd src-tauri; cargo tauri icon icons/app-icon.png
# and run this script again with -WindowsOnly to replace icon.ico / 32x32.png with
# Windows-tuned versions (full-bleed tile, simplified glyph at 16-32 px).
param([string]$Dir = "src-tauri/icons", [switch]$WindowsOnly)
Add-Type -AssemblyName System.Drawing

function C($a, $r, $g, $b) { [System.Drawing.Color]::FromArgb($a, $r, $g, $b) }
function P($x, $y) { [System.Drawing.PointF]::new($x, $y) }

$NAVY_TOP = C 255 38 43 78
$NAVY_BOT = C 255 18 21 38
$PERI = C 255 110 123 255
$PERI_DEEP = C 255 84 96 245
$LAV = C 255 190 198 255

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
function RoundRect($x, $y, $w, $h, $r) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $p.AddArc($x, $y, 2*$r, 2*$r, 180, 90)
  $p.AddArc($x + $w - 2*$r, $y, 2*$r, 2*$r, 270, 90)
  $p.AddArc($x + $w - 2*$r, $y + $h - 2*$r, 2*$r, 2*$r, 0, 90)
  $p.AddArc($x, $y + $h - 2*$r, 2*$r, 2*$r, 90, 90)
  $p.CloseFigure()
  $p
}
function Cursor($ox, $oy, $k) {
  $shape = @(@(0,0), @(0,300), @(75,232), @(128,346), @(180,322), @(128,212), @(226,210))
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $p.AddPolygon(($shape | ForEach-Object { P ($ox + $_[0]*$k) ($oy + $_[1]*$k) }))
  $p
}
function NewCanvas($S) {
  $bmp = New-Object System.Drawing.Bitmap $S, $S
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = 'AntiAlias'; $g.InterpolationMode = 'HighQualityBicubic'; $g.PixelOffsetMode = 'HighQuality'
  $g.Clear([System.Drawing.Color]::Transparent)
  @($bmp, $g)
}

# Tile: navy gradient, periwinkle glow, hairline highlight. $half = half size of the tile in a 1024 canvas.
function DrawTile($g, $half, $shadow) {
  $body = Squircle 512 512 $half 5
  if ($shadow) {
    for ($k = 1; $k -le 10; $k++) { $g.FillPath((New-Object System.Drawing.SolidBrush (C 8 0 0 20)), (Squircle 512 (522 + $k) ($half + $k) 5)) }
  }
  $grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush (P 0 (512 - $half)), (P 0 (512 + $half)), $NAVY_TOP, $NAVY_BOT
  $g.FillPath($grad, $body)
  $g.SetClip($body)
  $glow = New-Object System.Drawing.Drawing2D.GraphicsPath
  $glow.AddEllipse(152, 170, 720, 620)
  $pgb = New-Object System.Drawing.Drawing2D.PathGradientBrush $glow
  $pgb.CenterColor = C 90 110 123 255
  $pgb.SurroundColors = @((C 0 110 123 255))
  $g.FillPath($pgb, $glow)
  $g.ResetClip()
  $g.DrawPath((New-Object System.Drawing.Pen (C 40 255 255 255), 4), (Squircle 512 512 ($half - 2) 5))
}

# Full glyph: dim left screen, bright periwinkle right screen, white cursor crossing.
function DrawGlyph($g) {
  $back = RoundRect 176 350 306 214 36
  $g.FillPath((New-Object System.Drawing.SolidBrush (C 40 190 198 255)), $back)
  $pb = New-Object System.Drawing.Pen (C 150 190 198 255), 28; $pb.LineJoin = 'Round'
  $g.DrawPath($pb, $back)
  $st = New-Object System.Drawing.Pen (C 150 190 198 255), 28; $st.StartCap = 'Round'; $st.EndCap = 'Round'
  $g.DrawLine($st, 280, 618, 378, 618)

  $front = RoundRect 542 350 306 214 36
  for ($k = 1; $k -le 8; $k++) { $g.FillPath((New-Object System.Drawing.SolidBrush (C 14 0 0 30)), (RoundRect 542 (352 + 2*$k) 306 214 36)) }
  $fg = New-Object System.Drawing.Drawing2D.LinearGradientBrush (P 542 350), (P 848 564), $PERI, $PERI_DEEP
  $g.FillPath($fg, $front)
  $g.FillPath((New-Object System.Drawing.SolidBrush (C 70 255 255 255)), (RoundRect 568 374 254 166 20))
  $st2 = New-Object System.Drawing.Pen $PERI, 28; $st2.StartCap = 'Round'; $st2.EndCap = 'Round'
  $g.DrawLine($st2, 646, 618, 744, 618)

  for ($s = 1; $s -le 8; $s++) { $g.FillPath((New-Object System.Drawing.SolidBrush (C 16 0 0 20)), (Cursor 494 (464 + $s) 0.78)) }
  $cp = Cursor 488 452 0.78
  $ol = New-Object System.Drawing.Pen $NAVY_BOT, 26; $ol.LineJoin = 'Round'
  $g.DrawPath($ol, $cp)
  $g.FillPath((New-Object System.Drawing.SolidBrush (C 255 255 255 255)), $cp)
}

# Simplified glyph for 16-32 px: one bold periwinkle screen + big white cursor.
function DrawSimpleGlyph($g) {
  $scr = RoundRect 190 250 560 400 70
  $g.FillPath((New-Object System.Drawing.SolidBrush $PERI), $scr)
  $cp = Cursor 520 470 1.35
  $ol = New-Object System.Drawing.Pen $NAVY_BOT, 60; $ol.LineJoin = 'Round'
  $g.DrawPath($ol, $cp)
  $g.FillPath((New-Object System.Drawing.SolidBrush (C 255 255 255 255)), $cp)
}

function Render($half, $simple, $shadow) {
  $c = NewCanvas 1024; $bmp = $c[0]; $g = $c[1]
  DrawTile $g $half $shadow
  if ($simple) { DrawSimpleGlyph $g } else { DrawGlyph $g }
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
# Classic 32-bit DIB icon entry (BITMAPINFOHEADER + BGRA bottom-up + AND mask);
# every part of Windows reads these, unlike PNG-compressed entries.
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
  $mac = Render 412 $false $true
  $mac.Save((Join-Path $Dir "app-icon.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  Write-Host "saved app-icon.png"
  exit 0
}

# Windows: nearly full-bleed tile; simplified glyph below 48 px.
$full = Render 492 $false $false
$simple = Render 492 $true $false
$entries = foreach ($S in 16, 24, 32, 48, 64, 128, 256) {
  $src = if ($S -le 32) { $simple } else { $full }
  $img = Scale $src $S
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
