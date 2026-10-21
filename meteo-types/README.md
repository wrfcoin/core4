# Canonical weather contracts

This core4 crate is canonical (Refs core4#3242). The standalone meteo-types
repository is reference material. There is one `WeatherObservation` definition:
`wrfcoin_meteo_types::WeatherObservation` is the node/parser type. The historical
`observation::WeatherObservation` import is removed, forcing wider callers to
migrate explicitly rather than silently dropping fields during JSON decoding.

| Contract | Fields | Rust / generated TypeScript name |
| --- | --- | --- |
| Node/parser observation | Temperature, humidity, pressure, wind speed, precipitation, dewpoint, radio provenance | `WeatherObservation` |
| Environmental surface report | The same measurements plus wind direction and visibility | `EnvironmentalWeatherMeasurements` |

`WeatherPayload` remains an alias for the wider measurements used by
`ReportPayload::Weather`. Both positional field layouts, report tag, JSON names,
radio defaults and node parser behavior are retained. The wider type retains its
physical-plausibility and variable-count methods. No conversion discards fields
or adds them to the node type.

Code that previously imported `observation::WeatherObservation` for direction or
visibility must use `EnvironmentalWeatherMeasurements` or `WeatherPayload`.
Node callers use the canonical crate-root observation path.

With `--features ts`, ts-rs exports both contracts under distinct names and
`ReportPayload` references `EnvironmentalWeatherMeasurements`. Radio provenance
remains optional and nullable in both declarations. Export to a fresh directory
using `TS_RS_EXPORT_DIR`; old output directories can retain obsolete files.

Backend's current generated package still represents the wider contract under
its historical observation name. Refs wrfcoin/backend#1425 tracks consumer
migration and the companion binding update. Before regenerating backend bindings,
move wider callers to the wider type, preserve direction/visibility request and
storage behavior, add the barrel export, then run the backend compiler and full
suite. This crate change does not publish new backend generated files.

`canonical_observation_tests` freezes node/report bincode bytes and JSON shape,
checks both ts-rs declarations. Positive and compile-fail doctests pin the public
API migration; restoring the historical module name must fail that gate. The bincode dependency is test-only and already cached in the
workspace lockfile. No consensus field, activation schedule or migration changes.
