use std::collections::BTreeMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;
use memmap2::Mmap;
use rayon::prelude::*;
use rustc_hash::FxHashMap;

use crate::error::ExecutionError;

use super::wrf_common::{PhysicsSuite, WrfDomainConfig};

#[derive(Debug, Clone)]
pub struct WrfOutput {
    pub variables: FxHashMap<String, WrfVariable>,
    pub times: Vec<NaiveDateTime>,
    pub domain: WrfDomainConfig,
}

#[derive(Debug, Clone)]
pub struct WrfVariable {
    pub name: String,
    pub units: String,
    pub dimensions: Vec<String>,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
}

#[derive(Debug, Clone)]
struct AggregateStats {
    min: f64,
    max: f64,
    sum: f64,
    count: u64,
    units: String,
    dimensions: Vec<String>,
}

impl AggregateStats {
    fn new(units: String, dimensions: Vec<String>) -> Self {
        Self {
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            sum: 0.0,
            count: 0,
            units,
            dimensions,
        }
    }

    fn update(&mut self, value: f64) {
        if value.is_nan() {
            return;
        }

        if value < self.min {
            self.min = value;
        }
        if value > self.max {
            self.max = value;
        }
        self.sum += value;
        self.count += 1;
    }

    /// Combine two aggregate statistics. min/max are exact regardless of merge
    /// order. sum (and therefore mean) may differ by a few ULPs when files are
    /// processed in parallel, since floating-point addition is not associative.
    fn merge(&mut self, other: &AggregateStats) {
        if other.count == 0 {
            return;
        }
        if other.min < self.min {
            self.min = other.min;
        }
        if other.max > self.max {
            self.max = other.max;
        }
        self.sum += other.sum;
        self.count += other.count;
    }

    fn to_wrf_variable(&self, name: &str) -> Option<WrfVariable> {
        if self.count == 0 {
            return None;
        }

        Some(WrfVariable {
            name: name.to_string(),
            units: self.units.clone(),
            dimensions: self.dimensions.clone(),
            min: self.min,
            max: self.max,
            mean: self.sum / self.count as f64,
        })
    }
}

pub fn extract_wrf_outputs(
    output_dir: &Path,
    variables: &[&str],
) -> Result<WrfOutput, ExecutionError> {
    let mut wrf_files = discover_wrf_files(output_dir)?;
    if wrf_files.is_empty() {
        return Err(ExecutionError::OutputParsing(format!(
            "no wrfout_d01 files found in {}",
            output_dir.display()
        )));
    }
    wrf_files.sort();

    // Process files in parallel — each file is memory-mapped independently.
    let file_results: Vec<_> = wrf_files
        .par_iter()
        .map(|file| {
            let ts = parse_time_from_filename(file);
            let dataset = NcDataset::open(file)?;
            let config = dataset.to_domain_config();

            let mut local_agg: BTreeMap<String, AggregateStats> = BTreeMap::new();
            for requested in variables {
                let Some(sample) = dataset.sample_variable(requested)? else {
                    continue;
                };
                let entry = local_agg
                    .entry((*requested).to_string())
                    .or_insert_with(|| {
                        AggregateStats::new(sample.units.clone(), sample.dimensions.clone())
                    });
                entry.merge(&sample.stats);
            }

            Ok((ts, config, local_agg))
        })
        .collect::<Result<Vec<_>, ExecutionError>>()?;

    // Merge results sequentially (deterministic order since wrf_files is sorted
    // and rayon's IndexedParallelIterator preserves element order on collect).
    let mut aggregates: BTreeMap<String, AggregateStats> = BTreeMap::new();
    let mut times = Vec::new();
    let mut domain = None;

    for (ts, config, local_agg) in file_results {
        if let Some(t) = ts {
            times.push(t);
        }
        if domain.is_none() {
            domain = Some(config);
        }
        for (name, stats) in local_agg {
            let entry = aggregates.entry(name).or_insert_with(|| {
                AggregateStats::new(stats.units.clone(), stats.dimensions.clone())
            });
            entry.merge(&stats);
        }
    }

    if aggregates.is_empty() {
        return Err(ExecutionError::OutputParsing(format!(
            "none of requested variables found in {}",
            output_dir.display()
        )));
    }

    times.sort_unstable();
    times.dedup();

    let mut out = FxHashMap::default();
    for (name, agg) in aggregates {
        if let Some(v) = agg.to_wrf_variable(&name) {
            out.insert(name, v);
        }
    }

    if out.is_empty() {
        return Err(ExecutionError::OutputParsing(
            "requested variables were present but no numeric data could be summarized".to_string(),
        ));
    }

    Ok(WrfOutput {
        variables: out,
        times,
        domain: domain.unwrap_or_else(default_domain),
    })
}

