//! LoRa modem decode threshold + SPLAT-aligned reliability margin.

use anyhow::{bail, Result};
use statrs::distribution::{ContinuousCDF, Normal};

/// ITU-R P.526 / SPLAT-style log-normal fade margin scale (dB).
pub const RELIABILITY_SIGMA_DB: f64 = 2.375;

/// Semtech-reference sensitivity at 125 kHz BW (dBm).
fn base_sensitivity_dbm(sf: i32) -> Result<f64> {
    Ok(match sf {
        7 => -123.0,
        8 => -126.0,
        9 => -129.0,
        10 => -132.0,
        11 => -134.5,
        12 => -137.0,
        _ => bail!("unsupported modem.spreading_factor {:?}", sf),
    })
}

/// Narrow-band scaling + CR nudge matching Python `_lora_sensitivity_dbm`.
pub fn lora_sensitivity_dbm(sf: i32, bandwidth_khz: f64, coding_rate: i32) -> Result<f64> {
    let mut sens = base_sensitivity_dbm(sf)?;
    if bandwidth_khz <= 0.0 {
        bail!("modem.bandwidth_khz must be positive");
    }
    sens += 10.0 * (bandwidth_khz / 125.0).log10();

    sens += match coding_rate {
        n if n < 5 => -1.0,
        n if n > 5 => 1.0,
        _ => 0.0,
    };

    Ok(sens)
}

fn z_score_pct(p_pct: f64) -> Result<f64> {
    let p = (p_pct / 100.0).clamp(1e-9, 1.0 - 1e-9);
    let norm = Normal::new(0.0, 1.0)?;
    Ok(norm.inverse_cdf(p))
}

pub fn reliability_margin_db(
    situation_pct: f64,
    time_pct: f64,
    sigma_db: Option<f64>,
) -> Result<f64> {
    let sigma = sigma_db.unwrap_or(RELIABILITY_SIGMA_DB);
    let zs = z_score_pct(situation_pct)?;
    let zt = z_score_pct(time_pct)?;
    Ok((zs * zs + zt * zt).sqrt() * sigma)
}

#[derive(Debug, Clone)]
pub struct LoRaModemView<'a> {
    pub spreading_factor: i32,
    pub bandwidth_khz: f64,
    pub coding_rate: i32,
    pub implementation_margin_db: f64,
    pub sensitivity_dbm: Option<&'a f64>,
}

pub fn modem_decode_threshold_dbm(m: &LoRaModemView<'_>) -> Result<f64> {
    let impl_m = m.implementation_margin_db;
    if let Some(s) = m.sensitivity_dbm {
        return Ok(*s + impl_m);
    }
    Ok(
        lora_sensitivity_dbm(m.spreading_factor, m.bandwidth_khz, m.coding_rate)? + impl_m,
    )
}

pub fn effective_signal_threshold_dbm(
    m: &LoRaModemView<'_>,
    situation_pct: f64,
    time_pct: f64,
) -> Result<f64> {
    Ok(modem_decode_threshold_dbm(m)? + reliability_margin_db(situation_pct, time_pct, None)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sf7_62k5_matches_python_float() {
        let s = lora_sensitivity_dbm(7, 62.5, 5).unwrap();
        assert!((s - (-126.01029995663981_f64)).abs() < 1e-10);
    }

    #[test]
    fn effective_meshcore_modem_matches_python_preset_chain() -> Result<()> {
        let minus121 = -121.0_f64;
        let m = LoRaModemView {
            spreading_factor: 7,
            bandwidth_khz: 62.5,
            coding_rate: 5,
            implementation_margin_db: 3.0,
            sensitivity_dbm: Some(&minus121),
        };
        let eff = effective_signal_threshold_dbm(&m, 95.0, 95.0)?;
        let decode = modem_decode_threshold_dbm(&m)?;
        let rm = reliability_margin_db(95.0, 95.0, None)?;
        assert!((decode + rm - eff).abs() < 1e-12);
        assert!((eff + 112.4753360200358_f64).abs() < 1e-10);
        Ok(())
    }
}
