#!/usr/bin/env bash
# Linux-Pakete: tar.gz, deb, rpm (nfpm) und AppImage (appimagetool).
#
# Aufruf aus dem Wurzelverzeichnis: packaging/linux/paket.sh
# Braucht: cargo, nfpm, appimagetool (Pfade über NFPM/APPIMAGETOOL änderbar).
# Ergebnis: dist/
set -euo pipefail
cd "$(dirname "$0")/../.."
VERSION=$(packaging/version.sh)
ARCH=${ARCH:-$(uname -m)}
case "$ARCH" in x86_64) DEB_ARCH=amd64 ;; aarch64) DEB_ARCH=arm64 ;; *) DEB_ARCH=$ARCH ;; esac
NFPM=${NFPM:-nfpm}
APPIMAGETOOL=${APPIMAGETOOL:-appimagetool}
PROGRAMME=(sprechwerk-fenster sprechwerk-server-fenster sprechwerk sprechwerk-cli)

if [ -z "${OHNE_BAU:-}" ]; then
  cargo build --release --locked -p sprechwerk-fenster -p sprechwerk-server-fenster \
    -p sprechwerk-server -p sprechwerk-cli
fi

rm -rf dist/stage && mkdir -p dist
STAGE=$PWD/dist/stage
for p in "${PROGRAMME[@]}"; do
  install -Dm755 "target/release/$p" "$STAGE/usr/bin/$p"
done
install -Dm644 packaging/linux/de.weppelmann.Sprechwerk.desktop "$STAGE/usr/share/applications/de.weppelmann.Sprechwerk.desktop"
install -Dm644 packaging/linux/de.weppelmann.Sprechwerk.Server.desktop "$STAGE/usr/share/applications/de.weppelmann.Sprechwerk.Server.desktop"
install -Dm644 packaging/linux/de.weppelmann.Sprechwerk.metainfo.xml "$STAGE/usr/share/metainfo/de.weppelmann.Sprechwerk.metainfo.xml"
for g in 32x32 128x128 256x256 512x512; do
  install -Dm644 "packaging/icons/hicolor/$g/apps/sprechwerk.png" \
    "$STAGE/usr/share/icons/hicolor/$g/apps/de.weppelmann.Sprechwerk.png"
done
install -Dm644 packaging/icons/hicolor/scalable/apps/sprechwerk.svg \
  "$STAGE/usr/share/icons/hicolor/scalable/apps/de.weppelmann.Sprechwerk.svg"
install -Dm644 LICENSE "$STAGE/usr/share/doc/sprechwerk/LICENSE"
install -Dm644 crates/oberflaeche/schriften/OFL.txt "$STAGE/usr/share/doc/sprechwerk/IBM-Plex-OFL.txt"

# tar.gz: zum Auspacken ohne Paketverwaltung.
tar -C "$STAGE" -czf "dist/sprechwerk-${VERSION}-linux-${ARCH}.tar.gz" usr

# deb und rpm.
export VERSION STAGE
for art in deb rpm; do
  ARCH=$DEB_ARCH "$NFPM" package --config packaging/linux/nfpm.yaml --packager "$art" --target dist/
done

# AppImage: ein Fenster je Datei — das Client-Fenster (der Server gehört
# in deb/rpm/Container oder in sein eigenes Fenster aus dem tar.gz).
APPDIR=dist/Sprechwerk.AppDir
rm -rf "$APPDIR" && mkdir -p "$APPDIR"
cp -a "$STAGE/usr" "$APPDIR/usr"
cp packaging/linux/de.weppelmann.Sprechwerk.desktop "$APPDIR/"
cp packaging/icons/icon-256.png "$APPDIR/de.weppelmann.Sprechwerk.png"
cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
HIER=$(dirname "$(readlink -f "$0")")
exec "$HIER/usr/bin/sprechwerk-fenster" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"
ARCH=$ARCH "$APPIMAGETOOL" --no-appstream "$APPDIR" "dist/Sprechwerk-${VERSION}-${ARCH}.AppImage"

rm -rf "$STAGE" "$APPDIR"
ls -la dist/
