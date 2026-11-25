# Embeds assets/mappamente.ico into a Windows executable.
# Usage: .\embed-icon.ps1 -Exe .\Mappamente.exe -Icon .\assets\mappamente.ico

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Exe,
    [Parameter(Mandatory)][string]$Icon
)

$ErrorActionPreference = 'Stop'

Add-Type @'
using System;
using System.Runtime.InteropServices;

public static class MappamenteResourceUpdate
{
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern IntPtr BeginUpdateResource(string fileName, bool deleteExistingResources);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool UpdateResource(
        IntPtr update,
        IntPtr type,
        IntPtr name,
        ushort language,
        [In] byte[] data,
        uint dataSize);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool EndUpdateResource(IntPtr update, bool discard);
}
'@

$exePath = (Resolve-Path $Exe).Path
$iconPath = (Resolve-Path $Icon).Path
$bytes = [System.IO.File]::ReadAllBytes($iconPath)
if ($bytes.Length -lt 6) {
    throw "Invalid icon file: $iconPath"
}

$count = [BitConverter]::ToUInt16($bytes, 4)
if ($count -eq 0 -or $bytes.Length -lt (6 + $count * 16)) {
    throw "The icon has no usable images: $iconPath"
}

$images = @()
for ($index = 0; $index -lt $count; $index++) {
    $offset = 6 + $index * 16
    $length = [BitConverter]::ToUInt32($bytes, $offset + 8)
    $imageOffset = [BitConverter]::ToUInt32($bytes, $offset + 12)
    if ($imageOffset + $length -gt $bytes.Length) {
        throw "Corrupt icon entry $index in $iconPath"
    }
    $data = New-Object byte[] $length
    [Array]::Copy($bytes, $imageOffset, $data, 0, $length)
    $images += [pscustomobject]@{
        Width      = $bytes[$offset]
        Height     = $bytes[$offset + 1]
        ColorCount = $bytes[$offset + 2]
        Reserved   = $bytes[$offset + 3]
        Planes     = [BitConverter]::ToUInt16($bytes, $offset + 4)
        BitCount   = [BitConverter]::ToUInt16($bytes, $offset + 6)
        Length     = $length
        Data       = $data
    }
}

$update = [MappamenteResourceUpdate]::BeginUpdateResource($exePath, $false)
if ($update -eq [IntPtr]::Zero) {
    throw "BeginUpdateResource failed for $exePath (error $([Runtime.InteropServices.Marshal]::GetLastWin32Error()))"
}

try {
    for ($index = 0; $index -lt $images.Count; $index++) {
        $id = $index + 1
        $image = $images[$index]
        $ok = [MappamenteResourceUpdate]::UpdateResource(
            $update,
            [IntPtr]3,          # RT_ICON
            [IntPtr]$id,
            0,
            $image.Data,
            [uint32]$image.Length)
        if (-not $ok) {
            throw "UpdateResource RT_ICON $id failed (error $([Runtime.InteropServices.Marshal]::GetLastWin32Error()))"
        }
    }

    $group = New-Object byte[] (6 + 14 * $images.Count)
    [Array]::Copy([BitConverter]::GetBytes([uint16]0), 0, $group, 0, 2)
    [Array]::Copy([BitConverter]::GetBytes([uint16]1), 0, $group, 2, 2)
    [Array]::Copy([BitConverter]::GetBytes([uint16]$images.Count), 0, $group, 4, 2)
    $position = 6
    for ($index = 0; $index -lt $images.Count; $index++) {
        $image = $images[$index]
        $group[$position] = $image.Width
        $group[$position + 1] = $image.Height
        $group[$position + 2] = $image.ColorCount
        $group[$position + 3] = $image.Reserved
        [Array]::Copy([BitConverter]::GetBytes([uint16]$image.Planes), 0, $group, $position + 4, 2)
        [Array]::Copy([BitConverter]::GetBytes([uint16]$image.BitCount), 0, $group, $position + 6, 2)
        [Array]::Copy([BitConverter]::GetBytes([uint32]$image.Length), 0, $group, $position + 8, 4)
        [Array]::Copy([BitConverter]::GetBytes([uint16]($index + 1)), 0, $group, $position + 12, 2)
        $position += 14
    }

    $ok = [MappamenteResourceUpdate]::UpdateResource(
        $update,
        [IntPtr]14,         # RT_GROUP_ICON
        [IntPtr]1,
        0,
        $group,
        [uint32]$group.Length)
    if (-not $ok) {
        throw "UpdateResource RT_GROUP_ICON failed (error $([Runtime.InteropServices.Marshal]::GetLastWin32Error()))"
    }
}
catch {
    [MappamenteResourceUpdate]::EndUpdateResource($update, $true) | Out-Null
    throw
}

if (-not [MappamenteResourceUpdate]::EndUpdateResource($update, $false)) {
    throw "EndUpdateResource failed for $exePath (error $([Runtime.InteropServices.Marshal]::GetLastWin32Error()))"
}

Write-Host "Embedded $($images.Count) icon image(s) into $exePath"