fn discover_wrf_files(output_dir: &Path) -> Result<Vec<PathBuf>, ExecutionError> {
    let entries = std::fs::read_dir(output_dir).map_err(|e| {
        ExecutionError::OutputParsing(format!("failed reading {}: {e}", output_dir.display()))
    })?;

    let mut files = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|e| ExecutionError::OutputParsing(format!("failed reading dir entry: {e}")))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("wrfout_d01") {
            files.push(entry.path());
        }
    }

    Ok(files)
}

fn parse_time_from_filename(path: &Path) -> Option<NaiveDateTime> {
    let name = path.file_name()?.to_string_lossy();
    let idx = name.find("wrfout_d01_")? + "wrfout_d01_".len();
    let candidate = &name[idx..];

    NaiveDateTime::parse_from_str(candidate, "%Y-%m-%d_%H:%M:%S").ok()
}

fn default_domain() -> WrfDomainConfig {
    let start = chrono::DateTime::from_timestamp(0, 0)
        .expect("unix epoch timestamp should always be valid")
        .naive_utc();
    let end = start + chrono::Duration::hours(1);
    WrfDomainConfig {
        west_east: 100,
        south_north: 100,
        dx_meters: 3000.0,
        dy_meters: 3000.0,
        center_lat: 0.0,
        center_lon: 0.0,
        start_time: start,
        end_time: end,
        physics_suite: PhysicsSuite::Midlatitude,
    }
}

#[derive(Debug, Clone)]
struct VariableSample {
    units: String,
    dimensions: Vec<String>,
    stats: AggregateStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NcType {
    Byte,
    Char,
    Short,
    Int,
    Float,
    Double,
}

impl NcType {
    fn from_u32(v: u32) -> Result<Self, ExecutionError> {
        match v {
            1 => Ok(Self::Byte),
            2 => Ok(Self::Char),
            3 => Ok(Self::Short),
            4 => Ok(Self::Int),
            5 => Ok(Self::Float),
            6 => Ok(Self::Double),
            other => Err(ExecutionError::OutputParsing(format!(
                "unsupported NetCDF type code {other}"
            ))),
        }
    }

    fn width(self) -> usize {
        match self {
            Self::Byte | Self::Char => 1,
            Self::Short => 2,
            Self::Int | Self::Float => 4,
            Self::Double => 8,
        }
    }

    fn is_numeric(self) -> bool {
        self != Self::Char
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NcFormat {
    Classic32,
    Classic64,
}

impl NcFormat {
    fn from_version_byte(label: &str, version: u8) -> Result<Self, ExecutionError> {
        match version {
            1 => Ok(Self::Classic32),
            2 => Ok(Self::Classic64),
            other => Err(ExecutionError::OutputParsing(format!(
                "{label} uses unsupported NetCDF version CDF-{other} (only CDF-1/CDF-2 supported)"
            ))),
        }
    }

    #[cfg(test)]
    fn offset_width(self) -> usize {
        match self {
            Self::Classic32 => 4,
            Self::Classic64 => 8,
        }
    }
}

#[derive(Debug, Clone)]
struct NcDimension {
    name: String,
    len: u32,
    unlimited: bool,
}

#[derive(Debug, Clone)]
enum NcAttrValue {
    String(String),
    Number(f64),
    Bytes,
}

#[derive(Debug, Clone)]
struct NcVariable {
    name: String,
    dim_ids: Vec<usize>,
    attrs: FxHashMap<String, NcAttrValue>,
    xtype: NcType,
    begin: u64,
}

/// Backing store for NetCDF file data. Abstracts over memory-mapped files
/// (production) and owned byte vectors (tests).
enum NcBacking {
    Mapped(Mmap),
    #[cfg(test)]
    Owned(Vec<u8>),
}

impl Deref for NcBacking {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            NcBacking::Mapped(m) => m,
            #[cfg(test)]
            NcBacking::Owned(v) => v,
        }
    }
}

impl std::fmt::Debug for NcBacking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NcBacking::Mapped(m) => write!(f, "Mapped({} bytes)", m.len()),
            #[cfg(test)]
            NcBacking::Owned(v) => write!(f, "Owned({} bytes)", v.len()),
        }
    }
}

#[derive(Debug)]
struct NcDataset {
    bytes: NcBacking,
    num_records: u32,
    dimensions: Vec<NcDimension>,
    global_attrs: FxHashMap<String, NcAttrValue>,
    variables: Vec<NcVariable>,
    record_size: usize,
}

