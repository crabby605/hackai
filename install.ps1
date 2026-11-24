# HackAI Windows installer
# Usage: iwr https://raw.githubusercontent.com/crabby605/hackai/main/install.ps1 | iex

$repo = "crabby605/hackai"
$binary = "hackai.exe"

# Artifact name must match the release-cli build artifact (release-cli.yml).
$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -eq "AMD64") { $artifact = "hackai-windows-x86_64.exe" }
else { Write-Error "Unsupported architecture: $arch"; exit 1 }

$version = (Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest").tag_name
if (-not $version) { Write-Error "Failed to fetch latest version"; exit 1 }

$url = "https://github.com/$repo/releases/download/$version/$artifact"
$checksumUrl = "${url}.sha256"

Write-Host "Installing $binary $version for $artifact..."

$tmpDir = [System.IO.Path]::GetTempPath()
$tmpFile = Join-Path $tmpDir $binary
$tmpChecksum = Join-Path $tmpDir "$binary.sha256"

# Download binary
Invoke-WebRequest -Uri $url -OutFile $tmpFile

# Verify SHA-256 checksum (required — refuse to install without it)
Invoke-WebRequest -Uri $checksumUrl -OutFile $tmpChecksum
$expected = (Get-Content $tmpChecksum).Split()[0]
$actual = (Get-FileHash $tmpFile -Algorithm SHA256).Hash.ToLower()
if ($expected -ne $actual) {
  Write-Error "Checksum mismatch: expected $expected, got $actual"
  exit 1
}

$installDir = if ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) {
  "C:\Program Files\HackAI"
} else {
  "$env:USERPROFILE\.local\bin"
}

if (-not (Test-Path $installDir)) { New-Item -ItemType Directory -Force -Path $installDir }
Move-Item $tmpFile -Destination (Join-Path $installDir $binary) -Force

# Marker file so `hackai update` knows this binary was installed by the
# release installer and is eligible for self-update.
New-Item -ItemType File -Path (Join-Path $installDir "$binary.release") -Force | Out-Null

$env:PATH = "$installDir;$env:PATH"
[Environment]::SetEnvironmentVariable("PATH", $env:PATH, "User")

Write-Host "Installed $binary to $installDir"
Write-Host "Run 'hackai --help' to get started"
