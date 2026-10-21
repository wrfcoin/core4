use std::collections::BTreeSet;
use std::path::PathBuf;

use chrono::NaiveDate;
use reqwest::Url;

use crate::error::ExecutionError;

const DEFAULT_GFS_ENDPOINT: &str = "https://nomads.ncep.noaa.gov/cgi-bin/filter_gfs_0p25.pl";

#[derive(Debug, Clone, Copy)]
pub enum GfsCycle {
    Z00,
    Z06,
    Z12,
    Z18,
}

impl GfsCycle {
    pub fn hour_str(self) -> &'static str {
        match self {
            Self::Z00 => "00",
            Self::Z06 => "06",
            Self::Z12 => "12",
            Self::Z18 => "18",
        }
    }

    pub fn from_hour(hour: u32) -> Self {
        match hour {
            0 => Self::Z00,
            6 => Self::Z06,
            12 => Self::Z12,
            18 => Self::Z18,
            _ => Self::Z00,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GfsDomain {
    pub leftlon: f64,
    pub rightlon: f64,
    pub toplat: f64,
    pub bottomlat: f64,
}

#[derive(Debug, Clone)]
pub struct GfsFetchConfig {
    pub gfs_endpoint: String,
    pub cycle_date: NaiveDate,
    pub cycle: GfsCycle,
    pub forecast_hours: Vec<u32>,
    pub domain: GfsDomain,
    pub output_dir: PathBuf,
}

impl GfsFetchConfig {
    pub fn with_defaults(
        cycle_date: NaiveDate,
        cycle: GfsCycle,
        forecast_hours: Vec<u32>,
        domain: GfsDomain,
        output_dir: PathBuf,
    ) -> Self {
        Self {
            gfs_endpoint: DEFAULT_GFS_ENDPOINT.to_string(),
            cycle_date,
            cycle,
            forecast_hours,
            domain,
            output_dir,
        }
    }
}

pub async fn fetch_gfs_data(config: &GfsFetchConfig) -> Result<Vec<PathBuf>, ExecutionError> {
    if config.forecast_hours.is_empty() {
        return Err(ExecutionError::InputDownload(
            "forecast_hours cannot be empty".to_string(),
        ));
    }

    tokio::fs::create_dir_all(&config.output_dir)
        .await
        .map_err(|e| {
            ExecutionError::InputDownload(format!("failed to create GFS output dir: {e}"))
        })?;

    let mut requested: Vec<u32> = config
        .forecast_hours
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    requested.sort_unstable();

    let client = reqwest::Client::new();
    let mut paths = Vec::with_capacity(requested.len());

    for hour in requested {
        let filename = format!(
            "gfs.t{}z.pgrb2.0p25.f{:03}.grib2",
            config.cycle.hour_str(),
            hour
        );
        let path = config.output_dir.join(filename);

        if is_valid_grib_file(&path).await? {
            paths.push(path);
            continue;
        }

        let url = build_nomads_url(config, hour)?;
        let response = client.get(url).send().await.map_err(|e| {
            ExecutionError::InputDownload(format!("failed to fetch GFS hour {hour}: {e}"))
        })?;

        if !response.status().is_success() {
            return Err(ExecutionError::InputDownload(format!(
                "GFS fetch failed for hour {hour}: HTTP {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ExecutionError::InputDownload(format!("failed to read GFS bytes: {e}")))?;

        tokio::fs::write(&path, &bytes).await.map_err(|e| {
            ExecutionError::InputDownload(format!("failed to write {}: {e}", path.display()))
        })?;

        paths.push(path);
    }

    validate_gfs_files(&paths)?;

    let actual_hours = parse_hours_from_paths(&paths)?;
    let expected_hours: BTreeSet<u32> = config.forecast_hours.iter().copied().collect();
    if actual_hours != expected_hours {
        return Err(ExecutionError::InputDownload(format!(
            "GFS temporal coverage mismatch: expected {expected_hours:?}, got {actual_hours:?}"
        )));
    }

    Ok(paths)
}

pub fn validate_gfs_files(paths: &[PathBuf]) -> Result<(), ExecutionError> {
    if paths.is_empty() {
        return Err(ExecutionError::InputDownload(
            "no GFS files were provided".to_string(),
        ));
    }

    let mut seen_hours = BTreeSet::new();

    for path in paths {
        let meta = std::fs::metadata(path).map_err(|e| {
            ExecutionError::InputDownload(format!("failed to stat {}: {e}", path.display()))
        })?;

        if meta.len() <= 1024 {
            return Err(ExecutionError::InputDownload(format!(
                "{} is too small to be GRIB2 ({} bytes)",
                path.display(),
                meta.len()
            )));
        }

        let mut f = std::fs::File::open(path).map_err(|e| {
            ExecutionError::InputDownload(format!("failed to open {}: {e}", path.display()))
        })?;

        let mut magic = [0_u8; 4];
        use std::io::Read;
        f.read_exact(&mut magic).map_err(|e| {
            ExecutionError::InputDownload(format!("failed to read {}: {e}", path.display()))
        })?;

        if &magic != b"GRIB" {
            return Err(ExecutionError::InputDownload(format!(
                "{} does not start with GRIB2 magic bytes",
                path.display()
            )));
        }

        let hour = parse_forecast_hour(path).ok_or_else(|| {
            ExecutionError::InputDownload(format!(
                "{} does not contain a forecast hour in filename",
                path.display()
            ))
        })?;

        if !seen_hours.insert(hour) {
            return Err(ExecutionError::InputDownload(format!(
                "duplicate forecast hour {hour} detected in {path:?}"
            )));
        }
    }

    Ok(())
}

fn build_nomads_url(config: &GfsFetchConfig, forecast_hour: u32) -> Result<Url, ExecutionError> {
    let endpoint = if config.gfs_endpoint.trim().is_empty() {
        DEFAULT_GFS_ENDPOINT
    } else {
        config.gfs_endpoint.as_str()
    };

    let mut url = Url::parse(endpoint)
        .map_err(|e| ExecutionError::InputDownload(format!("invalid GFS endpoint: {e}")))?;

    let date = config.cycle_date.format("%Y%m%d").to_string();
    let cycle = config.cycle.hour_str();
    let file = format!("gfs.t{cycle}z.pgrb2.0p25.f{forecast_hour:03}");

    let mut qp = url.query_pairs_mut();
    qp.append_pair("file", &file)
        .append_pair("lev_2_m_above_ground", "on")
        .append_pair("lev_10_m_above_ground", "on")
        .append_pair("lev_surface", "on")
        .append_pair("var_TMP", "on")
        .append_pair("var_PRES", "on")
        .append_pair("var_RH", "on")
        .append_pair("var_UGRD", "on")
        .append_pair("var_VGRD", "on")
        .append_pair("subregion", "")
        .append_pair("leftlon", &format!("{}", config.domain.leftlon))
        .append_pair("rightlon", &format!("{}", config.domain.rightlon))
        .append_pair("toplat", &format!("{}", config.domain.toplat))
        .append_pair("bottomlat", &format!("{}", config.domain.bottomlat))
        .append_pair("dir", &format!("/gfs.{date}/{cycle}/atmos"));

    drop(qp);
    Ok(url)
}

fn parse_forecast_hour(path: &std::path::Path) -> Option<u32> {
    let name = path.file_name()?.to_string_lossy();
    let marker = ".f";
    let start = name.rfind(marker)? + marker.len();
    let digits: String = name[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .take(3)
        .collect();

    if digits.len() == 3 {
        digits.parse().ok()
    } else {
        None
    }
}

fn parse_hours_from_paths(paths: &[PathBuf]) -> Result<BTreeSet<u32>, ExecutionError> {
    let mut out = BTreeSet::new();
    for path in paths {
        let hour = parse_forecast_hour(path).ok_or_else(|| {
            ExecutionError::InputDownload(format!(
                "{} does not include a parsable forecast hour",
                path.display()
            ))
        })?;
        out.insert(hour);
    }
    Ok(out)
}

async fn is_valid_grib_file(path: &std::path::Path) -> Result<bool, ExecutionError> {
    let meta = match tokio::fs::metadata(path).await {
        Ok(meta) => meta,
        Err(_) => return Ok(false),
    };
    if meta.len() <= 1024 {
        return Ok(false);
    }

    let data = tokio::fs::read(path).await.map_err(|e| {
        ExecutionError::InputDownload(format!("failed reading {}: {e}", path.display()))
    })?;
    Ok(data.len() >= 4 && &data[..4] == b"GRIB")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_bad_magic() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("gfs.t00z.pgrb2.0p25.f000.grib2");
        let mut data = vec![0_u8; 2048];
        data[..4].copy_from_slice(b"NOPE");
        std::fs::write(&path, data).unwrap();

        assert!(validate_gfs_files(&[path]).is_err());
    }

    #[test]
    fn validate_accepts_minimal_grib() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("gfs.t06z.pgrb2.0p25.f003.grib2");
        let mut data = vec![0_u8; 2048];
        data[..4].copy_from_slice(b"GRIB");
        std::fs::write(&path, data).unwrap();

        validate_gfs_files(&[path]).unwrap();
    }

    #[test]
    fn parse_hour_from_filename() {
        let path = PathBuf::from("gfs.t18z.pgrb2.0p25.f012.grib2");
        assert_eq!(parse_forecast_hour(&path), Some(12));
    }
}