type NcHeader = (
    NcFormat,
    u32,
    Vec<NcDimension>,
    FxHashMap<String, NcAttrValue>,
    Vec<NcVariable>,
    usize,
);

/// Parse the NetCDF header from a byte slice. Shared by `open()` (mmap) and
/// `from_bytes()` (tests).
fn parse_nc_header(bytes: &[u8], path_label: &str) -> Result<NcHeader, ExecutionError> {
    if bytes.len() < 8 {
        return Err(ExecutionError::OutputParsing(format!(
            "{path_label} is too short to be NetCDF"
        )));
    }

    if bytes.starts_with(b"\x89HDF\r\n\x1a\n") {
        return Err(ExecutionError::OutputParsing(format!(
            "{path_label} is NetCDF-4/HDF5; only classic NetCDF (CDF-1/CDF-2) is supported"
        )));
    }

    if &bytes[..3] != b"CDF" {
        return Err(ExecutionError::OutputParsing(format!(
            "{path_label} is not NetCDF classic (missing CDF magic)"
        )));
    }

    let format = NcFormat::from_version_byte(path_label, bytes[3])?;

    let mut r = NcReader::new(bytes);
    r.offset = 4;

    let num_records = r.read_u32()?;
    let dimensions = r.read_dimensions()?;
    let global_attrs = r.read_attributes()?;
    let variables = r.read_variables(format)?;

    let record_size = variables
        .iter()
        .filter(|v| is_record_variable(v, &dimensions))
        .map(|v| padded(v.per_record_bytes(&dimensions, num_records)))
        .sum();

    Ok((format, num_records, dimensions, global_attrs, variables, record_size))
}

impl NcDataset {
    /// Open a NetCDF classic file using memory-mapped I/O. Only the pages
    /// actually accessed (header + requested variable data) are paged into
    /// physical memory by the OS, keeping resident memory proportional to
    /// the data read rather than the file size.
    fn open(path: &Path) -> Result<Self, ExecutionError> {
        let file = std::fs::File::open(path).map_err(|e| {
            ExecutionError::OutputParsing(format!("failed to open {}: {e}", path.display()))
        })?;

        // SAFETY: the file is produced by our Docker container and read
        // immediately after execution. No concurrent modification risk.
        let mmap = unsafe { Mmap::map(&file) }.map_err(|e| {
            ExecutionError::OutputParsing(format!("failed to mmap {}: {e}", path.display()))
        })?;

        let label = path.display().to_string();
        let (_format, num_records, dimensions, global_attrs, variables, record_size) =
            parse_nc_header(&mmap, &label)?;

        Ok(Self {
            bytes: NcBacking::Mapped(mmap),
            num_records,
            dimensions,
            global_attrs,
            variables,
            record_size,
        })
    }

    /// Construct from an owned byte vector (used in tests to avoid file I/O).
    #[cfg(test)]
    fn from_bytes(bytes: Vec<u8>) -> Result<Self, ExecutionError> {
        let (_format, num_records, dimensions, global_attrs, variables, record_size) =
            parse_nc_header(&bytes, "<in-memory>")?;

        Ok(Self {
            bytes: NcBacking::Owned(bytes),
            num_records,
            dimensions,
            global_attrs,
            variables,
            record_size,
        })
    }

    fn to_domain_config(&self) -> WrfDomainConfig {
        let west_east = self
            .find_dimension_len(&["west_east", "west_east_stag"])
            .unwrap_or(100);
        let south_north = self
            .find_dimension_len(&["south_north", "south_north_stag"])
            .unwrap_or(100);

        let dx_meters = self.find_attr_number("DX").unwrap_or(3000.0);
        let dy_meters = self.find_attr_number("DY").unwrap_or(3000.0);
        let center_lat = self
            .find_attr_number("CEN_LAT")
            .or_else(|| self.find_attr_number("MOAD_CEN_LAT"))
            .unwrap_or(0.0);
        let center_lon = self
            .find_attr_number("CEN_LON")
            .or_else(|| self.find_attr_number("STAND_LON"))
            .unwrap_or(0.0);

        let start = chrono::DateTime::from_timestamp(0, 0)
            .expect("unix epoch timestamp should always be valid")
            .naive_utc();
        let end = start + chrono::Duration::hours(1);

        WrfDomainConfig {
            west_east,
            south_north,
            dx_meters,
            dy_meters,
            center_lat,
            center_lon,
            start_time: start,
            end_time: end,
            physics_suite: PhysicsSuite::Midlatitude,
        }
    }

