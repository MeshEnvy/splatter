//! Long-lived splatter session: shared DEM mosaic across many coverage runs.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};

use crate::dem::DemMosaic;
use crate::engine::{run_batch_coverage_with_dem, run_coverage_with_dem, required_tile_names};
use crate::hash::{splat_input_sha256, Request as CovRequest};
use crate::propagate::{
    evaluate_link, evaluate_mutual_link_viable, evaluate_mutual_links_parallel,
    link_context_from_json, required_tile_names_for_points, LinkContext, LinkResult,
};

pub struct Session {
    mirror_root: PathBuf,
    dem: Mutex<DemMosaic>,
    verbose: bool,
}

impl Session {
    pub fn new(mirror_root: PathBuf, verbose: bool) -> Self {
        Self {
            mirror_root,
            dem: Mutex::new(DemMosaic::empty()),
            verbose,
        }
    }

    pub fn mirror_root(&self) -> &Path {
        &self.mirror_root
    }

    pub fn verbose(&self) -> bool {
        self.verbose
    }

    pub fn loaded_tile_count(&self) -> usize {
        self.dem.lock().unwrap().tile_count()
    }

    pub fn preload_tiles(&self, tile_names: &[String]) -> Result<()> {
        self.dem
            .lock()
            .unwrap()
            .ensure_tiles(&self.mirror_root, tile_names, self.verbose)
    }

    pub fn ensure_tiles_for_request(&self, req: &CovRequest) -> Result<()> {
        let tiles = required_tile_names(req.lat, req.lon, req.radius);
        self.preload_tiles(&tiles)
    }

    pub fn ensure_tiles_for_requests(&self, requests: &[CovRequest]) -> Result<()> {
        let mut tile_set: Vec<String> = Vec::new();
        for req in requests {
            tile_set.extend(required_tile_names(req.lat, req.lon, req.radius));
        }
        tile_set.sort();
        tile_set.dedup();
        self.preload_tiles(&tile_set)
    }

    pub fn ensure_tiles_for_points(&self, points: &[(f64, f64)], buffer_m: f64) -> Result<()> {
        let tiles = required_tile_names_for_points(points, buffer_m);
        self.preload_tiles(&tiles)
    }

    pub fn link_context(&self, rf_json: &str) -> Result<LinkContext> {
        link_context_from_json(rf_json)
    }

    pub fn link_eval(
        &self,
        tx_lat: f64,
        tx_lon: f64,
        rx_lat: f64,
        rx_lon: f64,
        rf_json: &str,
    ) -> Result<LinkResult> {
        let ctx = link_context_from_json(rf_json)?;
        let dem = self.dem.lock().unwrap();
        Ok(evaluate_link(
            &dem, tx_lat, tx_lon, rx_lat, rx_lon, &ctx,
        ))
    }

    pub fn link_viable(
        &self,
        tx_lat: f64,
        tx_lon: f64,
        rx_lat: f64,
        rx_lon: f64,
        rf_json: &str,
    ) -> Result<bool> {
        Ok(self
            .link_eval(tx_lat, tx_lon, rx_lat, rx_lon, rf_json)?
            .viable)
    }

    pub fn link_mutual_viable(
        &self,
        lat_a: f64,
        lon_a: f64,
        lat_b: f64,
        lon_b: f64,
        rf_json: &str,
    ) -> Result<bool> {
        let ctx = link_context_from_json(rf_json)?;
        let dem = self.dem.lock().unwrap();
        Ok(evaluate_mutual_link_viable(
            &dem, lat_a, lon_a, lat_b, lon_b, &ctx,
        ))
    }

    pub fn link_mutual_batch(
        &self,
        pairs: &[(f64, f64, f64, f64)],
        rf_json: &str,
    ) -> Result<Vec<bool>> {
        let ctx = link_context_from_json(rf_json)?;
        let dem = self.dem.lock().unwrap();
        Ok(evaluate_mutual_links_parallel(&dem, pairs, &ctx))
    }

    pub fn input_sha256(&self, req: &CovRequest) -> Result<String> {
        splat_input_sha256(req)
    }

    pub fn run(&self, work_dir: &Path) -> Result<()> {
        let req_path = work_dir.join("request.json");
        let raw = std::fs::read_to_string(&req_path)
            .with_context(|| format!("read {}", req_path.display()))?;
        let parsed: CovRequest =
            serde_json::from_str(&raw).context("parse request.json as SplatCoverageRequest")?;
        self.ensure_tiles_for_request(&parsed)?;
        let dem = self.dem.lock().unwrap();
        run_coverage_with_dem(work_dir, &dem, self.verbose)
    }

    pub fn run_batch(&self, work_dir: &Path, batch_jobs: usize, requests_json: Option<&str>) -> Result<()> {
        let requests: Vec<CovRequest> = if let Some(raw) = requests_json {
            serde_json::from_str(raw).context("parse batch requests JSON")?
        } else {
            let req_path = work_dir.join("request.json");
            let raw = std::fs::read_to_string(&req_path)
                .with_context(|| format!("read {}", req_path.display()))?;
            serde_json::from_str(&raw).context("parse request.json as [SplatCoverageRequest]")?
        };
        if requests.is_empty() {
            anyhow::bail!("batch must contain at least one coverage request");
        }
        self.ensure_tiles_for_requests(&requests)?;
        let dem = self.dem.lock().unwrap();
        run_batch_coverage_with_dem(work_dir, &dem, requests, self.verbose, batch_jobs)
    }
}
