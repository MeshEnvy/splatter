//! Minimal SPLAT-shaped KML for ``parse_lat_lon_box`` / GroundOverlay.

use std::fmt::Write as _;

pub fn ground_overlay_kml(
    name: &str,
    north: f64,
    south: f64,
    east: f64,
    west: f64,
    rotation_deg: f64,
) -> String {
    let mut s = String::new();
    writeln!(s, r#"<?xml version="1.0" encoding="UTF-8"?>"#).unwrap();
    writeln!(s, r#"<kml xmlns="http://www.opengis.net/kml/2.2">"#).unwrap();
    writeln!(s, "  <GroundOverlay>").unwrap();
    writeln!(s, "    <name>{}</name>", escape_xml(name)).unwrap();
    writeln!(s, "    <Icon>").unwrap();
    writeln!(s, "      <href>output.ppm</href>").unwrap();
    writeln!(s, "    </Icon>").unwrap();
    writeln!(s, "    <LatLonBox>").unwrap();
    writeln!(s, "      <north>{:.15}</north>", north).unwrap();
    writeln!(s, "      <south>{:.15}</south>", south).unwrap();
    writeln!(s, "      <east>{:.15}</east>", east).unwrap();
    writeln!(s, "      <west>{:.15}</west>", west).unwrap();
    writeln!(s, "      <rotation>{:.15}</rotation>", rotation_deg).unwrap();
    writeln!(s, "    </LatLonBox>").unwrap();
    writeln!(s, "  </GroundOverlay>").unwrap();
    writeln!(s, "</kml>").unwrap();
    s
}

fn escape_xml(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
