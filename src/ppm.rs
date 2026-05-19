//! Binary PPM P6 writer.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};

pub fn write_ppm_rgb(path: &Path, width: u32, height: u32, rgb: &[u8]) -> Result<()> {
    assert_eq!(rgb.len(), (width * height * 3) as usize);
    let f = File::create(path).with_context(|| path.display().to_string())?;
    let mut w = BufWriter::new(f);
    write!(w, "P6\n{} {}\n255\n", width, height).context("ppm header")?;
    w.write_all(rgb).context("ppm body")?;
    w.flush()?;
    Ok(())
}
