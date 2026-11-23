# HackAI Windows installer
# Usage: iwr https://raw.githubusercontent.com/hackclub/hackai/main/install.ps1 | iex

$repo = "crabby605/hackai"
$binary = "hackai.exe"

$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -eq "AMD64") { $target = "x86_64-pc-windows-msvc" }
elseif ($arch -eq "ARM64") { $target = "aarch64-pc-windows-msvc" }
else { Write-Error "Unsupported architecture: $arch"; exit 1 }

$version = (Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest").tag_name
if (-not $version) { Write-Error "Failed to fetch latest version"; exit 1 }

$url = "https://github.com/$repo/releases/download/$version/hackai-$target.exe"
Write-Host "Installing $binary $version for $target..."

$tmpDir = [System.IO.Path]::GetTempPath()
$tmpFile = Join-Path $tmpDir $binary

Invoke-WebRequest -Uri $url -OutFile $tmpFile

$installDir = if ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) {
  "C:\Program Files\HackAI"
} else {
  "$env:USERPROFILE\.local\bin"
}

if (-not (Test-Path $installDir)) { New-Item -ItemType Directory -Force -Path $installDir }
Move-Item $tmpFile -Destination (Join-Path $installDir $binary) -Force

$env:PATH = "$installDir;$env:PATH"
[Environment]::SetEnvironmentVariable("PATH", $env:PATH, "User")

Write-Host "Installed $binary to $installDir"
Write-Host "Run 'hackai --help' to get started"