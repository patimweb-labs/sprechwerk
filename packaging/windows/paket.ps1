# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
#
# Windows-Pakete für die **zwei getrennten Anwendungen**:
#
#   Client  „Sprechwerk"          sprechwerk-fenster.exe, sprechwerk-cli.exe
#                                 -> .msi und .zip
#   Server  „Sprechwerk Server"   sprechwerk.exe, sprechwerk-server-fenster.exe
#                                 -> .zip (kein .msi: der Dienst wird nicht
#                                    als Windows-Dienst eingetragen)
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

# --- Client ----------------------------------------------------------------
wix build packaging\windows\sprechwerk.wxs -arch x64 -d Version=$version -o "dist\Sprechwerk-$version-windows-x64.msi"
if ($LASTEXITCODE -ne 0) { throw "wix build" }

$client = "dist\Sprechwerk-$version-windows-x64"
New-Item -ItemType Directory -Force $client | Out-Null
Copy-Item target\release\sprechwerk-fenster.exe, target\release\sprechwerk-cli.exe,
          packaging\opus\opus.dll, LICENSE $client
Compress-Archive -Force -Path "$client\*" -DestinationPath "$client.zip"
Remove-Item -Recurse $client

# --- Server ----------------------------------------------------------------
# Hier entsteht **kein** Server-Paket: `server.yml` baut das Serverarchiv für
# alle vier Systeme (mit README und Beispielkonfiguration). Zwei Quellen für
# dieselbe Datei hießen zwei Namen im Release und ein Überschreiben beim
# Hochladen.

Get-ChildItem dist
