//! Precomputed terrain profiles along azimuth rays from the TX site.

use std::f64::consts::TAU;
use std::time::Instant;

use rayon::prelude::*;

use crate::dem::DemMosaic;

pub const EARTH_RADIUS_M: f64 = 6_378_137.0;

const MIN_PROFILE_STEPS: usize = 40;

pub struct RayTerrainCache {
    pub num_rays: usize,
    pub max_steps: usize,
    pub step_m: f64,
    /// ``terrain[ray * (max_steps + 1) + step]`` = AMSL + clutter at ``step * step_m`` along the ray.
    terrain: Vec<f32>,
}

impl RayTerrainCache {
    pub fn build(
        dem: &DemMosaic,
        tx_lat: f64,
        tx_lon: f64,
        radius_m: f64,
        num_rays: usize,
        step_m: f64,
        clutter_m: f64,
        verbose: bool,
    ) -> Self {
        let step_m = step_m.max(1.0);
        let max_steps = ((radius_m / step_m).ceil() as usize).max(MIN_PROFILE_STEPS);
        let stride = max_steps + 1;
        let mut terrain = vec![0.0_f32; num_rays * stride];

        let started = Instant::now();
        terrain
            .par_chunks_mut(stride)
            .enumerate()
            .for_each(|(ray, chunk)| {
                let bearing = TAU * ray as f64 / num_rays as f64;
                for step in 0..=max_steps {
                    let dist = step as f64 * step_m;
                    if dist > radius_m {
                        break;
                    }
                    let (lat, lon) = destination_point(tx_lat, tx_lon, bearing, dist);
                    chunk[step] = (dem.sample_m(lat, lon) + clutter_m) as f32;
                }
            });

        if verbose {
            eprintln!(
                "[splatter] ray terrain cache: {} rays × {} steps ({:.2}s)",
                num_rays,
                max_steps + 1,
                started.elapsed().as_secs_f64()
            );
        }

        Self {
            num_rays,
            max_steps,
            step_m,
            terrain,
        }
    }

    pub fn profile_step_count(&self, distance_m: f64) -> usize {
        if distance_m < 2.0 {
            return 0;
        }
        ((distance_m / self.step_m).ceil() as usize).clamp(MIN_PROFILE_STEPS, self.max_steps)
    }

    /// Bilinear interpolation in (bearing, distance) over the ray cache.
    pub fn terrain_at(&self, bearing_rad: f64, distance_m: f64) -> f64 {
        if distance_m <= 0.0 {
            return self.terrain_at_ray_step(0, 0) as f64;
        }

        let bearing = bearing_rad.rem_euclid(TAU);
        let ray_f = bearing / TAU * self.num_rays as f64;
        let r0 = (ray_f.floor() as usize) % self.num_rays;
        let r1 = (r0 + 1) % self.num_rays;
        let rf = (ray_f - ray_f.floor()) as f32;

        let step_f = (distance_m / self.step_m).clamp(0.0, self.max_steps as f64);
        let s0 = step_f.floor() as usize;
        let s1 = s0.min(self.max_steps.saturating_sub(1)) + 1;
        let sf = (step_f - step_f.floor()) as f32;

        let t00 = self.terrain_at_ray_step(r0, s0);
        let t10 = self.terrain_at_ray_step(r1, s0);
        let t01 = self.terrain_at_ray_step(r0, s1);
        let t11 = self.terrain_at_ray_step(r1, s1);

        let one_rf = 1.0_f32 - rf;
        let t0 = t00 * one_rf + t10 * rf;
        let t1 = t01 * one_rf + t11 * rf;
        let one_sf = 1.0_f32 - sf;
        (t0 * one_sf + t1 * sf) as f64
    }

    fn terrain_at_ray_step(&self, ray: usize, step: usize) -> f32 {
        let step = step.min(self.max_steps);
        self.terrain[ray * (self.max_steps + 1) + step]
    }
}

/// Initial bearing from point 1 to point 2 (radians, clockwise from north).
pub fn initial_bearing_rad(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1r = lat1.to_radians();
    let lat2r = lat2.to_radians();
    let dl = (lon2 - lon1).to_radians();
    let y = dl.sin() * lat2r.cos();
    let x = lat1r.cos() * lat2r.sin() - lat1r.sin() * lat2r.cos() * dl.cos();
    y.atan2(x)
}

/// Great-circle destination from ``(lat, lon)`` given initial bearing and distance.
pub fn destination_point(lat: f64, lon: f64, bearing_rad: f64, distance_m: f64) -> (f64, f64) {
    let lat1 = lat.to_radians();
    let lon1 = lon.to_radians();
    let ang = distance_m / EARTH_RADIUS_M;
    let lat2 = (lat1.sin() * ang.cos() + lat1.cos() * ang.sin() * bearing_rad.cos()).asin();
    let lon2 = lon1
        + (bearing_rad.sin() * ang.sin() * lat1.cos())
            .atan2(ang.cos() - lat1.sin() * lat2.sin());
    (lat2.to_degrees(), lon2.to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_at_zero_distance_is_origin() {
        let (la, lo) = destination_point(36.0, -115.0, 0.5, 0.0);
        assert!((la - 36.0).abs() < 1e-9);
        assert!((lo - (-115.0)).abs() < 1e-9);
    }

    #[test]
    fn bearing_north_is_zero() {
        let b = initial_bearing_rad(36.0, -115.0, 37.0, -115.0);
        assert!(b.abs() < 1e-6);
    }
}
