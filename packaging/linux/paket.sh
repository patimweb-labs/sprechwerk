#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
#
# Linux-Pakete für die **zwei getrennten Anwendungen** (tar.gz, deb, rpm,
# AppImage). Jede bekommt ihre eigenen Dateien; nichts wird gemischt.
#
#   Client  „Sprechwerk"          sprechwerk-fenster, sprechwerk-cli
#   Server  „Sprechwerk Server"   sprechwerk (Konsole), sprechwerk-server-fenster
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

# Was wohin gehört. Die Namen sind die Paketnamen; sie stehen auch in den
# beiden nfpm-Dateien und in den App-IDs der Fenster.
CLIENT=sprechwerk
SERVER=sprechwerk-server
CLIENT_PROGRAMME=(sprechwerk-fenster sprechwerk-cli)
SERVER_PROGRAMME=(sprechwerk sprechwerk-server-fenster)

if [ -z "${OHNE_BAU:-}" ]; then
  cargo build --release --locked -p sprechwerk-fenster -p sprechwerk-server-fenster \
    -p sprechwerk-server -p sprechwerk-cli
fi

rm -rf dist/stage dist/Sprechwerk.AppDir dist/Sprechwerk-Server.AppDir
mkdir -p dist

# --- Stufen: je Anwendung ein Baum -----------------------------------------
# Feste Pfade `dist/stage/<app>/usr`: nfpm setzt Umgebungsvariablen nur in
# Feldern wie `version` und `arch` ein, nicht in `contents`.

stufe_client=dist/stage/$CLIENT
stufe_server=dist/stage/$SERVER

for p in "${CLIENT_PROGRAMME[@]}"; do
  install -Dm755 "target/release/$p" "$stufe_client/usr/bin/$p"
done
for p in "${SERVER_PROGRAMME[@]}"; do
  install -Dm755 "target/release/$p" "$stufe_server/usr/bin/$p"
done

# Client: eigenes Symbol, eigene Metainfo, eigenes Fenster.
install -Dm644 packaging/linux/de.weppelmann.Sprechwerk.desktop \
  "$stufe_client/usr/share/applications/de.weppelmann.Sprechwerk.desktop"
install -Dm644 packaging/linux/de.weppelmann.Sprechwerk.metainfo.xml \
  "$stufe_client/usr/share/metainfo/de.weppelmann.Sprechwerk.metainfo.xml"
for g in 32x32 128x128 256x256 512x512; do
  install -Dm644 "packaging/icons/hicolor/$g/apps/sprechwerk.png" \
    "$stufe_client/usr/share/icons/hicolor/$g/apps/de.weppelmann.Sprechwerk.png"
done
install -Dm644 packaging/icons/hicolor/scalable/apps/sprechwerk.svg \
  "$stufe_client/usr/share/icons/hicolor/scalable/apps/de.weppelmann.Sprechwerk.svg"

# Server: eigenes Fenster, eigene Kennung, dieselbe Zeichnung unter eigenem
# Namen — damit sich beide Anwendungen in der Taskleiste unterscheiden.
install -Dm644 packaging/linux/de.weppelmann.Sprechwerk.Server.desktop \
  "$stufe_server/usr/share/applications/de.weppelmann.Sprechwerk.Server.desktop"
install -Dm644 packaging/linux/de.weppelmann.Sprechwerk.Server.metainfo.xml \
  "$stufe_server/usr/share/metainfo/de.weppelmann.Sprechwerk.Server.metainfo.xml"
for g in 32x32 128x128 256x256 512x512; do
  install -Dm644 "packaging/icons/hicolor/$g/apps/sprechwerk.png" \
    "$stufe_server/usr/share/icons/hicolor/$g/apps/de.weppelmann.Sprechwerk.Server.png"
done
install -Dm644 packaging/icons/hicolor/scalable/apps/sprechwerk.svg \
  "$stufe_server/usr/share/icons/hicolor/scalable/apps/de.weppelmann.Sprechwerk.Server.svg"

install -Dm644 LICENSE "$stufe_client/usr/share/doc/$CLIENT/LICENSE"
install -Dm644 LICENSE "$stufe_server/usr/share/doc/$SERVER/LICENSE"
# IBM Plex ist in beiden Fenstern eingebettet (OFL verlangt die Beilage).
install -Dm644 crates/oberflaeche/schriften/OFL.txt \
  "$stufe_client/usr/share/doc/$CLIENT/IBM-Plex-OFL.txt"
install -Dm644 crates/oberflaeche/schriften/OFL.txt \
  "$stufe_server/usr/share/doc/$SERVER/IBM-Plex-OFL.txt"

# --- tar.gz: zum Auspacken ohne Paketverwaltung ----------------------------
tar -C "$stufe_client/usr/.." -czf "dist/${CLIENT}-${VERSION}-linux-${ARCH}.tar.gz" usr
tar -C "$stufe_server/usr/.." -czf "dist/${SERVER}-${VERSION}-linux-${ARCH}.tar.gz" usr

# --- deb und rpm -----------------------------------------------------------
export VERSION
for art in deb rpm; do
  ARCH=$DEB_ARCH "$NFPM" package --config "packaging/linux/nfpm-${CLIENT}.yaml" \
    --packager "$art" --target dist/
  ARCH=$DEB_ARCH "$NFPM" package --config "packaging/linux/nfpm-${SERVER}.yaml" \
    --packager "$art" --target dist/
done

# --- AppImage: eine Datei je Anwendung -------------------------------------
# Die Symbole liegen schon in den Stufen; der AppDir braucht nur seinen Eintrag
# und einen AppRun, der das richtige Programm startet.
appimage() {
  local name=$1 stufe=$2 programm=$3 desktop=$4 symbol=$5
  local dir="dist/${name}.AppDir"
  rm -rf "$dir" && mkdir -p "$dir"
  cp -a "$stufe/usr" "$dir/usr"
  cp "packaging/linux/${desktop}" "$dir/"
  cp packaging/icons/icon-256.png "$dir/${symbol}.png"
  cat > "$dir/AppRun" <<APPRUN
#!/bin/sh
HIER=\$(dirname "\$(readlink -f "\$0")")
exec "\$HIER/usr/bin/${programm}" "\$@"
APPRUN
  chmod +x "$dir/AppRun"
  ARCH=$ARCH "$APPIMAGETOOL" --no-appstream "$dir" "dist/${name}-${VERSION}-${ARCH}.AppImage"
  rm -rf "$dir"
}

appimage "Sprechwerk" "$stufe_client" sprechwerk-fenster \
  de.weppelmann.Sprechwerk.desktop de.weppelmann.Sprechwerk
appimage "Sprechwerk-Server" "$stufe_server" sprechwerk-server-fenster \
  de.weppelmann.Sprechwerk.Server.desktop de.weppelmann.Sprechwerk.Server

rm -rf dist/stage
ls -la dist/
