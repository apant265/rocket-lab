//! Explicit user-triggered requests. Google credentials never enter mission files or error URLs.
use crate::config::Result;
use serde::{Deserialize, Serialize};
use std::io::Read;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
    #[serde(default)]
    pub mercator: bool,
}
impl Bounds {
    pub fn validate(&self) -> Result<()> {
        if ![self.west, self.south, self.east, self.north]
            .iter()
            .all(|v| v.is_finite())
            || self.west >= self.east
            || self.south >= self.north
            || self.west < -180.0
            || self.east > 180.0
            || self.south < -85.0
            || self.north > 85.0
        {
            return Err(
                "Map bounds must be ordered, within ±85° latitude and must not cross the date line"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn coordinate(&self, x: f64, y: f64) -> (f64, f64) {
        let lon = self.west + (self.east - self.west) * x;
        let lat = if self.mercator {
            let a = mercator_y(self.south);
            let b = mercator_y(self.north);
            inverse_y(b + (a - b) * y)
        } else {
            self.north + (self.south - self.north) * y
        };
        (lat, lon)
    }
    pub fn uv(&self, lat: f64, lon: f64) -> (f64, f64) {
        let x = (lon - self.west) / (self.east - self.west);
        let y = if self.mercator {
            (mercator_y(self.north) - mercator_y(lat))
                / (mercator_y(self.north) - mercator_y(self.south))
        } else {
            (self.north - lat) / (self.north - self.south)
        };
        (x, y)
    }
}
const MERCATOR_R: f64 = 6378137.0;
fn mercator_y(lat: f64) -> f64 {
    MERCATOR_R
        * (std::f64::consts::FRAC_PI_4 + lat.to_radians() * 0.5)
            .tan()
            .ln()
}
fn inverse_y(y: f64) -> f64 {
    (2.0 * (y / MERCATOR_R).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees()
}

#[derive(Clone)]
pub struct Request {
    pub source: usize,
    pub latitude: f64,
    pub longitude: f64,
    pub span_km: f64,
    pub date: String,
    pub image_path: String,
    pub bounds: Bounds,
}
pub struct SatelliteImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub bounds: Bounds,
    pub attribution: String,
    pub source: usize,
}
pub fn fetch(r: Request) -> Result<SatelliteImage> {
    if !r.latitude.is_finite()
        || !r.longitude.is_finite()
        || !r.span_km.is_finite()
        || r.latitude.abs() > 80.0
        || r.longitude.abs() > 180.0
        || !(1.0..=500.0).contains(&r.span_km)
    {
        return Err(
            "Set a finite launch coordinate within ±80° latitude and a map span of 1–500 km".into(),
        );
    }
    let (bytes, bounds, attribution) = if r.source == 2 {
        r.bounds.validate()?;
        let metadata = std::fs::metadata(&r.image_path)?;
        if metadata.len() > 16 * 1024 * 1024 {
            return Err("Offline image exceeds 16 MiB".into());
        }
        (
            std::fs::read(&r.image_path)?,
            r.bounds,
            "User supplied georeferenced image".to_string(),
        )
    } else {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(35))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let (request, bounds, attribution) = if r.source == 1 {
            let key=std::env::var("GOOGLE_MAPS_API_KEY").map_err(|_|"Google satellite requires GOOGLE_MAPS_API_KEY and an enabled Maps Static API billing project. Choose NASA for a key-free map.")?;
            let target = r.span_km * 1000.0 / 640.0;
            let z = (156543.03392804097 * r.latitude.to_radians().cos() / target)
                .log2()
                .floor()
                .clamp(0.0, 20.0);
            let half = 640.0 * 156543.03392804097 / 2.0_f64.powf(z) * 0.5;
            let x = MERCATOR_R * r.longitude.to_radians();
            let y = mercator_y(r.latitude);
            let bounds = Bounds {
                west: ((x - half) / MERCATOR_R).to_degrees(),
                east: ((x + half) / MERCATOR_R).to_degrees(),
                south: inverse_y(y - half),
                north: inverse_y(y + half),
                mercator: true,
            };
            bounds.validate()?;
            let request = client
                .get("https://maps.googleapis.com/maps/api/staticmap")
                .query(&[
                    ("center", format!("{},{}", r.latitude, r.longitude)),
                    ("zoom", format!("{z:.0}")),
                    ("size", "640x640".into()),
                    ("scale", "2".into()),
                    ("maptype", "satellite".into()),
                    ("format", "png".into()),
                    ("key", key),
                ]);
            (
                request,
                bounds,
                "Google Maps · attribution remains on the original image".to_string(),
            )
        } else {
            if r.date.len() != 10
                || r.date.as_bytes()[4] != b'-'
                || r.date.as_bytes()[7] != b'-'
                || !r
                    .date
                    .chars()
                    .enumerate()
                    .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
            {
                return Err("Satellite date must be YYYY-MM-DD".into());
            }
            let half = r.span_km * 500.0;
            let lat_span = (half / crate::atmosphere::EARTH_RADIUS).to_degrees();
            let lon_span = lat_span / r.latitude.to_radians().cos();
            let bounds = Bounds {
                west: r.longitude - lon_span,
                east: r.longitude + lon_span,
                south: r.latitude - lat_span,
                north: r.latitude + lat_span,
                mercator: false,
            };
            bounds.validate()?;
            let request = client
                .get("https://gibs.earthdata.nasa.gov/wms/epsg4326/best/wms.cgi")
                .query(&[
                    ("SERVICE", "WMS".to_string()),
                    ("VERSION", "1.1.1".into()),
                    ("REQUEST", "GetMap".into()),
                    (
                        "LAYERS",
                        "MODIS_Terra_CorrectedReflectance_TrueColor".into(),
                    ),
                    ("STYLES", "".into()),
                    ("SRS", "EPSG:4326".into()),
                    (
                        "BBOX",
                        format!(
                            "{},{},{},{}",
                            bounds.west, bounds.south, bounds.east, bounds.north
                        ),
                    ),
                    ("WIDTH", "1024".into()),
                    ("HEIGHT", "1024".into()),
                    ("FORMAT", "image/png".into()),
                    ("TIME", r.date.clone()),
                ]);
            (
                request,
                bounds,
                format!(
                    "NASA EOSDIS GIBS / MODIS Terra · {} · ~250 m imagery",
                    r.date
                ),
            )
        };
        let mut response = request.send().map_err(|e| e.without_url())?;
        if !response.status().is_success() {
            return Err(format!(
                "Satellite provider returned HTTP {}. Check provider setup, date and coverage.",
                response.status()
            )
            .into());
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut response)
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Satellite download failed while reading the image response")?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Satellite response exceeds 16 MiB".into());
        }
        (bytes, bounds, attribution)
    };
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    let mut reader = reader;
    reader.limits(limits);
    let decoded=reader.decode().map_err(|_|"Provider did not return a usable PNG/JPEG satellite image; check date/coverage or Google API configuration")?.to_rgba8();
    if r.source == 0 {
        std::fs::create_dir_all("output/site")?;
        std::fs::write("output/site/satellite.png", &bytes)?;
        std::fs::write(
            "output/site/bounds.json",
            serde_json::to_string_pretty(&bounds)?,
        )?;
        std::fs::write(
            "output/site/source.json",
            serde_json::to_string_pretty(
                &serde_json::json!({"provider":"NASA EOSDIS GIBS","attribution":attribution,"latitude":r.latitude,"longitude":r.longitude,"span_km":r.span_km,"date":r.date}),
            )?,
        )?;
    }
    Ok(SatelliteImage {
        width: decoded.width(),
        height: decoded.height(),
        rgba: decoded.into_raw(),
        bounds,
        attribution,
        source: r.source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_projection_roundtrips_in_both_image_projections() {
        for mercator in [false, true] {
            let b = Bounds {
                west: -108.0,
                east: -106.0,
                south: 32.0,
                north: 34.0,
                mercator,
            };
            b.validate().unwrap();
            for (x, y) in [(0.0, 0.0), (1.0, 1.0), (0.4, 0.7)] {
                let (lat, lon) = b.coordinate(x, y);
                let (u, v) = b.uv(lat, lon);
                assert!((x - u).abs() < 1e-10 && (y - v).abs() < 1e-10);
            }
        }
        assert!(
            Bounds {
                west: 179.0,
                east: -179.0,
                south: 0.0,
                north: 1.0,
                mercator: false
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn missing_google_key_is_reported_without_a_network_request() {
        if std::env::var_os("GOOGLE_MAPS_API_KEY").is_some() {
            return;
        }
        let r = Request {
            source: 1,
            latitude: 33.0,
            longitude: -107.0,
            span_km: 50.0,
            date: String::new(),
            image_path: String::new(),
            bounds: Bounds {
                west: 0.0,
                south: 0.0,
                east: 1.0,
                north: 1.0,
                mercator: false,
            },
        };
        assert!(
            fetch(r)
                .err()
                .unwrap()
                .to_string()
                .contains("GOOGLE_MAPS_API_KEY")
        );
    }
    #[test]
    fn offline_import_decodes_jpeg_and_keeps_georeferencing() {
        let path = std::env::temp_dir().join(format!("rocket-lab-map-{}.jpg", std::process::id()));
        image::RgbImage::from_pixel(8, 8, image::Rgb([180, 110, 60]))
            .save_with_format(&path, image::ImageFormat::Jpeg)
            .unwrap();
        let bounds = Bounds {
            west: -108.0,
            east: -106.0,
            south: 32.0,
            north: 34.0,
            mercator: false,
        };
        let r = Request {
            source: 2,
            latitude: 33.0,
            longitude: -107.0,
            span_km: 160.0,
            date: String::new(),
            image_path: path.to_string_lossy().into(),
            bounds,
        };
        let data = fetch(r).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!((data.width, data.height), (8, 8));
        assert_eq!(data.bounds.west, -108.0);
        assert!(
            data.rgba
                .chunks_exact(4)
                .all(|p| p[3] == 255 && p[0] > p[2])
        );
    }
}
