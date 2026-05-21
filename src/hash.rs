//! Canonical input hash matching peaky_finders ``splat_input_hash`` (schema v6).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Must match ``SPLAT_CACHE_SCHEMA_VERSION`` in ``splat_input_hash.py``.
pub const SPLAT_CACHE_SCHEMA_VERSION: i64 = 6;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LoRaModemParams {
    pub spreading_factor: i64,
    pub bandwidth_khz: f64,
    #[serde(default = "default_modem_coding_rate")]
    pub coding_rate: i64,
    #[serde(default)]
    pub implementation_margin_db: f64,
    pub sensitivity_dbm: Option<f64>,
}

fn default_modem_coding_rate() -> i64 {
    5
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Request {
    pub lat: f64,
    pub lon: f64,
    pub tx_height: f64,
    pub tx_power: f64,
    pub tx_gain: f64,
    pub frequency_mhz: f64,
    pub rx_height: f64,
    pub rx_gain: f64,
    pub signal_threshold: f64,
    pub clutter_height: f64,
    #[serde(default = "default_ground_dielectric")]
    pub ground_dielectric: f64,
    #[serde(default = "default_ground_conductivity")]
    pub ground_conductivity: f64,
    #[serde(default = "default_atmosphere_bending")]
    pub atmosphere_bending: f64,
    pub radius: f64,
    #[serde(default)]
    pub system_loss: f64,
    #[serde(default = "default_radio_climate")]
    pub radio_climate: String,
    #[serde(default = "default_polarization")]
    pub polarization: String,
    #[serde(default = "default_situation")]
    pub situation_fraction: f64,
    #[serde(default = "default_time")]
    pub time_fraction: f64,
    #[serde(default = "default_fresnel_clearance_fraction")]
    pub fresnel_clearance_fraction: f64,
    #[serde(default = "default_colormap")]
    pub colormap: String,
    #[serde(default = "default_min_dbm")]
    pub min_dbm: f64,
    #[serde(default = "default_max_dbm")]
    pub max_dbm: f64,
    #[serde(default = "default_high_resolution")]
    pub high_resolution: bool,
    pub modem: LoRaModemParams,
}

fn default_ground_dielectric() -> f64 {
    15.0
}
fn default_ground_conductivity() -> f64 {
    0.005
}
fn default_atmosphere_bending() -> f64 {
    301.0
}
fn default_radio_climate() -> String {
    "continental_temperate".to_string()
}
fn default_polarization() -> String {
    "vertical".to_string()
}
fn default_situation() -> f64 {
    95.0
}
fn default_time() -> f64 {
    95.0
}
fn default_fresnel_clearance_fraction() -> f64 {
    0.6
}
fn default_colormap() -> String {
    "rainbow".to_string()
}
fn default_min_dbm() -> f64 {
    -130.0
}
fn default_max_dbm() -> f64 {
    -30.0
}
fn default_high_resolution() -> bool {
    true
}

pub fn normalize(mut r: Request) -> Request {
    r.high_resolution = true;
    if r.radius > 100_000.0 {
        r.radius = 100_000.0;
    }
    r.fresnel_clearance_fraction = r.fresnel_clearance_fraction.clamp(0.0, 1.0);
    r
}

fn sort_json_value(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            let mut out = serde_json::Map::with_capacity(map.len());
            for k in keys {
                let inner = map.get(&k).unwrap();
                out.insert(k, sort_json_value(inner));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sort_json_value).collect()),
        x => x.clone(),
    }
}

/// SHA-256 hex digest matching ``splat_input_sha256`` / ``normalize_splat_request``.
pub fn splat_input_sha256(req: &Request) -> Result<String> {
    let n = normalize(req.clone());
    let req_val = serde_json::to_value(&n).context("request to JSON Value")?;
    let wrapped = serde_json::json!({
        "v": SPLAT_CACHE_SCHEMA_VERSION,
        "req": req_val,
    });
    let sorted = sort_json_value(&wrapped);
    let blob = serde_json::to_string(&sorted).context("canonical JSON string")?;
    let digest = Sha256::digest(blob.as_bytes());
    Ok(format!("{:x}", digest))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        include_str!("../tests/fixtures/splat_request_hash_fixture.json");
    const GOLDEN: &str =
        "c08c8569a1ab414a053679c7fb0ed9c8726c943449f514472e3ac4f71e27011d";

    #[test]
    fn input_sha256_matches_python_fixture() {
        let r: Request = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(splat_input_sha256(&r).unwrap(), GOLDEN);
    }

    #[test]
    fn rejects_request_json_without_modem() {
        let no_modem = r#"{
            "lat": 36.1,
            "lon": -115.2,
            "tx_height": 2.0,
            "tx_power": 10.0,
            "tx_gain": 2.0,
            "frequency_mhz": 911.525,
            "rx_height": 2.0,
            "rx_gain": 2.0,
            "signal_threshold": -112.0,
            "clutter_height": 1.0,
            "radius": 50000.0
        }"#;
        let err = serde_json::from_str::<Request>(no_modem).unwrap_err();
        assert!(
            err.to_string().contains("modem"),
            "unexpected serde error: {err}"
        );
    }
}