    fn sample_variable(&self, name: &str) -> Result<Option<VariableSample>, ExecutionError> {
        let Some(var) = self.variables.iter().find(|v| v.name == name) else {
            return Ok(None);
        };

        if !var.xtype.is_numeric() {
            return Ok(None);
        }

        let units = match var.attrs.get("units") {
            Some(NcAttrValue::String(s)) => s.clone(),
            _ => String::new(),
        };

        let dimensions = var
            .dim_ids
            .iter()
            .filter_map(|idx| self.dimensions.get(*idx).map(|d| d.name.clone()))
            .collect::<Vec<_>>();

        let mut stats = AggregateStats::new(units.clone(), dimensions.clone());

        if is_record_variable(var, &self.dimensions) {
            let per_record_bytes = var.per_record_bytes(&self.dimensions, self.num_records);
            for record in 0..self.num_records as usize {
                let record_offset = u64::try_from(record.saturating_mul(self.record_size))
                    .map_err(|_| {
                        ExecutionError::OutputParsing(format!(
                            "offset overflow while parsing {name}"
                        ))
                    })?;
                let begin = var.begin.checked_add(record_offset).ok_or_else(|| {
                    ExecutionError::OutputParsing(format!("offset overflow while parsing {name}"))
                })?;
                let (begin_idx, end_idx) =
                    slice_bounds(begin, per_record_bytes, self.bytes.len(), name)?;
                update_stats(var.xtype, &self.bytes[begin_idx..end_idx], &mut stats)?;
            }
        } else {
            let bytes_len = var.total_bytes(&self.dimensions, self.num_records);
            let (begin_idx, end_idx) = slice_bounds(var.begin, bytes_len, self.bytes.len(), name)?;
            update_stats(var.xtype, &self.bytes[begin_idx..end_idx], &mut stats)?;
        }

        Ok(Some(VariableSample {
            units,
            dimensions,
            stats,
        }))
    }

    fn find_dimension_len(&self, candidates: &[&str]) -> Option<u32> {
        self.dimensions
            .iter()
            .find(|d| candidates.iter().any(|c| *c == d.name))
            .map(|d| d.len)
    }

    fn find_attr_number(&self, name: &str) -> Option<f64> {
        match self.global_attrs.get(name) {
            Some(NcAttrValue::Number(v)) => Some(*v),
            _ => None,
        }
    }
}

impl NcVariable {
    fn total_elements(&self, dims: &[NcDimension], num_records: u32) -> usize {
        self.dim_ids.iter().fold(1_usize, |acc, idx| {
            let dim = &dims[*idx];
            let len = if dim.unlimited {
                usize::max(num_records as usize, 1)
            } else {
                dim.len as usize
            };
            acc.saturating_mul(len)
        })
    }

    fn total_bytes(&self, dims: &[NcDimension], num_records: u32) -> usize {
        self.total_elements(dims, num_records)
            .saturating_mul(self.xtype.width())
    }

    fn per_record_bytes(&self, dims: &[NcDimension], num_records: u32) -> usize {
        if !is_record_variable(self, dims) {
            return self.total_bytes(dims, num_records);
        }

        let elems = self.dim_ids.iter().skip(1).fold(1_usize, |acc, idx| {
            let dim = &dims[*idx];
            let len = if dim.unlimited {
                usize::max(num_records as usize, 1)
            } else {
                dim.len as usize
            };
            acc.saturating_mul(len)
        });

        elems.saturating_mul(self.xtype.width())
    }
}

fn is_record_variable(var: &NcVariable, dims: &[NcDimension]) -> bool {
    match var.dim_ids.first() {
        Some(first) => dims.get(*first).map(|d| d.unlimited).unwrap_or(false),
        None => false,
    }
}

fn slice_bounds(
    begin: u64,
    bytes_len: usize,
    file_len: usize,
    name: &str,
) -> Result<(usize, usize), ExecutionError> {
    let bytes_len = u64::try_from(bytes_len).map_err(|_| {
        ExecutionError::OutputParsing(format!("slice overflow while parsing {name}"))
    })?;
    let end = begin.checked_add(bytes_len).ok_or_else(|| {
        ExecutionError::OutputParsing(format!("slice overflow while parsing {name}"))
    })?;
    let file_len = u64::try_from(file_len).map_err(|_| {
        ExecutionError::OutputParsing(format!("slice overflow while parsing {name}"))
    })?;
    if end > file_len {
        return Err(ExecutionError::OutputParsing(format!(
            "variable {name} points outside file bounds"
        )));
    }

    let begin_idx = usize::try_from(begin).map_err(|_| {
        ExecutionError::OutputParsing(format!("slice overflow while parsing {name}"))
    })?;
    let end_idx = usize::try_from(end).map_err(|_| {
        ExecutionError::OutputParsing(format!("slice overflow while parsing {name}"))
    })?;
    Ok((begin_idx, end_idx))
}

