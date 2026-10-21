use std::env;
use std::path::PathBuf;

use wrfcoin_execution_engine::models::netcdf_parse::extract_wrf_outputs;

fn main() {
    let mut args = env::args().skip(1);
    let Some(path_arg) = args.next() else {
        eprintln!(
            "usage: cargo run -p wrfcoin-execution-engine --example replay_wrf_output -- <output-dir> [VAR ...]"
        );
        std::process::exit(2);
    };

    let requested = {
        let vars = args.collect::<Vec<_>>();
        if vars.is_empty() {
            vec![
                "T2".to_string(),
                "PSFC".to_string(),
                "U10".to_string(),
                "V10".to_string(),
            ]
        } else {
            vars
        }
    };

    let output_dir = PathBuf::from(path_arg);
    let requested_refs = requested.iter().map(String::as_str).collect::<Vec<_>>();

    match extract_wrf_outputs(&output_dir, &requested_refs) {
        Ok(out) => {
            println!(
                "parsed {} variable(s) from {}",
                out.variables.len(),
                output_dir.display()
            );
            println!(
                "domain west_east={} south_north={} dx_meters={} dy_meters={} center_lat={} center_lon={}",
                out.domain.west_east,
                out.domain.south_north,
                out.domain.dx_meters,
                out.domain.dy_meters,
                out.domain.center_lat,
                out.domain.center_lon
            );
            println!("times={:?}", out.times);

            let mut keys = out.variables.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            for key in keys {
                let var = &out.variables[&key];
                println!(
                    "{key}: units={} dims={:?} min={} max={} mean={}",
                    var.units, var.dimensions, var.min, var.max, var.mean
                );
            }
        },
        Err(err) => {
            eprintln!("parse failed for {}: {err}", output_dir.display());
            std::process::exit(1);
        },
    }
}
