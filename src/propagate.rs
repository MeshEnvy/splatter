//! Point-to-point RF link evaluation (same physics as coverage raster, no viewshed).

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::dem::DemMosaic;
use crate::hash::{normalize, Request as CovRequest};
use crate::lora;
use crate::ray_cache::{destination_point, initial_bearing_rad, EARTH_RADIUS_M};

/// Profile sample spacing (matches ray cache / raster).
pub const PROFILE_STEP_M: f64 = 120.0;

const K_EFFECTIVE: f64 = 4.0 / 3.0;
const MIN_PROFILE_STEPS: usize = 40;

/// Effective Earth radius for RF line-of-sight (4/3 Earth).
const RE_EFF_M: f64 = K_EFFECTIVE * EARTH_RADIUS_M;

#[derive(Debug, Clone)]
pub struct LinkContext {
    pub threshold_dbm: f64,
    pub eirp_chain: f64,
    pub freq_hz: f64,
    pub tx_height: f64,
    pub rx_height: f64,
    pub clutter: f64,
    pub fresnel_frac: f64,
    pub max_range_m: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct LinkResult {
    pub viable: bool,
    pub distance_m: f64,
    pub pr_dbm: f64,
    pub excess_loss_db: f64,
}

pub fn link_context_from_request(parsed: CovRequest) -> Result<LinkContext> {
    let req = normalize(parsed);
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
    )?;
    Ok(LinkContext {
        threshold_dbm,
        eirp_chain: req.tx_power + req.tx_gain + req.rx_gain - req.system_loss,
        freq_hz: req.frequency_mhz * 1e6,
        tx_height: req.tx_height,
        rx_height: req.rx_height,
        clutter: req.clutter_height.max(0.0),
        fresnel_frac: req.fresnel_clearance_fraction,
        max_range_m: req.radius.max(1.0),
    })
}

pub fn link_context_from_json(rf_json: &str) -> Result<LinkContext> {
    let parsed: CovRequest =
        serde_json::from_str(rf_json).context("parse RF propagation JSON")?;
    link_context_from_request(parsed)
}

pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let p1 = lat1.to_radians();
    let p2 = lat2.to_radians();
    let dl = (lon2 - lon1).to_radians();
    let dp = (lat2 - lat1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS_M * c
}

