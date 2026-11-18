#!/usr/bin/env bash
# macOS: zwei App-Bündel (Sprechwerk, Sprechwerk Server) als Universal-Binary
# (Apple Silicon + Intel) in einem .dmg, dazu die Konsolenprogramme als .zip.
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

# buendel <Anzeigename> <Programm> <Bundle-ID>
buendel() {
  local name=$1 programm=$2 kennung=$3
  local app="dist/dmg/$name.app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Frameworks"
  cp "dist/universal/$programm" "$app/Contents/MacOS/$programm"
  # Opus wird zur Laufzeit gesucht, u. a. in Contents/Frameworks (crates/audio/src/opus.rs).
  cp packaging/opus/libopus.0.dylib "$app/Contents/Frameworks/"
  cp packaging/icons/icon.icns "$app/Contents/Resources/sprechwerk.icns"
  sed -e "s/@NAME@/$name/g" -e "s/@PROGRAMM@/$programm/g" -e "s/@KENNUNG@/$kennung/g" \
    -e "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist.in > "$app/Contents/Info.plist"
  # Ad-hoc-Signatur: ohne sie startet ein Universal-Binary auf Apple Silicon nicht.
  codesign --force --deep --sign - "$app"
}

rm -rf dist/dmg && mkdir -p dist/dmg
buendel "Sprechwerk" sprechwerk-fenster de.weppelmann.Sprechwerk
buendel "Sprechwerk Server" sprechwerk-server-fenster de.weppelmann.Sprechwerk.Server
ln -s /Applications dist/dmg/Programme
hdiutil create -volname "Sprechwerk $VERSION" -srcfolder dist/dmg -ov -format UDZO \
  "dist/Sprechwerk-${VERSION}-macos-universal.dmg"

# Konsolenprogramme (Server ohne Fenster, Referenzclient).
mkdir -p dist/konsole
cp dist/universal/sprechwerk dist/universal/sprechwerk-cli packaging/opus/libopus.0.dylib dist/konsole/
(cd dist/konsole && zip -q "../sprechwerk-${VERSION}-macos-universal-konsole.zip" ./*)
rm -rf dist/dmg dist/universal dist/konsole
ls -la dist/
