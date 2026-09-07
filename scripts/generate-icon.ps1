# Repository-native OXY mark. Generates PNG + a multi-resolution Windows ICO.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$destination = Join-Path (Split-Path $PSScriptRoot -Parent) 'assets/branding'
New-Item -ItemType Directory -Path $destination -Force | Out-Null
$images = @()
foreach ($size in @(16, 24, 32, 48, 64, 128, 256)) {
    $bitmap = [Drawing.Bitmap]::new($size, $size)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.SmoothingMode = 'AntiAlias'
    $graphics.ScaleTransform($size / 256.0, $size / 256.0)
    $background = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(255, 22, 29, 36))
    $accent = [Drawing.Pen]::new([Drawing.Color]::FromArgb(255, 131, 224, 207), 14)
    $ink = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(255, 238, 249, 246))
    $points = [Drawing.PointF[]]@([Drawing.PointF]::new(128,15),[Drawing.PointF]::new(226,71),[Drawing.PointF]::new(226,185),[Drawing.PointF]::new(128,241),[Drawing.PointF]::new(30,185),[Drawing.PointF]::new(30,71))
    $graphics.FillPolygon($background, $points)
    $graphics.DrawPolygon($accent, $points)
    $font = [Drawing.Font]::new('Arial', 56, [Drawing.FontStyle]::Bold, [Drawing.GraphicsUnit]::Pixel)
    $format = [Drawing.StringFormat]::new()
    $format.Alignment = 'Center'
    $format.LineAlignment = 'Center'
    $graphics.DrawString('OXY', $font, $ink, [Drawing.RectangleF]::new(16,16,224,224), $format)
    $stream = [IO.MemoryStream]::new()
    $bitmap.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
    $images += ,$stream.ToArray()
    if ($size -eq 256) { [IO.File]::WriteAllBytes((Join-Path $destination 'oxy.png'), $stream.ToArray()) }
    $format.Dispose(); $font.Dispose(); $ink.Dispose(); $accent.Dispose(); $background.Dispose(); $graphics.Dispose(); $bitmap.Dispose(); $stream.Dispose()
}
$file = [IO.File]::Create((Join-Path $destination 'oxy.ico'))
$writer = [IO.BinaryWriter]::new($file)
try {
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$images.Count)
    $offset = 6 + 16 * $images.Count
    $sizes = @(16,24,32,48,64,128,0)
    for ($i = 0; $i -lt $images.Count; $i++) {
        $writer.Write([byte]$sizes[$i]); $writer.Write([byte]$sizes[$i]); $writer.Write([byte]0); $writer.Write([byte]0)
        $writer.Write([uint16]1); $writer.Write([uint16]32); $writer.Write([uint32]$images[$i].Length); $writer.Write([uint32]$offset)
        $offset += $images[$i].Length
    }
    foreach ($bytes in $images) { $writer.Write([byte[]]$bytes) }
} finally { $writer.Dispose(); $file.Dispose() }
