#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
#
# macOS-Pakete für die **zwei getrennten Anwendungen**, als Universal-Binary
# (Apple Silicon + Intel):
#
#   Client  „Sprechwerk"          Sprechwerk.app        -> eigenes .dmg
#                                 sprechwerk-cli        -> .zip
#   Server  „Sprechwerk Server"   Sprechwerk Server.app -> eigenes .dmg
#
# Den Konsolen-Server für alle vier Systeme baut `server.yml`; hier entstehen
# nur die Bündel, die ein Doppelklick startet.
#
# Aufruf aus dem Wurzelverzeichnis auf macOS: packaging/macos/paket.sh
# Braucht: rustup-Ziele aarch64-apple-darwin und x86_64-apple-darwin,
#          packaging/opus/libopus.0.dylib (universal, siehe pakete.yml).
# Unsigniert, solange keine Zertifikate hinterlegt sind (Gatekeeper fragt).
set -euo pipefail
cd "$(dirname "$0")/../.."
VERSION=$(packaging/version.sh)
export MACOSX_DEPLOYMENT_TARGET=11.0
ZIELE=(aarch64-apple-darwin x86_64-apple-darwin)
PROGRAMME=(sprechwerk-fenster sprechwerk-server-fenster sprechwerk sprechwerk-cli)

for ziel in "${ZIELE[@]}"; do
  cargo build --release --locked --target "$ziel" -p sprechwerk-fenster \
    -p sprechwerk-server-fenster -p sprechwerk-server -p sprechwerk-cli
done
mkdir -p dist/universal
for p in "${PROGRAMME[@]}"; do
  lipo -create "target/aarch64-apple-darwin/release/$p" "target/x86_64-apple-darwin/release/$p" \
    -output "dist/universal/$p"
done

# buendel <Anzeigename> <Programm> <Bundle-ID> <Zielordner>
buendel() {
  local name=$1 programm=$2 kennung=$3 ordner=$4
  local app="$ordner/$name.app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Frameworks"
  cp "dist/universal/$programm" "$app/Contents/MacOS/$programm"
  # Opus wird zur Laufzeit gesucht, u. a. in Contents/Frameworks (crates/audio/src/opus.rs).
  # Der Server braucht es nicht (er wandelt keine Sprache), schadet aber nicht.
  cp packaging/opus/libopus.0.dylib "$app/Contents/Frameworks/"
  cp packaging/icons/icon.icns "$app/Contents/Resources/sprechwerk.icns"
  sed -e "s/@NAME@/$name/g" -e "s/@PROGRAMM@/$programm/g" -e "s/@KENNUNG@/$kennung/g" \
    -e "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist.in > "$app/Contents/Info.plist"
  # Ad-hoc-Signatur: ohne sie startet ein Universal-Binary auf Apple Silicon nicht.
  codesign --force --deep --sign - "$app"
}

dmg() {
  local ordner=$1 datei=$2
  ln -s /Applications "$ordner/Programme"
  hdiutil create -volname "Sprechwerk $VERSION" -srcfolder "$ordner" -ov -format UDZO "$datei"
}

rm -rf dist/dmg-client dist/dmg-server dist/cli && mkdir -p dist/dmg-client dist/dmg-server dist/cli

# --- Client ----------------------------------------------------------------
buendel "Sprechwerk" sprechwerk-fenster de.weppelmann.Sprechwerk dist/dmg-client
dmg dist/dmg-client "dist/Sprechwerk-${VERSION}-macos-universal.dmg"

# Referenzclient als eigenes Zip — er hat kein Fenster, also kein Bündel.
cp dist/universal/sprechwerk-cli packaging/opus/libopus.0.dylib LICENSE dist/cli/
(cd dist/cli && zip -q "../sprechwerk-${VERSION}-macos-universal-konsole.zip" ./*)

# --- Server ----------------------------------------------------------------
buendel "Sprechwerk Server" sprechwerk-server-fenster de.weppelmann.Sprechwerk.Server dist/dmg-server
dmg dist/dmg-server "dist/Sprechwerk-Server-${VERSION}-macos-universal.dmg"

rm -rf dist/dmg-client dist/dmg-server dist/cli dist/universal
ls -la dist/
