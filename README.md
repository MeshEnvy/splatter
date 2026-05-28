# splatter

Fresnel-aware knife-edge diffraction (ITU-R P.526) + FSPL coverage raster. Drop-in SPLAT-shaped outputs: `output.ppm`, `output.kml`, `splat.png`, `manifest.json`.

Peaky imports the **PyO3 extension** (`pip install` / maturin wheel) for in-process coverage with a resident DEM mosaic. The optional CLI remains for debugging.

## Build extension (maturin)

```bash
cd splatter
pip install maturin
maturin develop --release
```

Docker (Peaky image):

```bash
docker build --target dev -t peaky:dev .
```

## Optional CLI

```bash
cargo build --release --bin splatter --no-default-features
./target/release/splatter --help
```

## Run (CLI)

Container entry reads `/work/request.json`:

- **Single site:** object → `splatter run --work-dir /work`
- **Batch:** JSON array → `splatter run-batch --work-dir /work` → writes `/work/<digest>/` per request

Loads Skadi HGT tiles from `SPLAT_CACHE` (fetching missing tiles from public Skadi S3 on demand), writes SPLAT-shaped outputs into each workspace dir.

```bash
docker run --rm \
  -v "$SITE:/work" -v "$MIRROR:/splat_cache" \
  -e SPLAT_CACHE=/splat_cache \
  splatter:latest run --work-dir /work --verbose
```

Batch (array `request.json` at viewsheds root when many workspaces share one container run):

```bash
docker run --rm \
  -v "$VIEWSHEDS:/work" -v "$MIRROR:/splat_cache" \
  -e SPLAT_CACHE=/splat_cache \
  -e SPLATTER_BATCH_JOBS=8 \
  splatter:latest run-batch --work-dir /work --verbose
```

Input hash (stable digest for workspace cache keys):

```bash
python -c 'import splatter, json; print(splatter.input_sha256(open("request.json").read()))'
```

## Python API

```python
from splatter import get_session

session = get_session(mirror_root="/path/to/skadi/mirror", verbose=True)
session.preload_tiles(["N40W118.hgt.gz", ...])
session.run("/work/site_workspace")          # request.json in work dir
session.run_batch("/work/viewsheds", batch_jobs=8, requests_json=open("batch.json").read())

# Pairwise RF link checks (corridor / pathfinding; reuses resident DEM)
session.ensure_tiles_for_points([(lat, lon), ...], buffer_m=5000.0)
session.link_mutual_viable(lat_a, lon_a, lat_b, lon_b, rf_json)
session.link_mutual_batch([(lat_a, lon_a, lat_b, lon_b), ...], rf_json)  # parallel
```

`rf_json` is a ``SplatCoverageRequest`` JSON blob (preset propagation fields; lat/lon/radius set ``radius`` as max hop range).

## Tests

```bash
cargo test
./peaky-test tests/test_peaky_los_input_hash.py
```

Golden hash fixture: `tests/fixtures/splat_request_hash_fixture.json`.
