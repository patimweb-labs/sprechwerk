# sprechwerk — ein Container, ein Port, ein Volume (SPEC §2).
FROM rust:1.83-slim AS bauen
WORKDIR /src

# Erst nur die Manifeste: dadurch bleibt die Abhängigkeitsschicht im Cache,
# solange sich der Code nicht ändert.
COPY Cargo.toml Cargo.lock ./
COPY crates/core/Cargo.toml crates/core/
COPY crates/proto/Cargo.toml crates/proto/
COPY crates/store/Cargo.toml crates/store/
COPY crates/client/Cargo.toml crates/client/
COPY crates/server/Cargo.toml crates/server/
COPY crates/cli/Cargo.toml crates/cli/
RUN mkdir -p crates/core/src crates/proto/src crates/store/src crates/client/src \
             crates/server/src crates/cli/src \
 && echo "" > crates/core/src/lib.rs && echo "" > crates/proto/src/lib.rs \
 && echo "" > crates/store/src/lib.rs && echo "" > crates/client/src/lib.rs \
 && echo "fn main() {}" > crates/server/src/main.rs && echo "fn main() {}" > crates/cli/src/main.rs \
 && cargo build --release --locked || true

COPY . .
RUN touch crates/*/src/*.rs && cargo build --release --locked

FROM debian:bookworm-slim
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
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s CMD ["/usr/local/bin/sprechwerk-cli", "--server", "127.0.0.1:7443", "--einmal", "ping"] || exit 1
ENTRYPOINT ["/usr/local/bin/sprechwerk"]
