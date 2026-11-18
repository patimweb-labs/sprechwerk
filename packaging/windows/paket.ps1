# Windows: .msi (WiX v5) und ein portables .zip.
#
# Aufruf aus dem Wurzelverzeichnis: pwsh packaging/windows/paket.ps1
# Braucht: cargo (MSVC), WiX v5 (`dotnet tool install --global wix`),
#          packaging/opus/opus.dll (siehe pakete.yml).
# Unsigniert, solange kein Zertifikat hinterlegt ist (SmartScreen fragt).
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$env:RUSTFLAGS = "-C target-feature=+crt-static"

cargo build --release --locked -p sprechwerk-fenster -p sprechwerk-server-fenster -p sprechwerk-server -p sprechwerk-cli
if ($LASTEXITCODE -ne 0) { throw "cargo build" }

New-Item -ItemType Directory -Force dist | Out-Null
wix build packaging\windows\sprechwerk.wxs -arch x64 -d Version=$version -o "dist\Sprechwerk-$version-windows-x64.msi"
if ($LASTEXITCODE -ne 0) { throw "wix build" }

$tragbar = "dist\Sprechwerk-$version-windows-x64"
New-Item -ItemType Directory -Force $tragbar | Out-Null
Copy-Item target\release\sprechwerk-fenster.exe, target\release\sprechwerk-server-fenster.exe,
          target\release\sprechwerk.exe, target\release\sprechwerk-cli.exe,
          packaging\opus\opus.dll, LICENSE $tragbar
Compress-Archive -Force -Path "$tragbar\*" -DestinationPath "$tragbar.zip"
Remove-Item -Recurse $tragbar
Get-ChildItem dist
