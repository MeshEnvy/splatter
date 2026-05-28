//! PyO3 bindings for in-process splatter coverage.

use std::path::PathBuf;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyModule;

use crate::hash::Request as CovRequest;
use crate::session::Session;

fn map_err(err: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(format!("{err:#}"))
}

#[pyclass(name = "Session")]
struct PySession {
    inner: Session,
}

#[pymethods]
impl PySession {
    #[new]
    #[pyo3(signature = (mirror_root, *, verbose=false))]
    fn new(mirror_root: String, verbose: bool) -> Self {
        Self {
            inner: Session::new(PathBuf::from(mirror_root), verbose),
        }
    }

    fn preload_tiles(&self, py: Python<'_>, tile_names: Vec<String>) -> PyResult<()> {
        py.allow_threads(|| self.inner.preload_tiles(&tile_names))
            .map_err(map_err)
    }

    fn loaded_tile_count(&self) -> usize {
        self.inner.loaded_tile_count()
    }

    fn mirror_root(&self) -> String {
        self.inner.mirror_root().display().to_string()
    }

    fn verbose(&self) -> bool {
        self.inner.verbose()
    }

    fn input_sha256(&self, request_json: &str) -> PyResult<String> {
        let req: CovRequest =
            serde_json::from_str(request_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        self.inner.input_sha256(&req).map_err(map_err)
    }

    #[pyo3(signature = (work_dir))]
    fn run(&self, py: Python<'_>, work_dir: &str) -> PyResult<()> {
        let path = PathBuf::from(work_dir);
        py.allow_threads(|| self.inner.run(&path)).map_err(map_err)
    }

    #[pyo3(signature = (work_dir, *, batch_jobs=1, requests_json=None))]
    fn run_batch(
        &self,
        py: Python<'_>,
        work_dir: &str,
        batch_jobs: usize,
        requests_json: Option<&str>,
    ) -> PyResult<()> {
        let path = PathBuf::from(work_dir);
        let jobs = batch_jobs.max(1);
        py.allow_threads(|| self.inner.run_batch(&path, jobs, requests_json))
            .map_err(map_err)
    }

    fn ensure_tiles_for_points(
        &self,
        py: Python<'_>,
        points: Vec<(f64, f64)>,
        buffer_m: f64,
    ) -> PyResult<()> {
        py.allow_threads(|| self.inner.ensure_tiles_for_points(&points, buffer_m))
            .map_err(map_err)
    }

    #[pyo3(signature = (lat_a, lon_a, lat_b, lon_b, rf_json))]
    fn link_mutual_viable(
        &self,
        py: Python<'_>,
        lat_a: f64,
        lon_a: f64,
        lat_b: f64,
        lon_b: f64,
        rf_json: &str,
    ) -> PyResult<bool> {
        py.allow_threads(|| {
            self.inner
                .link_mutual_viable(lat_a, lon_a, lat_b, lon_b, rf_json)
        })
        .map_err(map_err)
    }

    #[pyo3(signature = (pairs, rf_json))]
    fn link_mutual_batch(
        &self,
        py: Python<'_>,
        pairs: Vec<(f64, f64, f64, f64)>,
        rf_json: &str,
    ) -> PyResult<Vec<bool>> {
        py.allow_threads(|| self.inner.link_mutual_batch(&pairs, rf_json))
            .map_err(map_err)
    }
}

#[pyfunction]
#[pyo3(signature = (request_json))]
fn input_sha256(request_json: &str) -> PyResult<String> {
    let req: CovRequest =
        serde_json::from_str(request_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
    crate::splat_input_sha256(&req).map_err(map_err)
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySession>()?;
    m.add_function(wrap_pyfunction!(input_sha256, m)?)?;
    m.add("SPLAT_CACHE_SCHEMA_VERSION", crate::SPLAT_CACHE_SCHEMA_VERSION)?;
    Ok(())
}
