use criterion::{criterion_group, criterion_main, Criterion};

/// Build a minimal CDF-1 NetCDF file with configurable grid size.
/// Grid dimensions: [Time=1, south_north=grid_sz, west_east=grid_sz]
/// Two float variables: T2 and PSFC.
fn build_bench_netcdf(grid_sz: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"CDF");
    bytes.push(1); // CDF-1

    // numrecs = 1
    bytes.extend_from_slice(&1_u32.to_be_bytes());

    // dimensions
    bytes.extend_from_slice(&10_u32.to_be_bytes()); // NC_DIMENSION
    bytes.extend_from_slice(&3_u32.to_be_bytes());
    push_nc_string(&mut bytes, "Time");
    bytes.extend_from_slice(&0_u32.to_be_bytes()); // unlimited
    push_nc_string(&mut bytes, "south_north");
    bytes.extend_from_slice(&grid_sz.to_be_bytes());
    push_nc_string(&mut bytes, "west_east");
    bytes.extend_from_slice(&grid_sz.to_be_bytes());

    // global attrs: DX=3000.0
    bytes.extend_from_slice(&12_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    push_nc_string(&mut bytes, "DX");
    bytes.extend_from_slice(&5_u32.to_be_bytes()); // float
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&3000.0_f32.to_bits().to_be_bytes());

    // variables
    bytes.extend_from_slice(&11_u32.to_be_bytes()); // NC_VARIABLE
    bytes.extend_from_slice(&2_u32.to_be_bytes());

    let n_values = grid_sz as usize * grid_sz as usize;
    let var_bytes = n_values * 4; // float32

    // T2
    push_nc_string(&mut bytes, "T2");
    bytes.extend_from_slice(&3_u32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&2_u32.to_be_bytes());
    bytes.extend_from_slice(&12_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    push_nc_string(&mut bytes, "units");
    bytes.extend_from_slice(&2_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(b"K");
    pad4(&mut bytes, 1);
    bytes.extend_from_slice(&5_u32.to_be_bytes()); // float
    bytes.extend_from_slice(&(var_bytes as u32).to_be_bytes());
    let t2_begin_pos = bytes.len();
    bytes.resize(bytes.len() + 4, 0); // CDF-1: 4-byte offset

    // PSFC
    push_nc_string(&mut bytes, "PSFC");
    bytes.extend_from_slice(&3_u32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&2_u32.to_be_bytes());
    bytes.extend_from_slice(&12_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    push_nc_string(&mut bytes, "units");
    bytes.extend_from_slice(&2_u32.to_be_bytes());
    bytes.extend_from_slice(&2_u32.to_be_bytes());
    bytes.extend_from_slice(b"Pa");
    pad4(&mut bytes, 2);
    bytes.extend_from_slice(&5_u32.to_be_bytes());
    bytes.extend_from_slice(&(var_bytes as u32).to_be_bytes());
    let psfc_begin_pos = bytes.len();
    bytes.resize(bytes.len() + 4, 0);

    // Data
    let t2_begin = bytes.len() as u32;
    for i in 0..n_values {
        let v = 280.0 + (i as f32 % 20.0);
        bytes.extend_from_slice(&v.to_bits().to_be_bytes());
    }

    let psfc_begin = bytes.len() as u32;
    for i in 0..n_values {
        let v = 100000.0 + (i as f32 % 500.0);
        bytes.extend_from_slice(&v.to_bits().to_be_bytes());
    }

    bytes[t2_begin_pos..t2_begin_pos + 4].copy_from_slice(&t2_begin.to_be_bytes());
    bytes[psfc_begin_pos..psfc_begin_pos + 4].copy_from_slice(&psfc_begin.to_be_bytes());

    bytes
}

fn push_nc_string(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u32).to_be_bytes());
    buf.extend_from_slice(s.as_bytes());
    pad4(buf, s.len());
}

fn pad4(buf: &mut Vec<u8>, len: usize) {
    let pad = (4 - (len % 4)) % 4;
    for _ in 0..pad {
        buf.push(0);
    }
}

fn bench_open_and_sample(c: &mut Criterion) {
    let mut group = c.benchmark_group("netcdf_open_and_sample");

    for &grid_sz in &[50, 200, 500] {
        let bytes = build_bench_netcdf(grid_sz);
        let label = format!("{grid_sz}x{grid_sz}");

        // Write to tempfile once for the benchmark iterations.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("wrfout_d01_2026-01-01_00:00:00");
        std::fs::write(&path, &bytes).unwrap();

        group.bench_function(format!("open_{label}"), |b| {
            b.iter(|| {
                let _ = wrfcoin_execution_engine::models::netcdf_parse::extract_wrf_outputs(
                    tmp.path(),
                    &["T2", "PSFC"],
                );
            });
        });
    }

    group.finish();
}

fn bench_multi_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("netcdf_multi_file");

    let grid_sz = 100;
    let bytes = build_bench_netcdf(grid_sz);

    for &n_files in &[1, 4, 8] {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..n_files {
            let path = tmp
                .path()
                .join(format!("wrfout_d01_2026-01-01_{i:02}:00:00"));
            std::fs::write(&path, &bytes).unwrap();
        }

        group.bench_function(format!("{n_files}_files_{grid_sz}x{grid_sz}"), |b| {
            b.iter(|| {
                let _ = wrfcoin_execution_engine::models::netcdf_parse::extract_wrf_outputs(
                    tmp.path(),
                    &["T2", "PSFC"],
                );
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_open_and_sample, bench_multi_file);
criterion_main!(benches);