fn update_stats(
    xtype: NcType,
    bytes: &[u8],
    stats: &mut AggregateStats,
) -> Result<(), ExecutionError> {
    let width = xtype.width();
    if width == 0 || bytes.len() % width != 0 {
        return Err(ExecutionError::OutputParsing(
            "NetCDF variable byte length is not aligned to type width".to_string(),
        ));
    }

    match xtype {
        NcType::Byte => {
            for b in bytes {
                stats.update(*b as i8 as f64);
            }
        },
        NcType::Short => {
            for chunk in bytes.chunks_exact(2) {
                let v = i16::from_be_bytes([chunk[0], chunk[1]]) as f64;
                stats.update(v);
            }
        },
        NcType::Int => {
            for chunk in bytes.chunks_exact(4) {
                let v = i32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64;
                stats.update(v);
            }
        },
        NcType::Float => {
            for chunk in bytes.chunks_exact(4) {
                let bits = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                stats.update(f32::from_bits(bits) as f64);
            }
        },
        NcType::Double => {
            for chunk in bytes.chunks_exact(8) {
                let bits = u64::from_be_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ]);
                stats.update(f64::from_bits(bits));
            }
        },
        NcType::Char => {},
    }

    Ok(())
}

struct NcReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> NcReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_u32(&mut self) -> Result<u32, ExecutionError> {
        if self.offset + 4 > self.bytes.len() {
            return Err(ExecutionError::OutputParsing(
                "unexpected EOF while reading u32".to_string(),
            ));
        }
        let v = u32::from_be_bytes([
            self.bytes[self.offset],
            self.bytes[self.offset + 1],
            self.bytes[self.offset + 2],
            self.bytes[self.offset + 3],
        ]);
        self.offset += 4;
        Ok(v)
    }

    fn read_u64(&mut self) -> Result<u64, ExecutionError> {
        if self.offset + 8 > self.bytes.len() {
            return Err(ExecutionError::OutputParsing(
                "unexpected EOF while reading u64".to_string(),
            ));
        }
        let v = u64::from_be_bytes([
            self.bytes[self.offset],
            self.bytes[self.offset + 1],
            self.bytes[self.offset + 2],
            self.bytes[self.offset + 3],
            self.bytes[self.offset + 4],
            self.bytes[self.offset + 5],
            self.bytes[self.offset + 6],
            self.bytes[self.offset + 7],
        ]);
        self.offset += 8;
        Ok(v)
    }

    fn read_bytes(&mut self, len: usize) -> Result<Vec<u8>, ExecutionError> {
        if self.offset + len > self.bytes.len() {
            return Err(ExecutionError::OutputParsing(
                "unexpected EOF while reading bytes".to_string(),
            ));
        }
        let out = self.bytes[self.offset..self.offset + len].to_vec();
        self.offset += len;
        Ok(out)
    }

    fn read_nc_string(&mut self) -> Result<String, ExecutionError> {
        let len = self.read_u32()? as usize;
        let data = self.read_bytes(len)?;
        self.skip_padding(len)?;

        Ok(String::from_utf8_lossy(&data)
            .trim_matches('\0')
            .to_string())
    }

    fn skip_padding(&mut self, size: usize) -> Result<(), ExecutionError> {
        let pad = padded(size).saturating_sub(size);
        if pad > 0 {
            self.read_bytes(pad)?;
        }
        Ok(())
    }

    fn read_dimensions(&mut self) -> Result<Vec<NcDimension>, ExecutionError> {
        let tag = self.read_u32()?;
        if tag == 0 {
            let _ = self.read_u32()?;
            return Ok(Vec::new());
        }
        if tag != 10 {
            return Err(ExecutionError::OutputParsing(format!(
                "unexpected dimension tag {tag}"
            )));
        }

        let count = self.read_u32()? as usize;
        let mut dims = Vec::with_capacity(count);

        for _ in 0..count {
            let name = self.read_nc_string()?;
            let len = self.read_u32()?;
            dims.push(NcDimension {
                name,
                len,
                unlimited: len == 0,
            });
        }

        Ok(dims)
    }

    fn read_attributes(&mut self) -> Result<FxHashMap<String, NcAttrValue>, ExecutionError> {
        let tag = self.read_u32()?;
        if tag == 0 {
            let _ = self.read_u32()?;
            return Ok(FxHashMap::default());
        }
        if tag != 12 {
            return Err(ExecutionError::OutputParsing(format!(
                "unexpected attribute tag {tag}"
            )));
        }

        let count = self.read_u32()? as usize;
        let mut attrs = FxHashMap::with_capacity_and_hasher(count, Default::default());

        for _ in 0..count {
            let name = self.read_nc_string()?;
            let xtype = NcType::from_u32(self.read_u32()?)?;
            let nelems = self.read_u32()? as usize;
            let bytes_len = nelems.saturating_mul(xtype.width());
            let data = self.read_bytes(bytes_len)?;
            self.skip_padding(bytes_len)?;

            let value = match xtype {
                NcType::Char => NcAttrValue::String(
                    String::from_utf8_lossy(&data)
                        .trim_matches('\0')
                        .to_string(),
                ),
                NcType::Byte => {
                    if let Some(first) = data.first() {
                        NcAttrValue::Number(*first as i8 as f64)
                    } else {
                        let _ = data;
                        NcAttrValue::Bytes
                    }
                },
                NcType::Short => {
                    if data.len() >= 2 {
                        NcAttrValue::Number(i16::from_be_bytes([data[0], data[1]]) as f64)
                    } else {
                        let _ = data;
                        NcAttrValue::Bytes
                    }
                },
                NcType::Int => {
                    if data.len() >= 4 {
                        NcAttrValue::Number(
                            i32::from_be_bytes([data[0], data[1], data[2], data[3]]) as f64,
                        )
                    } else {
                        let _ = data;
                        NcAttrValue::Bytes
                    }
                },
                NcType::Float => {
                    if data.len() >= 4 {
                        let bits = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                        NcAttrValue::Number(f32::from_bits(bits) as f64)
                    } else {
                        let _ = data;
                        NcAttrValue::Bytes
                    }
                },
                NcType::Double => {
                    if data.len() >= 8 {
                        let bits = u64::from_be_bytes([
                            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                        ]);
                        NcAttrValue::Number(f64::from_bits(bits))
                    } else {
                        let _ = data;
                        NcAttrValue::Bytes
                    }
                },
            };

            attrs.insert(name, value);
        }

        Ok(attrs)
    }

    fn read_variables(&mut self, format: NcFormat) -> Result<Vec<NcVariable>, ExecutionError> {
        let tag = self.read_u32()?;
        if tag == 0 {
            let _ = self.read_u32()?;
            return Ok(Vec::new());
        }
        if tag != 11 {
            return Err(ExecutionError::OutputParsing(format!(
                "unexpected variable tag {tag}"
            )));
        }

        let count = self.read_u32()? as usize;
        let mut vars = Vec::with_capacity(count);

        for _ in 0..count {
            let name = self.read_nc_string()?;
            let ndims = self.read_u32()? as usize;

            let mut dim_ids = Vec::with_capacity(ndims);
            for _ in 0..ndims {
                dim_ids.push(self.read_u32()? as usize);
            }

            let attrs = self.read_attributes()?;
            let xtype = NcType::from_u32(self.read_u32()?)?;
            let _vsize = self.read_u32()?;
            let begin = match format {
                NcFormat::Classic32 => self.read_u32()? as u64,
                NcFormat::Classic64 => self.read_u64()?,
            };

            vars.push(NcVariable {
                name,
                dim_ids,
                attrs,
                xtype,
                begin,
            });
        }

        Ok(vars)
    }
}

