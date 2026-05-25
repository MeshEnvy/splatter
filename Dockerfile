# Fresnel + FSPL coverage — drop-in SPLAT-shaped outputs (PPM/KML/manifest/splat.png).
# Builder uses rust:bookworm (rolling stable) because transitive crates may require a recent Cargo.
#
# Build (splatter repo root):
#   docker build -t splatter:latest .
#
# Run one site dir that already has request.json (host paths):
#   SITE=/path/to/.cache/viewsheds/<digest>   # one propagation workspace dir
#   MIRROR=/path/to/.cache/splat_tiles
#   docker run --rm \
#     -v "$SITE:/work" -v "$MIRROR:/splat_cache" \
#     -e SPLAT_CACHE=/splat_cache \
#     splatter:latest run --work-dir /work
# Add ``--verbose`` (or ``-v``) for stderr progress (tiles, DEM load, raster rows).
# Batch: ``run-batch --work-dir /work`` with array ``request.json`` at the mount root.

FROM rust:bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/splatter /usr/local/bin/splatter

ENTRYPOINT ["/usr/local/bin/splatter"]
CMD ["run", "--work-dir", "/work"]
