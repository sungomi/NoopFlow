# Serves ui/ on http://localhost:5178 for previewing the interface in a browser
# (no Tauri backend: the page falls back to sample data).
param([int]$Port = 5178)
$root = Join-Path $PSScriptRoot '..\ui' | Resolve-Path
$types = @{ '.html' = 'text/html; charset=utf-8'; '.css' = 'text/css; charset=utf-8'; '.js' = 'text/javascript; charset=utf-8'; '.png' = 'image/png'; '.svg' = 'image/svg+xml' }
$l = New-Object System.Net.HttpListener
$l.Prefixes.Add("http://localhost:$Port/")
$l.Start()
Write-Host "UI preview on http://localhost:$Port/"
while ($l.IsListening) {
  $ctx = $l.GetContext()
  $path = $ctx.Request.Url.AbsolutePath.TrimStart('/')
  if (-not $path) { $path = 'index.html' }
  $file = Join-Path $root $path
  if ((Test-Path $file -PathType Leaf) -and ((Resolve-Path $file).Path.StartsWith($root.Path))) {
    $bytes = [System.IO.File]::ReadAllBytes($file)
    $ext = [System.IO.Path]::GetExtension($file)
    $ctx.Response.ContentType = $types[$ext]
    $ctx.Response.Headers.Add('Cache-Control', 'no-store')
    $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
  } else {
    $ctx.Response.StatusCode = 404
  }
  $ctx.Response.Close()
}
