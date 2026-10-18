# Raise a CI runner's desktop to WIDTHxHEIGHT for the native GUI tests.
#
# Usage: pwsh -File set-display-resolution.ps1 [-Width 1920] [-Height 1080] [-WhatIf]
#
# Hosted Windows runners start at 1024x768, smaller than the sidebar fixture's
# largest window sizes. Best effort: if the display refuses the mode, the
# fixture skips and counts the sizes that do not fit rather than failing.
# Changes the current session's display, so it is for CI runners only; with
# -WhatIf it only reports the current mode.
param([int]$Width = 1920, [int]$Height = 1080, [switch]$WhatIf)

Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
public static class HerdrDisplay {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct DEVMODE {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
        public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra;
        public int dmFields;
        // The display arm of the union: dmPosition, orientation, fixed output.
        public int dmPositionX, dmPositionY, dmDisplayOrientation, dmDisplayFixedOutput;
        public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
        public short dmLogPixels;
        public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
        public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2;
        public int dmPanningWidth, dmPanningHeight;
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern bool EnumDisplaySettings(string device, int mode, ref DEVMODE devMode);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int ChangeDisplaySettings(ref DEVMODE devMode, int flags);
}
'@

$mode = New-Object HerdrDisplay+DEVMODE
$mode.dmSize = [System.Runtime.InteropServices.Marshal]::SizeOf($mode)
# ENUM_CURRENT_SETTINGS
if (-not [HerdrDisplay]::EnumDisplaySettings([NullString]::Value, -1, [ref]$mode)) {
    Write-Output "No display settings to read; leaving the resolution alone"
    exit 0
}
Write-Output "Current display: $($mode.dmPelsWidth)x$($mode.dmPelsHeight)"
if ($WhatIf) { exit 0 }
$mode.dmPelsWidth = $Width
$mode.dmPelsHeight = $Height
# DM_PELSWIDTH | DM_PELSHEIGHT
$mode.dmFields = 0x80000 -bor 0x100000
# 0 is DISP_CHANGE_SUCCESSFUL; anything else leaves the old mode in place.
$result = [HerdrDisplay]::ChangeDisplaySettings([ref]$mode, 0)
[HerdrDisplay]::EnumDisplaySettings([NullString]::Value, -1, [ref]$mode) | Out-Null
Write-Output "ChangeDisplaySettings returned $result; display is now $($mode.dmPelsWidth)x$($mode.dmPelsHeight)"
