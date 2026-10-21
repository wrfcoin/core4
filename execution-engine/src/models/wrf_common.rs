use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike, Utc};

use crate::error::ExecutionError;
use crate::types::{ContainerStep, ModelOutput, ModelRunSpec, PreparedInputs};

use super::gfs_fetch::{GfsCycle, GfsDomain, GfsFetchConfig, fetch_gfs_data};

const DEFAULT_START: (i32, u32, u32, u32) = (2026, 1, 1, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicsSuite {
    Tropical,
    Midlatitude,
    HighRes,
    Fire,
}

#[derive(Debug, Clone)]
pub struct WrfDomainConfig {
    pub west_east: u32,
    pub south_north: u32,
    pub dx_meters: f64,
    pub dy_meters: f64,
    pub center_lat: f64,
    pub center_lon: f64,
    pub start_time: NaiveDateTime,
    pub end_time: NaiveDateTime,
    pub physics_suite: PhysicsSuite,
}

pub fn domain_config_from_spec(
    spec: &ModelRunSpec,
    default_suite: PhysicsSuite,
) -> Result<WrfDomainConfig, ExecutionError> {
    let p = &spec.params;

    let west_east = p
        .get("e_we")
        .or_else(|| p.get("west_east"))
        .and_then(|v| v.as_u64())
        .unwrap_or(100) as u32;
    let south_north = p
        .get("e_sn")
        .or_else(|| p.get("south_north"))
        .and_then(|v| v.as_u64())
        .unwrap_or(100) as u32;

    let dx_meters = p
        .get("dx_meters")
        .or_else(|| p.get("dx"))
        .and_then(|v| v.as_f64())
        .unwrap_or(3000.0);
    let dy_meters = p
        .get("dy_meters")
        .or_else(|| p.get("dy"))
        .and_then(|v| v.as_f64())
        .unwrap_or(dx_meters);

    let center_lat = p
        .get("center_lat")
        .or_else(|| p.get("lat"))
        .and_then(|v| v.as_f64())
        .unwrap_or(39.5)
        .clamp(-90.0, 90.0);

    let center_lon = p
        .get("center_lon")
        .or_else(|| p.get("lon"))
        .and_then(|v| v.as_f64())
        .unwrap_or(-98.35)
        .clamp(-180.0, 180.0);

    let start_time = parse_start_time(spec)?;
    let end_time = start_time + Duration::hours(spec.horizon_hours as i64);

    let physics_suite = parse_physics_suite(p).unwrap_or(default_suite);

    Ok(WrfDomainConfig {
        west_east,
        south_north,
        dx_meters,
        dy_meters,
        center_lat,
        center_lon,
        start_time,
        end_time,
        physics_suite,
    })
}

pub fn generate_namelist(config: &WrfDomainConfig) -> String {
    let run_hours = (config.end_time - config.start_time).num_hours().max(0);

    let (mp_physics, ra_lw_physics, ra_sw_physics, cu_physics, sf_surface_physics, bl_pbl_physics) =
        match config.physics_suite {
            PhysicsSuite::Tropical => (6, 4, 4, 1, 2, 1),
            PhysicsSuite::Midlatitude => (8, 4, 4, 5, 2, 1),
            PhysicsSuite::HighRes => (10, 4, 4, 0, 2, 1),
            PhysicsSuite::Fire => (2, 4, 4, 0, 2, 2),
        };

    format!(
        r#"&time_control
 run_hours = {run_hours},
 start_year = {start_year},
 start_month = {start_month:02},
 start_day = {start_day:02},
 start_hour = {start_hour:02},
 end_year = {end_year},
 end_month = {end_month:02},
 end_day = {end_day:02},
 end_hour = {end_hour:02},
 interval_seconds = 10800,
 history_interval = 60,
 frames_per_outfile = 1000,
 io_form_history = 2,
 io_form_input = 2,
 io_form_boundary = 2,
/
&domains
 max_dom = 1,
 e_we = {west_east},
 e_sn = {south_north},
 e_vert = 40,
 dx = {dx:.0},
 dy = {dy:.0},
 num_metgrid_levels = 34,
/
&physics
 mp_physics = {mp_physics},
 ra_lw_physics = {ra_lw_physics},
 ra_sw_physics = {ra_sw_physics},
 sf_sfclay_physics = 1,
 sf_surface_physics = {sf_surface_physics},
 bl_pbl_physics = {bl_pbl_physics},
 cu_physics = {cu_physics},
/
&dynamics
 w_damping = 0,
 diff_opt = 1,
 km_opt = 4,
 diff_6th_opt = 0,
 base_temp = 290.,
 damp_opt = 0,
/
"#,
        run_hours = run_hours,
        start_year = config.start_time.year(),
        start_month = config.start_time.month(),
        start_day = config.start_time.day(),
        start_hour = config.start_time.hour(),
        end_year = config.end_time.year(),
        end_month = config.end_time.month(),
        end_day = config.end_time.day(),
        end_hour = config.end_time.hour(),
        west_east = config.west_east,
        south_north = config.south_north,
        dx = config.dx_meters,
        dy = config.dy_meters,
        mp_physics = mp_physics,
        ra_lw_physics = ra_lw_physics,
        ra_sw_physics = ra_sw_physics,
        sf_surface_physics = sf_surface_physics,
        bl_pbl_physics = bl_pbl_physics,
        cu_physics = cu_physics,
    )
}

pub fn generate_namelist_wps(config: &WrfDomainConfig) -> String {
    let truelat1 = (config.center_lat - 5.0).clamp(-90.0, 90.0);
    let truelat2 = (config.center_lat + 5.0).clamp(-90.0, 90.0);
    let stand_lon = config.center_lon.clamp(-180.0, 180.0);

    format!(
        r#"&share
 wrf_core = 'ARW',
 max_dom = 1,
 start_date = '{start}',
 end_date = '{end}',
 interval_seconds = 10800,
/
&geogrid
 parent_id = 1,
 parent_grid_ratio = 1,
 i_parent_start = 1,
 j_parent_start = 1,
 e_we = {west_east},
 e_sn = {south_north},
 geog_data_res = 'default',
 dx = {dx:.0},
 dy = {dy:.0},
 map_proj = 'lambert',
 ref_lat = {center_lat:.4},
 ref_lon = {center_lon:.4},
 truelat1 = {truelat1:.4},
 truelat2 = {truelat2:.4},
 stand_lon = {stand_lon:.4},
/
&ungrib
 out_format = 'WPS',
 prefix = 'FILE',
/
&metgrid
 fg_name = 'FILE',
 io_form_metgrid = 2,
/
"#,
        start = format_wrf_datetime(config.start_time),
        end = format_wrf_datetime(config.end_time),
        west_east = config.west_east,
        south_north = config.south_north,
        dx = config.dx_meters,
        dy = config.dy_meters,
        center_lat = config.center_lat,
        center_lon = config.center_lon,
        truelat1 = truelat1,
        truelat2 = truelat2,
        stand_lon = stand_lon,
    )
}

pub async fn prepare_wrf_inputs(
    spec: &ModelRunSpec,
    work_dir: &str,
    config: &WrfDomainConfig,
    namelist_input: &str,
    namelist_wps: &str,
    gfs_endpoint: &str,
    extra_env: Vec<String>,
) -> Result<PreparedInputs, ExecutionError> {
    let run_dir = format!("{work_dir}/run");
    let output_dir = format!("{work_dir}/output");
    let gfs_dir = format!("{run_dir}/gfs");

    tokio::fs::create_dir_all(&run_dir)
        .await
        .map_err(|e| ExecutionError::Model(format!("failed to create run dir: {e}")))?;
    tokio::fs::create_dir_all(&output_dir)
        .await
        .map_err(|e| ExecutionError::Model(format!("failed to create output dir: {e}")))?;
    tokio::fs::create_dir_all(&gfs_dir)
        .await
        .map_err(|e| ExecutionError::Model(format!("failed to create gfs dir: {e}")))?;

    tokio::fs::write(format!("{run_dir}/namelist.input"), namelist_input)
        .await
        .map_err(|e| ExecutionError::Model(format!("failed to write namelist.input: {e}")))?;

    tokio::fs::write(format!("{run_dir}/namelist.wps"), namelist_wps)
        .await
        .map_err(|e| ExecutionError::Model(format!("failed to write namelist.wps: {e}")))?;

    if !spec
        .params
        .get("skip_gfs_fetch")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let cycle_date = config.start_time.date();
        let cycle_hour = config
            .start_time
            .hour()
            .checked_div(6)
            .unwrap_or(0)
            .saturating_mul(6);
        let cycle = GfsCycle::from_hour(cycle_hour);

        let forecast_hours = parse_forecast_hours(spec);
        let domain = build_gfs_domain(config);

        let fetch_cfg = GfsFetchConfig {
            gfs_endpoint: gfs_endpoint.to_string(),
            cycle_date,
            cycle,
            forecast_hours,
            domain,
            output_dir: std::path::PathBuf::from(&gfs_dir),
        };

        fetch_gfs_data(&fetch_cfg).await?;
    }

    let mut env = vec![
        format!("WRF_REGION={}", spec.region),
        format!("WRF_HORIZON={}", spec.horizon_hours),
        format!("WRF_START={}", format_wrf_datetime(config.start_time)),
        format!("WRF_END={}", format_wrf_datetime(config.end_time)),
    ];
    env.extend(extra_env);

    Ok(PreparedInputs {
        steps: vec![
            ContainerStep {
                name: "geogrid.exe".to_string(),
                cmd: vec![
                    "sh".to_string(),
                    "-lc".to_string(),
                    "cd /wrf/run && geogrid.exe".to_string(),
                ],
            },
            ContainerStep {
                name: "ungrib.exe".to_string(),
                cmd: vec![
                    "sh".to_string(),
                    "-lc".to_string(),
                    "cd /wrf/run && ungrib.exe".to_string(),
                ],
            },
            ContainerStep {
                name: "metgrid.exe".to_string(),
                cmd: vec![
                    "sh".to_string(),
                    "-lc".to_string(),
                    "cd /wrf/run && metgrid.exe".to_string(),
                ],
            },
            ContainerStep {
                name: "real.exe".to_string(),
                cmd: vec![
                    "sh".to_string(),
                    "-lc".to_string(),
                    "cd /wrf/run && real.exe".to_string(),
                ],
            },
            ContainerStep {
                name: "wrf.exe".to_string(),
                cmd: vec![
                    "sh".to_string(),
                    "-lc".to_string(),
                    "cd /wrf/run && wrf.exe".to_string(),
                ],
            },
        ],
        binds: vec![
            format!("{run_dir}:/wrf/run:rw"),
            format!("{output_dir}:/data/output:rw"),
        ],
        env,
    })
}

