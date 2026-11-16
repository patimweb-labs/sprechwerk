# sprechwerk — ein Container, ein Port, ein Volume (SPEC §2).
#
# **Was hier stand und nicht funktionierte (gefunden 2026-11-14):** die
# Crate-Pfade hießen `crates/core`, `crates/proto`, `crates/client` — so heißen
# sie seit der Umbenennung (`domain`, `protocol`, `adapters`) nicht mehr. Der
# erste `COPY` scheiterte also, `docker compose up` war tot. Außerdem stand hier
# `rust:1.83-slim`, während `rust-toolchain.toml` die Version festlegt
# (1.100.0): der Bau hätte eine andere Rust-Version benutzt als CI und Tests.
#
# Und der `HEALTHCHECK` war gefährlich: er rief `sprechwerk-cli --einmal ping`
# auf — und der **erste Client wird Administrator** (SPEC §7). Der Check hätte
# sich den Administratorplatz alle 30 Sekunden mit einem Wegwerfschlüssel
# genommen. Jetzt prüft er ein Lebenszeichen, das der Server selbst schreibt
# (`crates/server/src/gesundheit.rs`).

# Dieselbe Version wie `rust-toolchain.toml` — nicht „irgendein Rust".
# `-trixie` ausdrücklich: die Laufzeit unten ist `debian:trixie-slim`, und ohne
# Zusatz wechselt das Abbild die Debian-Fassung, wenn Docker es so will.
FROM rust:1.100-slim-trixie AS bauen
WORKDIR /src

# Erst nur die Manifeste: dadurch bleibt die Abhängigkeitsschicht im Cache,
# solange sich der Code nicht ändert. Die Liste muss zu den `members` in
# `Cargo.toml` passen (ohne `crates/desktop`: Tauri hat eine eigene
# Sperrdatei und gehört nicht in den Container).
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/domain/Cargo.toml crates/domain/
COPY crates/protocol/Cargo.toml crates/protocol/
COPY crates/store/Cargo.toml crates/store/
COPY crates/adapters/Cargo.toml crates/adapters/
COPY crates/ipc/Cargo.toml crates/ipc/
COPY crates/audio/Cargo.toml crates/audio/
COPY crates/server/Cargo.toml crates/server/
COPY crates/cli/Cargo.toml crates/cli/
RUN for k in domain protocol store adapters ipc audio; do \
      mkdir -p "crates/$k/src" && echo "" > "crates/$k/src/lib.rs"; \
    done \
 && echo "fn main() {}" > crates/server/src/main.rs \
 && echo "fn main() {}" > crates/cli/src/main.rs \
 && cargo build --release --locked -p sprechwerk-server -p sprechwerk-cli || true

COPY . .
# `touch` auf die Quellen: sonst hält Cargo die Platzhalter für aktuell.
RUN touch crates/*/src/*.rs \
 && cargo build --release --locked -p sprechwerk-server -p sprechwerk-cli

# Laufzeit: gleiche Debian-Familie wie der Bau (sonst passt die glibc nicht).
FROM debian:trixie-slim
RUN useradd --system --uid 10001 --home /data --shell /usr/sbin/nologin sprechwerk \
 && mkdir -p /data && chown -R sprechwerk /data
COPY --from=bauen /src/target/release/sprechwerk /usr/local/bin/sprechwerk
COPY --from=bauen /src/target/release/sprechwerk-cli /usr/local/bin/sprechwerk-cli

USER sprechwerk
VOLUME ["/data"]
# QUIC spricht UDP. TCP wird nicht gebraucht — das ist der ganze Punkt.
EXPOSE 7443/udp
ENV SPRECHWERK_DATEN=/data \
    SPRECHWERK_BIND=0.0.0.0 \
    SPRECHWERK_PORT=7443
# Prüft das Lebenszeichen des Dienstes — **keine** Testverbindung: die würde
# den Administratorplatz belegen (SPEC §7).
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD ["/usr/local/bin/sprechwerk", "--gesundheit"]
ENTRYPOINT ["/usr/local/bin/sprechwerk"]
