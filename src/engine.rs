//! Fresnel-aware knife-edge diffraction (ITU-R P.526 style) + FSPL → SPLAT-shaped outputs.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{Context, Result};
use crate::dem::DemMosaic;
use crate::hash::{normalize, splat_input_sha256, Request as CovRequest, SPLAT_CACHE_SCHEMA_VERSION};
use crate::kml;
use crate::lora;
use crate::ppm;
use crate::ray_cache::{initial_bearing_rad, RayTerrainCache, EARTH_RADIUS_M};
use image::imageops::{self, FilterType};
use image::{ImageBuffer, Rgba};
use rayon::prelude::*;
use serde_json::json;

/// Effective Earth radius factor (standard 4/3 atmosphere).
const K_EFFECTIVE: f64 = 4.0 / 3.0;

pub fn run_coverage(work_dir: &Path, verbose: bool) -> Result<()> {
    let log = |msg: &str| {
        if verbose {
            eprintln!("[splatter] {}", msg);
        }
    };

    let mirror = std::env::var("SPLAT_CACHE").unwrap_or_else(|_| {
        work_dir
            .join(".tile_cache")
            .to_string_lossy()
            .into_owned()
    });
    let mirror_root = Path::new(&mirror);
    log(&format!(
        "work_dir={}  SPLAT_CACHE={}",
        work_dir.display(),
        mirror_root.display()
    ));

    let req_path = work_dir.join("request.json");
    let raw = fs::read_to_string(&req_path)
        .with_context(|| format!("read {}", req_path.display()))?;
    let parsed: CovRequest =
        serde_json::from_str(&raw).context("parse request.json as SplatCoverageRequest")?;
    let req = normalize(parsed);
    let input_sha = splat_input_sha256(&req).context("input sha256")?;

    let sf: i32 = i32::try_from(req.modem.spreading_factor)
        .with_context(|| format!("modem.spreading_factor={}", req.modem.spreading_factor))?;
    let cr: i32 = i32::try_from(req.modem.coding_rate)
        .with_context(|| format!("modem.coding_rate={}", req.modem.coding_rate))?;
    let modem_view = lora::LoRaModemView {
        spreading_factor: sf,
        bandwidth_khz: req.modem.bandwidth_khz,
        coding_rate: cr,
        implementation_margin_db: req.modem.implementation_margin_db,
        sensitivity_dbm: req.modem.sensitivity_dbm.as_ref(),
    };
    let threshold_dbm = lora::effective_signal_threshold_dbm(
        &modem_view,
        req.situation_fraction,
        req.time_fraction,
    )
    .context("compute LoRa effective threshold")?;

    log(&format!(
        "TX {:.6},{:.6}  radius={:.0} m  {:.3} MHz  schema_sha256={}…",
        req.lat,
        req.lon,
        req.radius,
        req.frequency_mhz,
        &input_sha[..12.min(input_sha.len())]
    ));

    let tiles = required_tile_names(req.lat, req.lon, req.radius);
    log(&format!(
        "DEM mirror: {} tile(s): {}",
        tiles.len(),
        tiles.join(", ")
    ));
    let dem = DemMosaic::load_mirror(mirror_root, &tiles, verbose).context("DEM mosaic")?;

    let z_tx_base = dem.sample_m(req.lat, req.lon);
    log(&format!(
        "terrain @ TX ≈ {:.1} m AMSL (AGL heights applied in profile)",
        z_tx_base
    ));

    let (north, south, east, west) = bbox_for_radius(req.lat, req.lon, req.radius);
    let (w, h) = grid_dims(req.radius);
    log(&format!(
        "grid {}×{} px  bbox N={:.6} S={:.6} E={:.6} W={:.6}  fresnel_clearance_fraction={:.3}",
        w,
        h,
        north,
        south,
        east,
        west,
        req.fresnel_clearance_fraction
    ));

    let re_eff = K_EFFECTIVE * EARTH_RADIUS_M;
    let clutter = req.clutter_height.max(0.0);
    let fresnel_frac = req.fresnel_clearance_fraction;
    let freq_hz = req.frequency_mhz * 1e6;
    let z_tx_amsl = z_tx_base + req.tx_height;
    let eirp_chain = req.tx_power + req.tx_gain + req.rx_gain - req.system_loss;
    let cmap = req.colormap.trim().to_lowercase();

    let num_rays = (w as usize * 4).max(720);
    let terrain_cache = RayTerrainCache::build(
        &dem,
        req.lat,
        req.lon,
        req.radius,
        num_rays,
        clutter,
        verbose,
    );

    let mut rgb: Vec<u8> = vec![255u8; (w * h * 3) as usize];

    let raster_started = Instant::now();
    let rows_done = AtomicUsize::new(0);
    let log_mx = Mutex::new(());
    let report_every = ((h as usize) / 20).max(1);

    let rows: Vec<Vec<u8>> = (0..h)
        .into_par_iter()
        .map(|py| {
            let mut row = vec![255u8; (w * 3) as usize];
            for px in 0..w {
                let (rlat, rlon) = pixel_lat_lon(px, py, w, h, north, south, east, west);
                let d = haversine_m(req.lat, req.lon, rlat, rlon);
                let off = (px * 3) as usize;
                if d > req.radius {
                    continue;
                }
                let fspl = fspl_db(d, req.frequency_mhz);
                if eirp_chain - fspl < threshold_dbm {
                    continue;
                }
                let z_rx_amsl = dem.sample_m(rlat, rlon) + req.rx_height;
                let bearing = initial_bearing_rad(req.lat, req.lon, rlat, rlon);
                let diff_db = knife_edge_excess_loss_db(
                    &terrain_cache,
                    bearing,
                    d,
                    z_tx_amsl,
                    z_rx_amsl,
                    re_eff,
                    freq_hz,
                    fresnel_frac,
                );
                let pr = eirp_chain - fspl - diff_db;
                if pr < threshold_dbm {
                    continue;
                }
                let c = dbm_to_rgb(pr, req.min_dbm, req.max_dbm, &cmap);
                row[off] = c[0];
                row[off + 1] = c[1];
                row[off + 2] = c[2];
            }
            if verbose {
                let n = rows_done.fetch_add(1, Ordering::Relaxed) + 1;
                if n % report_every == 0 || n == h as usize {
                    let _lk = log_mx.lock().unwrap();
                    eprintln!(
                        "[splatter] raster rows completed {}/{} ({:.1}s)",
                        n,
                        h,
                        raster_started.elapsed().as_secs_f64()
                    );
                }
            }
            row
        })
        .collect();

    if verbose {
        log(&format!(
            "raster finished {} rows in {:.2}s",
            h,
            raster_started.elapsed().as_secs_f64()
        ));
    }

    for (py, row) in rows.iter().enumerate() {
        let base = (py as u32 * w * 3) as usize;
        rgb[base..base + row.len()].copy_from_slice(row);
    }

    let ppm_path = work_dir.join("output.ppm");
    log(&format!("writing {}", ppm_path.display()));
    ppm::write_ppm_rgb(&ppm_path, w, h, &rgb).context("write output.ppm")?;

    let kml_txt =
        kml::ground_overlay_kml("Splatter coverage", north, south, east, west, 0.0);
    log("writing output.kml");
    fs::write(work_dir.join("output.kml"), kml_txt).context("write output.kml")?;

    log("writing splat.png");
    write_splat_png(work_dir.join("splat.png").as_path(), w, h, &rgb).context("write splat.png")?;

    let bbox = json!({
        "north": north,
        "south": south,
        "east": east,
        "west": west,
        "rotation": 0.0,
    });

    let manifest = json!({
        "splat_exit_code": 0,
        "splat_input_sha256": input_sha,
        "splat_cache_schema_version": SPLAT_CACHE_SCHEMA_VERSION,
        "splat_stdout_tail": "splatter Fresnel knife-edge + FSPL engine\n",
        "splat_stderr_tail": "",
        "bbox": bbox,
        "splatter_engine": env!("CARGO_PKG_VERSION"),
    });

    log("writing manifest.json");
    fs::write(
        work_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .context("write manifest.json")?;

    log("done.");
    Ok(())
}

fn required_tile_names(lat: f64, lon: f64, radius_m: f64) -> Vec<String> {
    let delta_deg = radius_m / EARTH_RADIUS_M * (180.0 / std::f64::consts::PI);
    let lat_min = lat - delta_deg;
    let lat_max = lat + delta_deg;
    let cos_lat = lat.to_radians().cos().max(0.01);
    let lon_min = lon - delta_deg / cos_lat;
    let lon_max = lon + delta_deg / cos_lat;
    let lat_min_tile = lat_min.floor() as i32;
    let lat_max_tile = lat_max.floor() as i32;
    let lon_min_tile = lon_min.floor() as i32;
    let lon_max_tile = lon_max.floor() as i32;
    let mut out = Vec::new();
    for lat_tile in lat_min_tile..=lat_max_tile {
        for lon_tile in lon_min_tile..=lon_max_tile {
            let ns = if lat_tile >= 0 { 'N' } else { 'S' };
            let ew = if lon_tile >= 0 { 'E' } else { 'W' };
            out.push(format!(
                "{}{}{}{:03}.hgt.gz",
                ns,
                lat_tile.abs(),
                ew,
                lon_tile.abs()
            ));
        }
    }
    out.sort();
    out.dedup();
    out
}

fn bbox_for_radius(lat: f64, lon: f64, radius_m: f64) -> (f64, f64, f64, f64) {
    let delta_deg = radius_m / EARTH_RADIUS_M * (180.0 / std::f64::consts::PI);
    let cos_lat = lat.to_radians().cos().max(0.01);
    let lat_scale = delta_deg;
    let lon_scale = delta_deg / cos_lat;
    let north = lat + lat_scale;
    let south = lat - lat_scale;
    let east = lon + lon_scale;
    let west = lon - lon_scale;
    (north, south, east, west)
}

fn grid_dims(radius_m: f64) -> (u32, u32) {
    let px = ((radius_m / 120.0).round() as u32).clamp(160, 2048);
    (px, px)
}

fn pixel_lat_lon(
    px: u32,
    py: u32,
    w: u32,
    h: u32,
    north: f64,
    south: f64,
    east: f64,
    west: f64,
) -> (f64, f64) {
    let u = (px as f64 + 0.5) / w as f64;
    let v = (py as f64 + 0.5) / h as f64;
    let lon = west + u * (east - west);
    let lat = north - v * (north - south);
    (lat, lon)
}

fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let p1 = lat1.to_radians();
    let p2 = lat2.to_radians();
    let dl = (lon2 - lon1).to_radians();
    let dp = (lat2 - lat1).to_radians();
    let a =
        (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS_M * c
}

/// ITU-R P.526-style excess diffraction loss (dB) for a single knife-edge, normalized height ν.
fn itu_p526_single_knife_edge_loss_db(nu: f64) -> f64 {
    if nu < -0.78 {
        return 0.0;
    }
    let inner = ((nu - 0.1).powi(2) + 1.0).sqrt() + nu - 0.1;
    if inner <= 1e-12 {
        return 0.0;
    }
    (6.9 + 20.0 * inner.log10()).clamp(0.0, 120.0)
}

/// Dominant single-edge excess loss using precomputed ray terrain (no per-pixel DEM sampling).
fn knife_edge_excess_loss_db(
    cache: &RayTerrainCache,
    bearing_rad: f64,
    distance_m: f64,
    z_tx_amsl: f64,
    z_rx_amsl: f64,
    re_eff: f64,
    freq_hz: f64,
    fresnel_clearance_frac: f64,
) -> f64 {
    let d = distance_m;
    if d < 2.0 {
        return 0.0;
    }
    let steps = cache.profile_step_count(d);
    if steps < 2 {
        return 0.0;
    }
    let wl = 299_792_458.0 / freq_hz;
    let mut nu_max: Option<f64> = None;
    for i in 1..steps {
        let frac = i as f64 / steps as f64;
        let s = frac * d;
        let h_line =
            z_tx_amsl * (1.0 - s / d) + z_rx_amsl * (s / d) - s * (d - s) / (2.0 * re_eff);
        let terr = cache.terrain_at(bearing_rad, s);
        let d1 = s;
        let d2 = d - s;
        if d1 < 2.0 || d2 < 2.0 {
            continue;
        }
        let f1 = (wl * d1 * d2 / d).sqrt();
        let need = fresnel_clearance_frac * f1;
        let clearance_floor = h_line - need;
        let h_obs = terr - clearance_floor;
        if h_obs <= 0.0 {
            continue;
        }
        let nu = h_obs * (2.0 * d / (wl * d1 * d2)).sqrt();
        nu_max = Some(match nu_max {
            None => nu,
            Some(m) => m.max(nu),
        });
    }
    match nu_max {
        None => 0.0,
        Some(nu) => itu_p526_single_knife_edge_loss_db(nu),
    }
}

#[cfg(test)]
mod diff_tests {
    use super::itu_p526_single_knife_edge_loss_db;

    #[test]
    fn itu_knife_edge_non_negative() {
        assert!(itu_p526_single_knife_edge_loss_db(-1.0) < 1e-6);
        assert!(itu_p526_single_knife_edge_loss_db(0.0) > 0.0);
        assert!(itu_p526_single_knife_edge_loss_db(2.0) > itu_p526_single_knife_edge_loss_db(0.0));
    }
}

fn fspl_db(distance_m: f64, freq_mhz: f64) -> f64 {
    let d_km = distance_m.max(1.0) / 1000.0;
    20.0 * d_km.log10() + 20.0 * freq_mhz.max(1e-6).log10() + 32.44
}

fn dbm_to_rgb(pr_dbm: f64, min_dbm: f64, max_dbm: f64, cmap: &str) -> [u8; 3] {
    let lo = min_dbm.min(max_dbm);
    let hi = min_dbm.max(max_dbm);
    let t = if hi <= lo {
        0.5
    } else {
        ((pr_dbm - lo) / (hi - lo)).clamp(0.0, 1.0)
    };
    match cmap {
        "plasma" => plasma_rgb(t),
        "rainbow" | "jet" => rainbow_rgb(t),
        _ => plasma_rgb(t),
    }
}

fn plasma_rgb(t: f64) -> [u8; 3] {
    let c0 = [13u8, 8, 135];
    let c1 = [126u8, 3, 168];
    let c2 = [204u8, 71, 120];
    let c3 = [248u8, 148, 65];
    let c4 = [240u8, 249, 33];
    let stops = [c0, c1, c2, c3, c4];
    let n = stops.len() - 1;
    let x = t * n as f64;
    let i = (x.floor() as usize).min(n - 1);
    let f = x - i as f64;
    let a = stops[i];
    let b = stops[i + 1];
    [
        (a[0] as f64 * (1.0 - f) + b[0] as f64 * f).round() as u8,
        (a[1] as f64 * (1.0 - f) + b[1] as f64 * f).round() as u8,
        (a[2] as f64 * (1.0 - f) + b[2] as f64 * f).round() as u8,
    ]
}

fn rainbow_rgb(t: f64) -> [u8; 3] {
    let h = 240.0 * (1.0 - t);
    hsv_to_rgb(h, 1.0, 1.0)
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (rp, gp, bp) = match h {
        hh if hh < 60.0 => (c, x, 0.0),
        hh if hh < 120.0 => (x, c, 0.0),
        hh if hh < 180.0 => (0.0, c, x),
        hh if hh < 240.0 => (0.0, x, c),
        hh if hh < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((rp + m) * 255.0).round() as u8,
        ((gp + m) * 255.0).round() as u8,
        ((bp + m) * 255.0).round() as u8,
    ]
}

fn overlay_max_edge_from_env() -> Option<u32> {
    match std::env::var("PEAKY_SPLAT_OVERLAY_MAX_EDGE") {
        Ok(s) => {
            let s = s.trim();
            if s.is_empty() || s.eq_ignore_ascii_case("none") {
                return Some(8192);
            }
            let Ok(n) = s.parse::<i64>() else {
                return Some(8192);
            };
            if n < 1 {
                None
            } else {
                Some((n as u32).max(32))
            }
        }
        Err(_) => Some(8192),
    }
}

fn overlay_dimensions_capped(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    if width.max(height) <= max_edge {
        return (width, height);
    }
    let scale = max_edge as f64 / width.max(height) as f64;
    let nw = ((width as f64 * scale).round() as u32).max(1);
    let nh = ((height as f64 * scale).round() as u32).max(1);
    (nw, nh)
}

fn write_splat_png(path: &Path, width: u32, height: u32, rgb: &[u8]) -> Result<()> {
    let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 3) as usize;
            let r = rgb[i];
            let g = rgb[i + 1];
            let b = rgb[i + 2];
            let a = if r == 255 && g == 255 && b == 255 { 0 } else { 255 };
            img.put_pixel(x, y, Rgba([r, g, b, a]));
        }
    }

    let img_final = match overlay_max_edge_from_env() {
        Some(mx) => {
            let (nw, nh) = overlay_dimensions_capped(width, height, mx);
            if (nw, nh) != (width, height) {
                imageops::resize(&img, nw, nh, FilterType::Lanczos3)
            } else {
                img
            }
        }
        None => img,
    };

    img_final
        .save(path)
        .with_context(|| path.display().to_string())?;
    Ok(())
}
