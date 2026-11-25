# Builds the desktop app and puts a shortcut on the Desktop (or Start Menu).

[CmdletBinding()]
param(
    [string]$Destination,
    [switch]$StartMenu,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrEmpty($env:CARGO_TARGET_DIR)) {
    $env:CARGO_TARGET_DIR = Join-Path $root 'target'
}

$exe = Join-Path $root 'target\release\mappamente-desktop.exe'
if (-not (Test-Path $exe)) {
    if ($SkipBuild) {
        throw "Missing $exe. Build it first or omit -SkipBuild."
    }
    Write-Host 'Building mappamente-desktop (release)...'
    Push-Location $root
    try {
        cargo build --release --bin mappamente-desktop
    }
    finally {
        Pop-Location
    }
}
if (-not (Test-Path $exe)) {
    throw "The build did not produce $exe"
}

if ($StartMenu) {
    $folder = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Microsoft\Windows\Start Menu\Programs'
}
elseif ($Destination) {
    $folder = $Destination
}
else {
    $folder = [Environment]::GetFolderPath('Desktop')
}

if (-not (Test-Path $folder)) {
    New-Item -ItemType Directory -Path $folder | Out-Null
}

$link = Join-Path $folder 'Mappamente.lnk'
$icon = Join-Path $root 'assets\mappamente.ico'
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($link)
$shortcut.TargetPath = $exe
$shortcut.WorkingDirectory = $root
$shortcut.Description = 'Mappamente - lightweight mind maps'
$shortcut.IconLocation = if (Test-Path $icon) { "$icon,0" } else { "$exe,0" }
$shortcut.WindowStyle = 1
$shortcut.Save()

Write-Host "Shortcut created: $link"
Write-Host "Target: $exe"
