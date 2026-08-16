# Generates crisp native-size tray/window icons from the whale SVG path
# (dist/favicon.svg, same geometry as the titlebar logo). Windows scales a
# single-size HICON to the tray slot, so we render the vector art AT the exact
# slot size (16@100% / 20@125% / 24@150% / 28@175% / 32@200% DPI) instead of
# downscaling one large bitmap — this is what fixes the blurry tray icon.
#
# Outputs: src-tauri/icons/tray-16.png / tray-20.png / tray-24.png / tray-28.png / tray-32.png
# Also validates the renderer against the 512px master (icons/icon.png).

Add-Type -AssemblyName System.Drawing

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$srcTauri = Join-Path $root 'src-tauri'
$iconsDir = Join-Path $srcTauri 'icons'
$svgPath = Join-Path $root 'dist\favicon.svg'
$masterPath = Join-Path $iconsDir 'icon.png'

# ---- 1. parse the SVG path (M / C / Z only) ----
$svg = Get-Content $svgPath -Raw
$idx = $svg.IndexOf('d="M') + 3
$end = $svg.IndexOf('"', $idx)
$d = $svg.Substring($idx, $end - $idx)

$tokens = [regex]::Matches($d, '[MCZ]|-?\d+\.?\d*') | ForEach-Object { $_.Value }
$cmds = New-Object System.Collections.ArrayList  # each: type + float coords
$i = 0
while ($i -lt $tokens.Count) {
  $t = $tokens[$i]
  if ($t -eq 'M' -or $t -eq 'C' -or $t -eq 'Z') {
    $need = if ($t -eq 'M') { 2 } elseif ($t -eq 'C') { 6 } else { 0 }
    $coords = @()
    for ($j = 0; $j -lt $need; $j++) { $i++; $coords += [double]$tokens[$i] }
    [void]$cmds.Add(@{ type = $t; coords = $coords })
  } else {
    throw "unexpected token: $t"
  }
  $i++
}

# ---- 2. build a GDI+ GraphicsPath from the commands ----
function Build-WhalePath([float]$scale) {
  $path = New-Object System.Drawing.Drawing2D.GraphicsPath
  $cur = @(0.0, 0.0)
  $start = @(0.0, 0.0)
  foreach ($c in $cmds) {
    switch ($c.type) {
      'M' {
        if ($path.PointCount -gt 0) { $path.StartFigure() }
        $cur = @([float]($c.coords[0] * $scale), [float]($c.coords[1] * $scale))
        $start = $cur
        $path.AddLine($cur[0], $cur[1], $cur[0], $cur[1])
      }
      'C' {
        $c1 = @([float]($c.coords[0] * $scale), [float]($c.coords[1] * $scale))
        $c2 = @([float]($c.coords[2] * $scale), [float]($c.coords[3] * $scale))
        $p1 = @([float]($c.coords[4] * $scale), [float]($c.coords[5] * $scale))
        $path.AddBezier($cur[0], $cur[1], $c1[0], $c1[1], $c2[0], $c2[1], $p1[0], $p1[1])
        $cur = $p1
      }
      'Z' {
        $path.CloseFigure()
        $cur = $start
      }
    }
  }
  return $path
}

# ---- 3. render at a given size (black silhouette, anti-aliased, thin stroke
#          to keep the body from thinning out at tiny sizes) ----
function Render-Whale([int]$size) {
  $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.Clear([System.Drawing.Color]::Transparent)
  $path = Build-WhalePath ([float]($size / 50.0))
  $brush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 0, 0, 0))
  $g.FillPath($brush, $path)
  # thin same-color stroke keeps small silhouettes from looking washed out
  $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 0, 0, 0), [float]0.9)
  $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
  $g.DrawPath($pen, $path)
  $pen.Dispose(); $brush.Dispose(); $path.Dispose(); $g.Dispose()
  return $bmp
}

# ---- 4. validate the renderer against the 512 master ----
$master = New-Object System.Drawing.Bitmap($masterPath)
$probe = Render-Whale 512
$mismatch = 0; $checked = 0
for ($y = 0; $y -lt 512; $y += 4) {
  for ($x = 0; $x -lt 512; $x += 4) {
    $checked++
    $a1 = $master.GetPixel($x, $y).A
    $a2 = $probe.GetPixel($x, $y).A
    if ([Math]::Abs($a1 - $a2) -gt 40) { $mismatch++ }
  }
}
$probe.Dispose()
Write-Host ("validation: {0} mismatches of {1} samples ({2:P1})" -f $mismatch, $checked, ($mismatch / $checked))
$master.Dispose()
if ($mismatch -gt ($checked * 0.05)) { throw 'renderer deviates too much from master icon.png' }

# ---- 5. emit the tray sizes ----
foreach ($size in 16, 20, 24, 28, 32) {
  $bmp = Render-Whale $size
  $out = Join-Path $iconsDir ("tray-{0}.png" -f $size)
  $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  Write-Host "wrote $out"
}
Write-Host 'done'
