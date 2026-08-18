param(
    [Parameter(Mandatory = $true)][string]$Out,
    [int]$Wait = 0,
    [string]$Window = ""
)

if ($Wait -gt 0) { Start-Sleep -Seconds $Wait }

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Dpi { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }
"@

[void][Dpi]::SetProcessDPIAware()

$area = [System.Windows.Forms.SystemInformation]::VirtualScreen

if ($Window -ne "") {
    Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win {
    [DllImport("user32.dll")] public static extern IntPtr FindWindowW(string c, string n);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
    $handle = [Win]::FindWindowW($null, $Window)

    if ($handle -eq [IntPtr]::Zero) {
        Write-Error "shot: no window titled '$Window'"
        exit 1
    }

    $rect = New-Object Win+RECT
    [void][Win]::GetWindowRect($handle, [ref]$rect)
    $area = New-Object System.Drawing.Rectangle $rect.L, $rect.T, ($rect.R - $rect.L), ($rect.B - $rect.T)
}

$bitmap = New-Object System.Drawing.Bitmap $area.Width, $area.Height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($area.Location, [System.Drawing.Point]::Empty, $bitmap.Size)
$bitmap.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()

Write-Output "shot: wrote $Out ($($area.Width)x$($area.Height))"
