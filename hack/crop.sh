#!/usr/bin/env bash
set -u
src="${1:?src}"; x="${2:?x}"; y="${3:?y}"; w="${4:?w}"; h="${5:?h}"; out="${6:?out}"
s=$(wslpath -w "$src"); o=$(wslpath -w "$out")
powershell.exe -NoProfile -Command "Add-Type -AssemblyName System.Drawing; \$src=[System.Drawing.Image]::FromFile('$s'); \$crop=New-Object System.Drawing.Rectangle $x,$y,$w,$h; \$bmp=New-Object System.Drawing.Bitmap $w,$h; \$g=[System.Drawing.Graphics]::FromImage(\$bmp); \$g.DrawImage(\$src,(New-Object System.Drawing.Rectangle 0,0,$w,$h),\$crop,'Pixel'); \$bmp.Save('$o'); \$bmp.Dispose(); \$src.Dispose()" >/dev/null 2>&1
echo "crop: $out"
