param(
    [Parameter(Mandatory = $true)][string]$Title,
    [Parameter(Mandatory = $true)][string]$Out,
    [switch]$NoFocus
)

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Shot {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string n);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, System.Text.StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    public delegate bool EnumProc(IntPtr h, IntPtr p);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public static IntPtr ByTitle(string wanted) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr h, IntPtr p) {
            if (!IsWindowVisible(h)) return true;
            var sb = new System.Text.StringBuilder(512);
            GetWindowTextW(h, sb, 512);
            if (sb.ToString() == wanted) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@

[void][Shot]::SetProcessDPIAware()

$handle = [Shot]::ByTitle($Title)

if ($handle -eq [IntPtr]::Zero) {
    Write-Error "shot-window: no window titled '$Title'"
    exit 1
}

if (-not $NoFocus) {
    [void][Shot]::ShowWindow($handle, 9)
    [void][Shot]::SetForegroundWindow($handle)
    Start-Sleep -Milliseconds 900
}

$rect = New-Object Shot+RECT
[void][Shot]::GetWindowRect($handle, [ref]$rect)

$w = $rect.R - $rect.L
$h = $rect.B - $rect.T

$bitmap = New-Object System.Drawing.Bitmap $w, $h
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($rect.L, $rect.T, 0, 0, $bitmap.Size)
$bitmap.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()

Write-Output "shot-window: $Out ($w x $h) title '$Title'"
