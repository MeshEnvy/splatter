//! Public Skadi SRTM tile fetch (``elevation-tiles-prod``), matching ``peaky_finders.skadi_dem``.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};

const SKADI_BUCKET: &str = "elevation-tiles-prod";
const SKADI_PREFIX: &str = "v2/skadi";
const SKADI_FETCH_TIMEOUT: Duration = Duration::from_secs(120);

/// Ensure one ``*.hgt.gz`` exists under ``mirror_root``; download from Skadi S3 on miss.
pub fn ensure_mirror_tile(mirror_root: &Path, tile_name: &str, verbose: bool) -> Result<PathBuf> {
    validate_tile_name(tile_name)?;
    mirror_root
        .try_exists()
        .with_context(|| format!("mirror root {}", mirror_root.display()))?;
    fs::create_dir_all(mirror_root)
        .with_context(|| format!("create mirror root {}", mirror_root.display()))?;

    let path = mirror_root.join(tile_name);
    if path.is_file() {
        return Ok(path);
    }

    if verbose {
        eprintln!("[splatter] DEM fetch {tile_name} (Skadi S3)");
    }
    let bytes = fetch_skadi_hgt_gzip_bytes(tile_name)
        .with_context(|| format!("fetch Skadi tile {tile_name}"))?;
    write_bytes_atomic(&path, &bytes)
        .with_context(|| format!("write mirror tile {}", path.display()))?;
    if verbose {
        eprintln!(
            "[splatter] DEM fetch wrote {} ({} bytes)",
            path.display(),
            bytes.len()
        );
    }
    Ok(path)
}

fn validate_tile_name(tile_name: &str) -> Result<()> {
    if tile_name.is_empty()
        || tile_name.contains('/')
        || tile_name.contains('\\')
        || tile_name.starts_with('.')
        || !tile_name.ends_with(".hgt.gz")
    {
        bail!("unsafe or invalid Skadi tile mirror key: {tile_name:?}");
    }
    Ok(())
}

fn skadi_s3_urls(tile_name: &str) -> [String; 2] {
    let tile_dir_prefix = &tile_name[..3.min(tile_name.len())];
    [
        format!(
            "https://{SKADI_BUCKET}.s3.amazonaws.com/{SKADI_PREFIX}/{tile_dir_prefix}/{tile_name}"
        ),
        format!(
            "https://{SKADI_BUCKET}.s3.amazonaws.com/skadi/{tile_dir_prefix}/{tile_name}"
        ),
    ]
}

fn fetch_skadi_hgt_gzip_bytes(tile_name: &str) -> Result<Vec<u8>> {
    validate_tile_name(tile_name)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(SKADI_FETCH_TIMEOUT)
        .build()
        .context("build HTTP client")?;

    let urls = skadi_s3_urls(tile_name);
    let mut last_err: Option<anyhow::Error> = None;
    for url in &urls {
        match client.get(url).send() {
            Ok(resp) if resp.status().is_success() => {
                return resp
                    .bytes()
                    .context("read Skadi tile body")
                    .map(|b| b.to_vec());
            }
            Ok(resp) if resp.status().as_u16() == 404 => {
                last_err = Some(anyhow::anyhow!("HTTP 404 for {url}"));
            }
            Ok(resp) => {
                last_err = Some(anyhow::anyhow!("HTTP {} for {url}", resp.status()));
            }
            Err(e) => {
                last_err = Some(e.into());
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no Skadi URLs tried for {tile_name}")))
        .with_context(|| format!("Skadi tile {tile_name} not found in public S3 mirror"))
}

fn write_bytes_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_file_name(format!(
        "{}.partial",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("tile")
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_name_validation_rejects_traversal() {
        assert!(validate_tile_name("../N40W119.hgt.gz").is_err());
        assert!(validate_tile_name("N40W119.hgt").is_err());
        assert!(validate_tile_name("N40W119.hgt.gz").is_ok());
    }

    #[test]
    fn skadi_urls_match_python_layout() {
        let urls = skadi_s3_urls("N40W119.hgt.gz");
        assert_eq!(
            urls[0],
            "https://elevation-tiles-prod.s3.amazonaws.com/v2/skadi/N40/N40W119.hgt.gz"
        );
        assert_eq!(
            urls[1],
            "https://elevation-tiles-prod.s3.amazonaws.com/skadi/N40/N40W119.hgt.gz"
        );
    }
}
