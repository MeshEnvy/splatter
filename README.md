# splatter

Fresnel-aware knife-edge diffraction (ITU-R P.526) + FSPL coverage raster. Drop-in SPLAT-shaped outputs: `output.ppm`, `output.kml`, `splat.png`, `manifest.json`.

## Build

```bash
cargo build --release
```

Docker:

```bash
docker build -t splatter:latest .
```

## Run

Container entry reads `/work/request.json`, loads Skadi HGT tiles from `SPLAT_CACHE` (fetching missing tiles from public Skadi S3 on demand), writes SPLAT-shaped outputs into the work dir.

```bash
docker run --rm \
  -v "$SITE:/work" -v "$MIRROR:/splat_cache" \
  -e SPLAT_CACHE=/splat_cache \
  splatter:latest run --work-dir /work --verbose
```

Input hash parity with peaky_finders:

```bash
splatter input-sha256 --request /path/to/request.json
```

## Tests

```bash
cargo test
```

Golden hash fixture lives in `tests/fixtures/splat_request_hash_fixture.json` (mirrors peaky_finders Python tests).
