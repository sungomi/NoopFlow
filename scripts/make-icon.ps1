# Draws the NoopFlow app icon (1024x1024 PNG). Regenerate all sizes with:
#   powershell -File scripts/make-icon.ps1 -Out src-tauri/icons/app-icon.png
#   cd src-tauri; cargo tauri icon icons/app-icon.png
param([string]$Out = "src-tauri/icons/app-icon.png")
Add-Type -AssemblyName System.Drawing
$S = 1024
$bmp = New-Object System.Drawing.Bitmap $S, $S
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = 'AntiAlias'
$g.InterpolationMode = 'HighQualityBicubic'
$g.PixelOffsetMode = 'HighQuality'
$g.Clear([System.Drawing.Color]::Transparent)
function C($a, $r, $gr, $b) { [System.Drawing.Color]::FromArgb($a, $r, $gr, $b) }

# Squircle (superellipse) body, Apple-style 100px margin
function Squircle($cx, $cy, $half, $n) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $pts = for ($i = 0; $i -lt 360; $i++) {
    $t = $i * [math]::PI / 180
    $c = [math]::Cos($t); $s = [math]::Sin($t)
    $x = $cx + $half * [math]::Sign($c) * [math]::Pow([math]::Abs($c), 2 / $n)
    $y = $cy + $half * [math]::Sign($s) * [math]::Pow([math]::Abs($s), 2 / $n)
    [System.Drawing.PointF]::new($x, $y)
  }
  $p.AddPolygon($pts)
  return $p
}
function RoundRect($x, $y, $w, $h, $r) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $p.AddArc($x, $y, 2*$r, 2*$r, 180, 90)
  $p.AddArc($x + $w - 2*$r, $y, 2*$r, 2*$r, 270, 90)
  $p.AddArc($x + $w - 2*$r, $y + $h - 2*$r, 2*$r, 2*$r, 0, 90)
  $p.AddArc($x, $y + $h - 2*$r, 2*$r, 2*$r, 90, 90)
  $p.CloseFigure()
  return $p
}

$body = Squircle 512 512 412 5

# drop shadow under the body
for ($k = 1; $k -le 10; $k++) {
  $sh = Squircle 512 (524 + $k) (412 + $k) 5
  $g.FillPath((New-Object System.Drawing.SolidBrush (C 7 20 20 60)), $sh)
}

# gradient body
$grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush ([System.Drawing.PointF]::new(100, 100)), ([System.Drawing.PointF]::new(924, 924)), (C 255 112 84 255), (C 255 18 196 230)
$blend = New-Object System.Drawing.Drawing2D.ColorBlend 3
$blend.Colors = @((C 255 118 82 255), (C 255 64 120 255), (C 255 20 200 232))
$blend.Positions = @(0.0, 0.5, 1.0)
$grad.InterpolationColors = $blend
$g.FillPath($grad, $body)

# soft top glow
$g.SetClip($body)
$glow = New-Object System.Drawing.Drawing2D.GraphicsPath
$glow.AddEllipse(-80, -520, 1184, 980)
$pgb = New-Object System.Drawing.Drawing2D.PathGradientBrush $glow
$pgb.CenterColor = C 70 255 255 255
$pgb.SurroundColors = @((C 0 255 255 255))
$g.FillPath($pgb, $glow)
$g.ResetClip()

# back screen (left, translucent)
$back = RoundRect 168 340 316 222 38
$g.FillPath((New-Object System.Drawing.SolidBrush (C 60 255 255 255)), $back)
$pb = New-Object System.Drawing.Pen (C 170 255 255 255), 30
$pb.LineJoin = 'Round'
$g.DrawPath($pb, $back)
$stand1 = New-Object System.Drawing.Pen (C 170 255 255 255), 30
$stand1.StartCap = 'Round'; $stand1.EndCap = 'Round'
$g.DrawLine($stand1, 276, 616, 376, 616)

# front screen (right, solid)
for ($k = 1; $k -le 8; $k++) {
  $g.FillPath((New-Object System.Drawing.SolidBrush (C 10 10 20 80)), (RoundRect (540 + $k) (340 + 2*$k) 316 222 38))
}
$front = RoundRect 540 340 316 222 38
$g.FillPath((New-Object System.Drawing.SolidBrush (C 255 255 255 255)), $front)
$inner = RoundRect 562 362 272 178 22
$ig = New-Object System.Drawing.Drawing2D.LinearGradientBrush ([System.Drawing.PointF]::new(562, 362)), ([System.Drawing.PointF]::new(834, 540)), (C 255 232 230 255), (C 255 214 246 252)
$g.FillPath($ig, $inner)
$stand2 = New-Object System.Drawing.Pen (C 255 255 255 255), 30
$stand2.StartCap = 'Round'; $stand2.EndCap = 'Round'
$g.DrawLine($stand2, 648, 616, 748, 616)

# cursor crossing into the front screen
$ox = 486; $oy = 452; $k = 0.8
$shape = @(@(0,0), @(0,300), @(75,232), @(128,346), @(180,322), @(128,212), @(226,210))
for ($s = 1; $s -le 8; $s++) {
  $spts = $shape | ForEach-Object { [System.Drawing.PointF]::new($ox + 6 + $_[0]*$k, $oy + 10 + $s + $_[1]*$k) }
  $sp = New-Object System.Drawing.Drawing2D.GraphicsPath; $sp.AddPolygon($spts)
  $g.FillPath((New-Object System.Drawing.SolidBrush (C 12 15 15 60)), $sp)
}
$pts = $shape | ForEach-Object { [System.Drawing.PointF]::new($ox + $_[0]*$k, $oy + $_[1]*$k) }
$cp = New-Object System.Drawing.Drawing2D.GraphicsPath
$cp.AddPolygon($pts)
$outline = New-Object System.Drawing.Pen (C 255 58 52 200), 26
$outline.LineJoin = 'Round'
$g.DrawPath($outline, $cp)
$g.FillPath((New-Object System.Drawing.SolidBrush (C 255 255 255 255)), $cp)

$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host "saved $Out"