pub async fn extract_wrf_outputs(
    work_dir: &str,
    logs: &str,
    model_label: &str,
    variables: Vec<String>,
) -> Result<ModelOutput, ExecutionError> {
    let output_dir = std::path::PathBuf::from(format!("{work_dir}/output"));
    let run_dir = std::path::PathBuf::from(format!("{work_dir}/run"));

    let requested = variables.iter().map(String::as_str).collect::<Vec<_>>();

    let parsed = super::netcdf_parse::extract_wrf_outputs(&output_dir, &requested)
        .or_else(|_| super::netcdf_parse::extract_wrf_outputs(&run_dir, &requested));

    let mut output_paths = Vec::new();
    let mut total_size: u64 = 0;

    for dir in [&output_dir, &run_dir] {
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            continue;
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("wrfout_d01") {
                continue;
            }

            let Ok(meta) = entry.metadata().await else {
                continue;
            };

            total_size = total_size.saturating_add(meta.len());
            output_paths.push(entry.path().to_string_lossy().to_string());
        }
    }

    output_paths.sort();
    output_paths.dedup();

    if output_paths.is_empty() && (logs.contains("ERROR") || logs.contains("FATAL")) {
        return Err(ExecutionError::OutputParsing(format!(
            "{} run failed. Logs: {}",
            model_label,
            &logs[..logs.len().min(500)]
        )));
    }

    let mut hasher = blake3::Hasher::new();
    for path in &output_paths {
        if let Ok(data) = tokio::fs::read(path).await {
            hasher.update(&data);
        }
    }
    if output_paths.is_empty() {
        hasher.update(logs.as_bytes());
    }

    let variables = match parsed {
        Ok(parsed) => {
            let mut keys = parsed.variables.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            keys
        },
        Err(_) => {
            let mut fallback = variables;
            fallback.sort();
            fallback
        },
    };

    Ok(ModelOutput {
        output_hash: hasher.finalize().to_hex().to_string(),
        variables,
        output_size_bytes: total_size,
        output_paths,
    })
}