pub fn required_tile_names_for_bbox(north: f64, south: f64, east: f64, west: f64) -> Vec<String> {
    let lat_min_tile = south.floor() as i32;
    let lat_max_tile = north.floor() as i32;
    let lon_min_tile = west.floor() as i32;
    let lon_max_tile = east.floor() as i32;
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

pub fn required_tile_names_for_points(points: &[(f64, f64)], buffer_m: f64) -> Vec<String> {
    if points.is_empty() {
        return Vec::new();
    }
    let delta_deg = buffer_m / EARTH_RADIUS_M * (180.0 / std::f64::consts::PI);
    let mut north = f64::NEG_INFINITY;
    let mut south = f64::INFINITY;
    let mut east = f64::NEG_INFINITY;
    let mut west = f64::INFINITY;
    for (lat, lon) in points {
        let cos_lat = lat.to_radians().cos().max(0.01);
        north = north.max(*lat + delta_deg);
        south = south.min(*lat - delta_deg);
        east = east.max(*lon + delta_deg / cos_lat);
        west = west.min(*lon - delta_deg / cos_lat);
    }
    required_tile_names_for_bbox(north, south, east, west)
}

fn fspl_db(distance_m: f64, freq_mhz: f64) -> f64 {
    let d_km = distance_m.max(1.0) / 1000.0;
    20.0 * d_km.log10() + 20.0 * freq_mhz.max(1e-6).log10() + 32.44
}

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

fn profile_step_count(distance_m: f64) -> usize {
    if distance_m < 2.0 {
        return 0;
    }
    let max_steps = ((distance_m / PROFILE_STEP_M).ceil() as usize).max(MIN_PROFILE_STEPS);
    max_steps.max(2)
}

fn knife_edge_excess_loss_db_profile(
    dem: &DemMosaic,
    tx_lat: f64,
    tx_lon: f64,
    rx_lat: f64,
    rx_lon: f64,
    distance_m: f64,
    z_tx_amsl: f64,
    z_rx_amsl: f64,
    freq_hz: f64,
    fresnel_clearance_frac: f64,
    clutter: f64,
) -> f64 {
    let d = distance_m;
    if d < 2.0 {
        return 0.0;
    }
    let steps = profile_step_count(d);
    if steps < 2 {
        return 0.0;
    }
    let bearing = initial_bearing_rad(tx_lat, tx_lon, rx_lat, rx_lon);
    let wl = 299_792_458.0 / freq_hz;
    let mut nu_max: Option<f64> = None;
    for i in 1..steps {
        let frac = i as f64 / steps as f64;
        let s = frac * d;
        let (lat, lon) = destination_point(tx_lat, tx_lon, bearing, s);
        let terr = dem.sample_m(lat, lon) + clutter;
        let h_line =
            z_tx_amsl * (1.0 - s / d) + z_rx_amsl * (s / d) - s * (d - s) / (2.0 * RE_EFF_M);
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

pub fn evaluate_link(
    dem: &DemMosaic,
    tx_lat: f64,
    tx_lon: f64,
    rx_lat: f64,
    rx_lon: f64,
    ctx: &LinkContext,
) -> LinkResult {
    let distance_m = haversine_m(tx_lat, tx_lon, rx_lat, rx_lon);
    if distance_m > ctx.max_range_m {
        return LinkResult {
            viable: false,
            distance_m,
            pr_dbm: f64::NEG_INFINITY,
            excess_loss_db: f64::INFINITY,
        };
    }
    if distance_m < 2.0 {
        return LinkResult {
            viable: true,
            distance_m,
            pr_dbm: ctx.eirp_chain,
            excess_loss_db: 0.0,
        };
    }
    let freq_mhz = ctx.freq_hz / 1e6;
    let fspl = fspl_db(distance_m, freq_mhz);
    if ctx.eirp_chain - fspl < ctx.threshold_dbm {
        return LinkResult {
            viable: false,
            distance_m,
            pr_dbm: ctx.eirp_chain - fspl,
            excess_loss_db: 0.0,
        };
    }
    let z_tx_amsl = dem.sample_m(tx_lat, tx_lon) + ctx.tx_height;
    let z_rx_amsl = dem.sample_m(rx_lat, rx_lon) + ctx.rx_height;
    let excess_loss_db = knife_edge_excess_loss_db_profile(
        dem,
        tx_lat,
        tx_lon,
        rx_lat,
        rx_lon,
        distance_m,
        z_tx_amsl,
        z_rx_amsl,
        ctx.freq_hz,
        ctx.fresnel_frac,
        ctx.clutter,
    );
    let pr_dbm = ctx.eirp_chain - fspl - excess_loss_db;
    LinkResult {
        viable: pr_dbm >= ctx.threshold_dbm,
        distance_m,
        pr_dbm,
        excess_loss_db,
    }
}

pub fn evaluate_link_viable(
    dem: &DemMosaic,
    tx_lat: f64,
    tx_lon: f64,
    rx_lat: f64,
    rx_lon: f64,
    ctx: &LinkContext,
) -> bool {
    evaluate_link(dem, tx_lat, tx_lon, rx_lat, rx_lon, ctx).viable
}

pub fn evaluate_mutual_link_viable(
    dem: &DemMosaic,
    lat_a: f64,
    lon_a: f64,
    lat_b: f64,
    lon_b: f64,
    ctx: &LinkContext,
) -> bool {
    evaluate_link_viable(dem, lat_a, lon_a, lat_b, lon_b, ctx)
        && evaluate_link_viable(dem, lat_b, lon_b, lat_a, lon_a, ctx)
}

pub fn evaluate_mutual_links_parallel(
    dem: &DemMosaic,
    pairs: &[(f64, f64, f64, f64)],
    ctx: &LinkContext,
) -> Vec<bool> {
    pairs
        .par_iter()
        .map(|(lat_a, lon_a, lat_b, lon_b)| {
            evaluate_mutual_link_viable(dem, *lat_a, *lon_a, *lat_b, *lon_b, ctx)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_zero_distance() {
        assert!(haversine_m(36.0, -115.0, 36.0, -115.0).abs() < 1e-6);
    }

    #[test]
    fn link_context_parses_fixture_json() {
        let raw = include_str!("../tests/fixtures/splat_request_hash_fixture.json");
        let ctx = link_context_from_json(raw).expect("fixture RF JSON");
        assert!(ctx.threshold_dbm < 0.0);
        assert!(ctx.max_range_m > 0.0);
    }
}
