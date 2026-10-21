# wrfcoin-execution-engine

## Docker image contract for WRF models

Each WRF model image is expected to provide:

- Executables in `PATH`: `geogrid.exe`, `ungrib.exe`, `metgrid.exe`, `real.exe`, `wrf.exe`
- Writable run mount at `/wrf/run`
- Writable output mount at `/data/output`
- NetCDF outputs named `wrfout_d01_*` in `/wrf/run` and/or `/data/output`

Default model images:

- `wrfcoin/wrf-nano:latest`
- `wrfcoin/wrf-chem:latest`
- `wrfcoin/wrf-fire:latest`
- `wrfcoin/hydro-wrf:latest`
- `wrfcoin/soil-model:latest`
- `wrfcoin/water-quality:latest`

## Running tests

```bash
cargo test -p wrfcoin-execution-engine
cargo test -p wrfcoin-execution-engine --features docker-tests
```