pub fn format_wrf_datetime(ts: NaiveDateTime) -> String {
    ts.format("%Y-%m-%d_%H:%M:%S").to_string()
}

fn parse_start_time(spec: &ModelRunSpec) -> Result<NaiveDateTime, ExecutionError> {
    if let Some(start) = spec.params.get("start_time").and_then(|v| v.as_str()) {
        return NaiveDateTime::parse_from_str(start, "%Y-%m-%d_%H:%M:%S").map_err(|e| {
            ExecutionError::Model(format!(
                "invalid start_time '{start}': expected YYYY-MM-DD_HH:MM:SS ({e})"
            ))
        });
    }

    let start_year = spec
        .params
        .get("start_year")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_START.0 as i64) as i32;
    let start_month = spec
        .params
        .get("start_month")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_START.1 as u64) as u32;
    let start_day = spec
        .params
        .get("start_day")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_START.2 as u64) as u32;
    let start_hour = spec
        .params
        .get("start_hour")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_START.3 as u64) as u32;

    let date = NaiveDate::from_ymd_opt(start_year, start_month, start_day).ok_or_else(|| {
        ExecutionError::Model(format!(
            "invalid start date components: year={start_year}, month={start_month}, day={start_day}"
        ))
    })?;

    date.and_hms_opt(start_hour, 0, 0).ok_or_else(|| {
        ExecutionError::Model(format!("invalid start hour: {start_hour}; expected 0..23"))
    })
}