fn padded(n: usize) -> usize {
    (n + 3) & !3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wrf_file_stats() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("wrfout_d01_2026-01-01_00:00:00");

        let bytes = build_test_netcdf(NcFormat::Classic32);
        std::fs::write(&path, bytes).unwrap();

        let out = extract_wrf_outputs(tmp.path(), &["T2", "PSFC"]).unwrap();
        assert_eq!(out.variables.len(), 2);
        assert_eq!(out.times.len(), 1);

        let t2 = out.variables.get("T2").unwrap();
        assert_eq!(t2.min, 280.0);
        assert_eq!(t2.max, 283.0);
        assert!((t2.mean - 281.5).abs() < 1e-9);
        assert_eq!(t2.units, "K");
    }

    #[test]
    fn extract_rejects_missing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let err = extract_wrf_outputs(tmp.path(), &["T2"]).unwrap_err();
        assert!(err.to_string().contains("no wrfout_d01 files"));
    }

    #[test]
    fn parse_cdf2_wrf_file_stats() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("wrfout_d01_2026-01-01_03:00:00");

        let bytes = build_test_netcdf(NcFormat::Classic64);
        std::fs::write(&path, bytes).unwrap();

        let out = extract_wrf_outputs(tmp.path(), &["T2", "PSFC"]).unwrap();
        assert_eq!(out.variables.len(), 2);
        assert_eq!(out.times.len(), 1);

        let psfc = out.variables.get("PSFC").unwrap();
        assert_eq!(psfc.units, "Pa");
        assert_eq!(psfc.min, 100000.0);
        assert_eq!(psfc.max, 100300.0);
        assert!((psfc.mean - 100150.0).abs() < 1e-9);
    }

    #[test]
    fn extract_rejects_netcdf4_hdf5_files_explicitly() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("wrfout_d01_2026-01-01_06:00:00");
        std::fs::write(&path, b"\x89HDF\r\n\x1a\nrest").unwrap();

        let err = extract_wrf_outputs(tmp.path(), &["T2"]).unwrap_err();
        assert!(err.to_string().contains("NetCDF-4/HDF5"));
    }

    #[test]
    fn mmap_matches_from_bytes() {
        let bytes = build_test_netcdf(NcFormat::Classic32);

        // Parse via from_bytes (owned Vec).
        let ds_owned = NcDataset::from_bytes(bytes.clone()).unwrap();
        let sample_owned = ds_owned.sample_variable("T2").unwrap().unwrap();

        // Parse via mmap (write to disk, open).
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("wrfout_d01_2026-01-01_12:00:00");
        std::fs::write(&path, &bytes).unwrap();
        let ds_mmap = NcDataset::open(&path).unwrap();
        let sample_mmap = ds_mmap.sample_variable("T2").unwrap().unwrap();

        assert_eq!(sample_owned.stats.min, sample_mmap.stats.min);
        assert_eq!(sample_owned.stats.max, sample_mmap.stats.max);
        assert_eq!(sample_owned.stats.count, sample_mmap.stats.count);
        assert!((sample_owned.stats.sum - sample_mmap.stats.sum).abs() < 1e-9);
    }

    #[test]
    fn aggregate_stats_merge_across_chunks() {
        let mut a = AggregateStats::new("K".to_string(), vec!["Time".to_string()]);
        a.update(10.0);
        a.update(20.0);
        a.update(30.0);

        let mut b = AggregateStats::new("K".to_string(), vec!["Time".to_string()]);
        b.update(5.0);
        b.update(35.0);

        a.merge(&b);

        assert_eq!(a.min, 5.0);
        assert_eq!(a.max, 35.0);
        assert_eq!(a.count, 5);
        assert!((a.sum - 100.0).abs() < 1e-9);
        let var = a.to_wrf_variable("test").unwrap();
        assert!((var.mean - 20.0).abs() < 1e-9);
    }

    #[test]
    fn aggregate_stats_merge_empty_is_noop() {
        let mut a = AggregateStats::new("K".to_string(), vec![]);
        a.update(42.0);
        let b = AggregateStats::new("K".to_string(), vec![]);
        a.merge(&b);
        assert_eq!(a.count, 1);
        assert_eq!(a.min, 42.0);
    }

    #[test]
    fn multi_file_parallel_produces_correct_stats() {
        let tmp = tempfile::tempdir().unwrap();

        // Write 3 synthetic files with different timestamps.
        for (i, ts) in ["00:00:00", "03:00:00", "06:00:00"].iter().enumerate() {
            let path = tmp.path().join(format!("wrfout_d01_2026-01-01_{ts}"));
            let bytes = build_test_netcdf(if i % 2 == 0 {
                NcFormat::Classic32
            } else {
                NcFormat::Classic64
            });
            std::fs::write(&path, bytes).unwrap();
        }

        let out = extract_wrf_outputs(tmp.path(), &["T2", "PSFC"]).unwrap();
        assert_eq!(out.variables.len(), 2);
        assert_eq!(out.times.len(), 3);

        // All files have the same data, so merged stats should match single-file stats.
        let t2 = out.variables.get("T2").unwrap();
        assert_eq!(t2.min, 280.0);
        assert_eq!(t2.max, 283.0);
        // mean = (280+281+282+283)*3 / (4*3) = 3384/12 = 282.0... wait let me recalculate.
        // Each file: 4 values [280, 281, 282, 283], sum=1126, count=4
        // 3 files merged: sum=3378, count=12, mean=281.5
        assert!((t2.mean - 281.5).abs() < 1e-9, "mean={}", t2.mean);
    }

    fn build_test_netcdf(format: NcFormat) -> Vec<u8> {
        // Minimal CDF-1 file with dimensions Time=1 (unlimited), south_north=2, west_east=2
        // and two float variables (T2, PSFC), both shaped [Time, south_north, west_east].

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"CDF");
        bytes.push(match format {
            NcFormat::Classic32 => 1,
            NcFormat::Classic64 => 2,
        });

        // numrecs = 1
        bytes.extend_from_slice(&1_u32.to_be_bytes());

        // dimensions
        bytes.extend_from_slice(&10_u32.to_be_bytes()); // NC_DIMENSION
        bytes.extend_from_slice(&3_u32.to_be_bytes());
        push_nc_string(&mut bytes, "Time");
        bytes.extend_from_slice(&0_u32.to_be_bytes()); // unlimited
        push_nc_string(&mut bytes, "south_north");
        bytes.extend_from_slice(&2_u32.to_be_bytes());
        push_nc_string(&mut bytes, "west_east");
        bytes.extend_from_slice(&2_u32.to_be_bytes());

        // global attrs: DX=3000.0
        bytes.extend_from_slice(&12_u32.to_be_bytes()); // NC_ATTRIBUTE
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        push_nc_string(&mut bytes, "DX");
        bytes.extend_from_slice(&5_u32.to_be_bytes()); // float
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&3000.0_f32.to_bits().to_be_bytes());

        // variables
        bytes.extend_from_slice(&11_u32.to_be_bytes()); // NC_VARIABLE
        bytes.extend_from_slice(&2_u32.to_be_bytes());

        // T2 var
        push_nc_string(&mut bytes, "T2");
        bytes.extend_from_slice(&3_u32.to_be_bytes()); // ndims
        bytes.extend_from_slice(&0_u32.to_be_bytes()); // Time
        bytes.extend_from_slice(&1_u32.to_be_bytes()); // south_north
        bytes.extend_from_slice(&2_u32.to_be_bytes()); // west_east
        bytes.extend_from_slice(&12_u32.to_be_bytes()); // attrs present
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        push_nc_string(&mut bytes, "units");
        bytes.extend_from_slice(&2_u32.to_be_bytes()); // char
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(b"K");
        pad4(&mut bytes, 1);
        bytes.extend_from_slice(&5_u32.to_be_bytes()); // float
        bytes.extend_from_slice(&16_u32.to_be_bytes()); // vsize (4 values * 4)
        let t2_begin_pos = bytes.len();
        bytes.resize(bytes.len() + format.offset_width(), 0);

        // PSFC var
        push_nc_string(&mut bytes, "PSFC");
        bytes.extend_from_slice(&3_u32.to_be_bytes()); // ndims
        bytes.extend_from_slice(&0_u32.to_be_bytes()); // Time
        bytes.extend_from_slice(&1_u32.to_be_bytes()); // south_north
        bytes.extend_from_slice(&2_u32.to_be_bytes()); // west_east
        bytes.extend_from_slice(&12_u32.to_be_bytes()); // attrs
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        push_nc_string(&mut bytes, "units");
        bytes.extend_from_slice(&2_u32.to_be_bytes()); // char
        bytes.extend_from_slice(&2_u32.to_be_bytes());
        bytes.extend_from_slice(b"Pa");
        pad4(&mut bytes, 2);
        bytes.extend_from_slice(&5_u32.to_be_bytes()); // float
        bytes.extend_from_slice(&16_u32.to_be_bytes());
        let psfc_begin_pos = bytes.len();
        bytes.resize(bytes.len() + format.offset_width(), 0);

        // data section starts here
        let t2_begin = bytes.len() as u64;
        for v in [280.0_f32, 281.0, 282.0, 283.0] {
            bytes.extend_from_slice(&v.to_bits().to_be_bytes());
        }

        let psfc_begin = bytes.len() as u64;
        for v in [100000.0_f32, 100100.0, 100200.0, 100300.0] {
            bytes.extend_from_slice(&v.to_bits().to_be_bytes());
        }

        write_begin_offset(&mut bytes, t2_begin_pos, t2_begin, format);
        write_begin_offset(&mut bytes, psfc_begin_pos, psfc_begin, format);

        bytes
    }

    fn write_begin_offset(buf: &mut [u8], pos: usize, begin: u64, format: NcFormat) {
        match format {
            NcFormat::Classic32 => {
                let begin = u32::try_from(begin).unwrap();
                buf[pos..pos + 4].copy_from_slice(&begin.to_be_bytes());
            },
            NcFormat::Classic64 => {
                buf[pos..pos + 8].copy_from_slice(&begin.to_be_bytes());
            },
        }
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
}
