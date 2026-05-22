//! Skadi HGT mirror: one gzipped ``*.hgt.gz`` per tile at mirror root (see ``skadi_dem.skadi_mirror_tile_gz_path``).

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;

pub const VOID_SRTM: i16 = -32768;

pub struct DemTile {
    pub sw_lat: f64,
    pub sw_lon: f64,
    pub n: usize,
    pub elevations: Vec<i16>,
}

impl DemTile {
    pub fn spacing_deg(&self) -> f64 {
        1.0 / (self.n - 1) as f64
    }

    /// Bilinear sample AMSL meters; void/partial void → 0.0 (ocean / ignore).
    ///
    /// HGT row 0 is the **northern** edge of the 1° cell (SRTM convention).
    pub fn sample_m(&self, lat: f64, lon: f64) -> f64 {
        let spacing = self.spacing_deg();
        let north = self.sw_lat + 1.0;
        let fx = (lon - self.sw_lon) / spacing;
        let fy = (north - lat) / spacing;
        let x0 = fx.floor() as isize;
        let y0 = fy.floor() as isize;
        if x0 < 0 || y0 < 0 {
            return 0.0;
        }
        let x1 = x0 + 1;
        let y1 = y0 + 1;
        let xm = self.n as isize - 1;
        let ym = self.n as isize - 1;
        if x1 > xm || y1 > ym {
            return 0.0;
        }
        let tx = fx - x0 as f64;
        let ty = fy - y0 as f64;

        let z00 = elev_m(&self.elevations, self.n, x0 as usize, y0 as usize);
        let z10 = elev_m(&self.elevations, self.n, x1 as usize, y0 as usize);
        let z01 = elev_m(&self.elevations, self.n, x0 as usize, y1 as usize);
        let z11 = elev_m(&self.elevations, self.n, x1 as usize, y1 as usize);

        let z0 = z00 * (1.0 - tx) + z10 * tx;
        let z1 = z01 * (1.0 - tx) + z11 * tx;
        z0 * (1.0 - ty) + z1 * ty
    }
}

fn elev_m(buf: &[i16], n: usize, ix: usize, iy: usize) -> f64 {
    let v = buf[iy * n + ix];
    if v == VOID_SRTM || v < -12000 {
        return 0.0;
    }
    v as f64
}

type TileCoord = (i32, i32);

pub struct DemMosaic {
    tiles: HashMap<TileCoord, DemTile>,
}

impl DemMosaic {
    pub fn load_mirror(mirror_root: &Path, tile_names: &[String], verbose: bool) -> Result<Self> {
        let mut tiles = HashMap::new();
        for name in tile_names {
            let path: PathBuf = mirror_root.join(name);
            if !path.is_file() {
                bail!("missing DEM mirror tile {} (expected {})", name, path.display());
            }
            if verbose {
                eprintln!("[splatter] DEM load {}", path.display());
            }
            let tile = load_hgt_gz(&path).with_context(|| format!("load {}", path.display()))?;
            let stem = stem_key(name);
            let coord = tile_coord_from_stem(&stem)?;
            if verbose {
                eprintln!(
                    "[splatter] DEM tile {} → {}×{} samples",
                    stem, tile.n, tile.n
                );
            }
            tiles.insert(coord, tile);
        }
        Ok(Self { tiles })
    }

    pub fn sample_m(&self, lat: f64, lon: f64) -> f64 {
        let key = tile_coord_for_lat_lon(lat, lon);
        let Some(tile) = self.tiles.get(&key) else {
            return 0.0;
        };
        tile.sample_m(lat, lon)
    }
}

fn tile_coord_for_lat_lon(lat: f64, lon: f64) -> TileCoord {
    (lat.floor() as i32, lon.floor() as i32)
}

fn stem_key(tile_name: &str) -> String {
    tile_name
        .trim_end_matches(".gz")
        .trim_end_matches(".hgt")
        .to_string()
}

fn tile_coord_from_stem(stem: &str) -> Result<TileCoord> {
    let (sw_lat, sw_lon) = parse_tile_sw_corner(stem)?;
    Ok((sw_lat as i32, sw_lon as i32))
}

fn load_hgt_gz(path: &Path) -> Result<DemTile> {
    let f = File::open(path)?;
    let mut gz = GzDecoder::new(f);
    let mut raw = Vec::new();
    gz.read_to_end(&mut raw)?;
    let n = if raw.len() == 1201 * 1201 * 2 {
        1201
    } else if raw.len() == 3601 * 3601 * 2 {
        3601
    } else {
        bail!("unexpected HGT uncompressed size {} bytes", raw.len());
    };
    let mut elevations = Vec::with_capacity(n * n);
    for chunk in raw.chunks_exact(2) {
        elevations.push(i16::from_be_bytes([chunk[0], chunk[1]]));
    }

    let stem = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .trim_end_matches(".gz")
        .trim_end_matches(".hgt");
    let (sw_lat, sw_lon) = parse_tile_sw_corner(stem)?;

    Ok(DemTile {
        sw_lat,
        sw_lon,
        n,
        elevations,
    })
}

fn parse_tile_sw_corner(stem: &str) -> Result<(f64, f64)> {
    let stem = stem.trim();
    if stem.len() < 7 {
        bail!("bad tile stem {stem:?}");
    }
    let ns = stem.chars().next().unwrap();
    let ew = stem.chars().nth(3).unwrap();
    let lat_d: i32 = stem[1..3].parse().context("lat digits")?;
    let lon_d: i32 = stem[4..7].parse().context("lon digits")?;
    let sw_lat = if ns == 'N' || ns == 'n' {
        lat_d as f64
    } else {
        -(lat_d as f64)
    };
    let sw_lon = if ew == 'E' || ew == 'e' {
        lon_d as f64
    } else {
        -(lon_d as f64)
    };
    Ok((sw_lat, sw_lon))
}