// Forecast inputs may originate in a decoded run specification. Bound both the
// scan and the allocation: a long JSON array must not turn into an unbounded
// allocation in the execution worker.
const MAX_EXPLICIT_FORECAST_HOURS: usize = 256;

fn parse_forecast_hours(spec: &ModelRunSpec) -> Vec<u32> {
    if let Some(hours) = spec.params.get("forecast_hours").and_then(|v| v.as_array()) {
        let mut values = Vec::new();
        for value in hours.iter().take(MAX_EXPLICIT_FORECAST_HOURS) {
            if let Some(hour) = value.as_u64() {
                values.push(hour as u32);
            }
        }
        values.sort_unstable();
        values.dedup();
        if !values.is_empty() {
            return values;
        }
    }

    let horizon = spec.horizon_hours.max(1);
    let mut out = Vec::new();
    let mut h = 0_u32;
    while h <= horizon {
        out.push(h);
        h = h.saturating_add(3);
        if h == 0 {
            break;
        }
    }
    if out.last().copied() != Some(horizon) {
        out.push(horizon);
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn parse_physics_suite(params: &serde_json::Value) -> Option<PhysicsSuite> {
    let s = params.get("physics_suite")?.as_str()?.to_ascii_lowercase();
    match s.as_str() {
        "tropical" => Some(PhysicsSuite::Tropical),
        "midlatitude" => Some(PhysicsSuite::Midlatitude),
        "highres" | "high_res" => Some(PhysicsSuite::HighRes),
        "fire" => Some(PhysicsSuite::Fire),
        _ => None,
    }
}

fn build_gfs_domain(config: &WrfDomainConfig) -> GfsDomain {
    // Rough geographic footprint from domain center and point spacing.
    let km_per_degree_lat = 111.0;
    let half_width_km = (config.west_east as f64 * config.dx_meters) / 2000.0;
    let half_height_km = (config.south_north as f64 * config.dy_meters) / 2000.0;

    let lat_delta = (half_height_km / km_per_degree_lat).max(0.5);
    let cos_lat = config.center_lat.to_radians().cos().abs().max(0.2);
    let lon_delta = (half_width_km / (km_per_degree_lat * cos_lat)).max(0.5);

    GfsDomain {
        leftlon: (config.center_lon - lon_delta).clamp(-180.0, 180.0),
        rightlon: (config.center_lon + lon_delta).clamp(-180.0, 180.0),
        toplat: (config.center_lat + lat_delta).clamp(-90.0, 90.0),
        bottomlat: (config.center_lat - lat_delta).clamp(-90.0, 90.0),
    }
}

pub fn now_utc_naive() -> NaiveDateTime {
    Utc::now().naive_utc()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ResourceLimits;

    fn sample_spec() -> ModelRunSpec {
        ModelRunSpec {
            run_id: "run-1".to_string(),
            model_id: "wrf-nano".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 6,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({
                "start_year": 2026,
                "start_month": 1,
                "start_day": 30,
                "start_hour": 12,
                "e_we": 10,
                "e_sn": 10,
                "skip_gfs_fetch": true,
            }),
        }
    }

    #[test]
    fn domain_config_rolls_end_time() {
        let spec = sample_spec();
        let config = domain_config_from_spec(&spec, PhysicsSuite::Midlatitude).unwrap();
        assert_eq!(config.end_time.year(), 2026);
        assert_eq!(config.end_time.month(), 1);
        assert_eq!(config.end_time.day(), 30);
        assert_eq!(config.end_time.hour(), 18);
    }

    #[test]
    fn namelist_contains_required_sections() {
        let spec = sample_spec();
        let config = domain_config_from_spec(&spec, PhysicsSuite::Midlatitude).unwrap();
        let nl = generate_namelist(&config);

        assert!(nl.contains("&time_control"));
        assert!(nl.contains("&domains"));
        assert!(nl.contains("&physics"));
        assert!(nl.contains("&dynamics"));
        assert!(nl.contains("start_year = 2026"));
    }

    #[test]
    fn namelist_wps_contains_dates_and_projection() {
        let spec = sample_spec();
        let config = domain_config_from_spec(&spec, PhysicsSuite::HighRes).unwrap();
        let wps = generate_namelist_wps(&config);

        assert!(wps.contains("&share"));
        assert!(wps.contains("&geogrid"));
        assert!(wps.contains("map_proj = 'lambert'"));
        assert!(wps.contains("2026-01-30_12:00:00"));
        assert!(wps.contains("2026-01-30_18:00:00"));
    }

    #[test]
    fn parse_forecast_hours_defaults_to_3h_steps() {
        let mut spec = sample_spec();
        spec.horizon_hours = 10;
        spec.params = serde_json::json!({});

        let hours = parse_forecast_hours(&spec);
        assert_eq!(hours, vec![0, 3, 6, 9, 10]);
    }

    #[test]
    fn parse_forecast_hours_bounds_decoded_array_allocation() {
        let mut spec = sample_spec();
        spec.params = serde_json::json!({
            "forecast_hours": (0..(MAX_EXPLICIT_FORECAST_HOURS + 10)).collect::<Vec<_>>()
        });

        let hours = parse_forecast_hours(&spec);
        assert_eq!(hours.len(), MAX_EXPLICIT_FORECAST_HOURS);
        assert_eq!(hours.first(), Some(&0));
        assert_eq!(hours.last(), Some(&((MAX_EXPLICIT_FORECAST_HOURS - 1) as u32)));
    }

    #[tokio::test]
    async fn prepare_wrf_inputs_writes_files_and_steps() {
        let spec = sample_spec();
        let config = domain_config_from_spec(&spec, PhysicsSuite::Midlatitude).unwrap();
        let nl = generate_namelist(&config);
        let wps = generate_namelist_wps(&config);

        let tmp = tempfile::tempdir().unwrap();
        let prepared = prepare_wrf_inputs(
            &spec,
            tmp.path().to_string_lossy().as_ref(),
            &config,
            &nl,
            &wps,
            "https://example.invalid/gfs",
            vec!["EXTRA=1".to_string()],
        )
        .await
        .unwrap();

        assert_eq!(prepared.steps.len(), 5);
        assert_eq!(prepared.steps[0].name, "geogrid.exe");
        assert_eq!(prepared.steps[4].name, "wrf.exe");
        assert!(prepared.env.iter().any(|e| e == "EXTRA=1"));

        let run = tmp.path().join("run");
        assert!(run.join("namelist.input").exists());
        assert!(run.join("namelist.wps").exists());
    }

    #[test]
    fn gfs_domain_is_bounded() {
        let config = WrfDomainConfig {
            west_east: 200,
            south_north: 200,
            dx_meters: 3000.0,
            dy_meters: 3000.0,
            center_lat: 37.0,
            center_lon: -122.0,
            start_time: now_utc_naive(),
            end_time: now_utc_naive() + Duration::hours(1),
            physics_suite: PhysicsSuite::Midlatitude,
        };

        let domain = build_gfs_domain(&config);
        assert!(domain.leftlon < domain.rightlon);
        assert!(domain.bottomlat < domain.toplat);
        assert!(domain.leftlon >= -180.0);
        assert!(domain.rightlon <= 180.0);
    }
}
