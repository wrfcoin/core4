//! Proving-cost measurement harness for story #2177.
//!
//! Produces the circuit x input-size x (time, memory, proof bytes, key bytes)
//! table published in `docs/zk/PROVING_COSTS.md`.
//!
//! # Why this is not a criterion bench
//!
//! Criterion measures *wall time* well and *memory* not at all. Peak RSS is
//! only meaningful if exactly one thing ran in the process, because
//! `VmHWM` is a monotonic high-water mark that never decreases. So this
//! harness runs each (case, phase) pair in its **own child process** and reads
//! that child's `VmHWM`. The parent orchestrates and aggregates.
//!
//! Splitting by phase also matches how the code is actually deployed:
//!
//! - `setup`   — one-time trusted setup; writes proving and verifying keys to disk.
//! - `prove`   — loads the proving key **from disk** and proves. This is the
//!   prover-side footprint of a station operator, and it is the number that
//!   matters for #1833 / #2038: the OOM there came from generating keys at
//!   request time instead of loading them.
//! - `verify`  — loads the verifying key from disk and verifies. This is the
//!   per-node consensus-path cost.
//!
//! # Usage
//!
//! ```text
//! # full sweep, markdown to stdout
//! cargo run --release -p wrfcoin-zk-circuits --bin zk-proving-report
//!
//! # label the hardware profile and write both artifacts
//! cargo run --release -p wrfcoin-zk-circuits --bin zk-proving-report -- \
//!     --profile "ci-runner" --out costs.md --json costs.json
//!
//! # constrain to one core to emulate a low-end node
//! taskset -c 0 cargo run --release -p wrfcoin-zk-circuits --bin zk-proving-report -- \
//!     --profile "single-core"
//! ```
//!
//! Build in `--release`. Debug-build ark arithmetic is ~50x slower and the
//! resulting numbers are meaningless.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use ark_bn254::Bn254;
use ark_groth16::{Groth16, ProvingKey, VerifyingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;
use wrfcoin_zk_circuits::bench_matrix::{CircuitKind, MATRIX};
use wrfcoin_zk_circuits::budgets;
use wrfcoin_zk_circuits::report_contract::assert_json_contract;
use wrfcoin_zk_circuits::proving;
use wrfcoin_zk_circuits::types::ZkProof;

// Sampling. The first committed revision of this harness timed setup ONCE and
// proved five times with no warmup, then drew architectural conclusions from
// the result. The #2230 review reproduced `range-proof/8` setup at 314/87/90 ms
// against a committed 40 ms — the numbers were not wrong so much as
// unfalsifiable, because a single sample has no spread to compare against.
//
// Every phase now warms up and then reports a distribution.
const SETUP_WARMUP: usize = 1;
const SETUP_ITERS: usize = 5;
const PROVE_WARMUP: usize = 2;
const PROVE_ITERS: usize = 10;
const VERIFY_WARMUP: usize = 5;
const VERIFY_ITERS: usize = 100;
const PROCESS_VK_WARMUP: usize = 2;
const PROCESS_VK_ITERS: usize = 10;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match Options::parse(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            std::process::exit(2);
        },
    };

    if opts.list {
        for kind in MATRIX {
            println!("{}", kind.id());
        }
        return;
    }

    match (&opts.case, &opts.phase) {
        (Some(id), Some(phase)) => run_child(id, phase, &opts.workdir),
        (None, None) => run_parent(&opts),
        _ => {
            eprintln!("error: --case and --phase must be given together\n\n{USAGE}");
            std::process::exit(2);
        },
    }
}

const USAGE: &str = "\
usage: zk-proving-report [options]

  --profile <name>   label for the hardware profile (default: host)
  --cases <a,b,...>  restrict to these case ids (default: the full matrix)
  --out <path>       write the markdown table here (default: stdout)
  --json <path>      also write raw measurements as JSON
  --list             print the matrix case ids and exit

  --case <id> --phase <setup|prove|verify> [--workdir <dir>]
                     internal: run one phase in this process and emit JSON
";

struct Options {
    profile: String,
    cases: Vec<CircuitKind>,
    out: Option<PathBuf>,
    json: Option<PathBuf>,
    list: bool,
    case: Option<String>,
    phase: Option<String>,
    workdir: PathBuf,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut opts = Options {
            profile: "host".to_string(),
            cases: MATRIX.to_vec(),
            out: None,
            json: None,
            list: false,
            case: None,
            phase: None,
            workdir: std::env::temp_dir().join("zk-proving-report-work"),
        };
        let mut i = 0;
        while i < args.len() {
            let take = |i: &mut usize| -> Result<String, String> {
                *i += 1;
                args.get(*i)
                    .cloned()
                    .ok_or_else(|| format!("{} needs a value", args[*i - 1]))
            };
            match args[i].as_str() {
                "--profile" => opts.profile = take(&mut i)?,
                "--out" => opts.out = Some(PathBuf::from(take(&mut i)?)),
                "--json" => opts.json = Some(PathBuf::from(take(&mut i)?)),
                "--workdir" => opts.workdir = PathBuf::from(take(&mut i)?),
                "--case" => opts.case = Some(take(&mut i)?),
                "--phase" => opts.phase = Some(take(&mut i)?),
                "--list" => opts.list = true,
                "--cases" => {
                    let raw = take(&mut i)?;
                    opts.cases = raw
                        .split(',')
                        .map(|id| {
                            CircuitKind::from_id(id.trim())
                                .ok_or_else(|| format!("unknown case id: {id}"))
                        })
                        .collect::<Result<_, _>>()?;
                },
                "-h" | "--help" => {
                    println!("{USAGE}");
                    std::process::exit(0);
                },
                other => return Err(format!("unknown argument: {other}")),
            }
            i += 1;
        }
        Ok(opts)
    }
}

// ---------------------------------------------------------------------------
// Child: one phase, one process, so VmHWM means something.
// ---------------------------------------------------------------------------

fn run_child(id: &str, phase: &str, workdir: &Path) {
    let kind = match CircuitKind::from_id(id) {
        Some(k) => k,
        None => {
            eprintln!("unknown case id: {id}");
            std::process::exit(2);
        },
    };
    let dir = workdir.join(id.replace('/', "_"));
    let result = match phase {
        "setup" => child_setup(kind, &dir),
        "prove" => child_prove(kind, &dir),
        "verify" => child_verify(kind, &dir),
        other => {
            eprintln!("unknown phase: {other}");
            std::process::exit(2);
        },
    };
    match result {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("{id}/{phase}: {e}");
            std::process::exit(1);
        },
    }
}

fn child_setup(kind: CircuitKind, dir: &Path) -> Result<String, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;

    // Constraint synthesis alone, so the table can separate the fixed cost of
    // building the circuit (Poseidon parameter derivation, R1CS allocation)
    // from the Groth16 arithmetic wrapped around it.
    let start = Instant::now();
    let constraints = kind.num_constraints().map_err(|e| e.to_string())?;
    let synth_micros = start.elapsed().as_micros();

    // Warm up: the first key generation in a process pays page-fault and
    // allocator warm-up costs that no subsequent one does, and it was that
    // first sample the original harness reported.
    for _ in 0..SETUP_WARMUP {
        kind.generate_keys().map_err(|e| e.to_string())?;
    }

    let mut setup_samples = Vec::with_capacity(SETUP_ITERS);
    let mut pk = None;
    for _ in 0..SETUP_ITERS {
        let start = Instant::now();
        let keys = kind.generate_keys().map_err(|e| e.to_string())?;
        setup_samples.push(start.elapsed().as_micros());
        pk = Some(keys.0);
    }
    let setup = Stats::from(&mut setup_samples);
    let pk = pk.expect("SETUP_ITERS >= 1");

    // Measure both encodings: compressed is what you ship, uncompressed is
    // what you want on local disk because it loads without point decompression.
    let mut pk_compressed = Vec::new();
    pk.serialize_compressed(&mut pk_compressed)
        .map_err(|e| e.to_string())?;
    let mut pk_uncompressed = Vec::new();
    pk.serialize_uncompressed(&mut pk_uncompressed)
        .map_err(|e| e.to_string())?;

    let mut vk_compressed = Vec::new();
    pk.vk
        .serialize_compressed(&mut vk_compressed)
        .map_err(|e| e.to_string())?;
    let mut vk_uncompressed = Vec::new();
    pk.vk
        .serialize_uncompressed(&mut vk_uncompressed)
        .map_err(|e| e.to_string())?;

    fs::write(dir.join("pk.bin"), &pk_uncompressed).map_err(|e| e.to_string())?;
    fs::write(dir.join("vk.bin"), &vk_uncompressed).map_err(|e| e.to_string())?;

    Ok(format!(
        r#"{{"phase":"setup","constraints":{constraints},"synth_micros":{synth_micros},"setup_micros":{},"setup_min_micros":{},"setup_median_micros":{},"setup_max_micros":{},"setup_samples":{SETUP_ITERS},"pk_compressed_bytes":{},"pk_uncompressed_bytes":{},"vk_compressed_bytes":{},"vk_uncompressed_bytes":{},"peak_rss_kib":{}}}"#,
        setup.median,
        setup.min,
        setup.median,
        setup.max,
        pk_compressed.len(),
        pk_uncompressed.len(),
        vk_compressed.len(),
        vk_uncompressed.len(),
        peak_rss_kib_json(),
    ))
}

fn child_prove(kind: CircuitKind, dir: &Path) -> Result<String, String> {
    let pk_bytes = fs::read(dir.join("pk.bin")).map_err(|e| format!("read pk: {e}"))?;

    // `_unchecked` skips subgroup membership checks. That is correct for a
    // key the node produced or fetched through the trusted-setup channel, and
    // it is the fast path a real prover uses; validating a multi-hundred-MB
    // key on every start would dominate the measurement.
    let start = Instant::now();
    let pk = ProvingKey::<Bn254>::deserialize_uncompressed_unchecked(&pk_bytes[..])
        .map_err(|e| format!("deserialize pk: {e}"))?;
    let key_load_micros = start.elapsed().as_micros();
    drop(pk_bytes);

    // Warm up before timing, for the same reason setup does.
    for _ in 0..PROVE_WARMUP {
        kind.prove(&pk).map_err(|e| e.to_string())?;
    }

    let mut samples = Vec::with_capacity(PROVE_ITERS);
    let mut proof_bytes = 0usize;
    let mut envelope_bytes = 0usize;
    for _ in 0..PROVE_ITERS {
        let start = Instant::now();
        let proof = kind.prove(&pk).map_err(|e| e.to_string())?;
        samples.push(start.elapsed().as_micros());
        proof_bytes = proof.proof_bytes.len();
        envelope_bytes = proof.to_bytes().len();
    }
    let stats = Stats::from(&mut samples);

    // Keep one proof on disk for the verify child.
    let proof = kind.prove(&pk).map_err(|e| e.to_string())?;
    fs::write(dir.join("proof.json"), proof.to_bytes()).map_err(|e| e.to_string())?;

    Ok(format!(
        r#"{{"phase":"prove","key_load_micros":{key_load_micros},"prove_min_micros":{},"prove_median_micros":{},"prove_max_micros":{},"prove_samples":{PROVE_ITERS},"proof_bytes":{proof_bytes},"envelope_bytes":{envelope_bytes},"peak_rss_kib":{}}}"#,
        stats.min,
        stats.median,
        stats.max,
        peak_rss_kib_json(),
    ))
}

fn child_verify(kind: CircuitKind, dir: &Path) -> Result<String, String> {
    let vk_bytes = fs::read(dir.join("vk.bin")).map_err(|e| format!("read vk: {e}"))?;
    let vk = VerifyingKey::<Bn254>::deserialize_uncompressed_unchecked(&vk_bytes[..])
        .map_err(|e| format!("deserialize vk: {e}"))?;

    // `process_vk` was the last single-shot timing in the harness, and finding 2
    // rests entirely on it. A single sample produced a 22x `process_vk`:`verify`
    // ratio on `weather-model/1` against a ~2.3x typical — an outlier that would
    // have gone straight into the document as a finding. Repeat it like every
    // other phase.
    for _ in 0..PROCESS_VK_WARMUP {
        Groth16::<Bn254>::process_vk(&vk).map_err(|e| e.to_string())?;
    }
    let mut pvk_samples = Vec::with_capacity(PROCESS_VK_ITERS);
    let mut pvk = Groth16::<Bn254>::process_vk(&vk).map_err(|e| e.to_string())?;
    for _ in 0..PROCESS_VK_ITERS {
        let start = Instant::now();
        pvk = Groth16::<Bn254>::process_vk(&vk).map_err(|e| e.to_string())?;
        pvk_samples.push(start.elapsed().as_micros());
    }
    let pvk_stats = Stats::from(&mut pvk_samples);
    let process_vk_micros = pvk_stats.median;

    let proof_bytes = fs::read(dir.join("proof.json")).map_err(|e| format!("read proof: {e}"))?;
    let proof = ZkProof::from_bytes(&proof_bytes).map_err(|e| e.to_string())?;
    let inputs = kind.public_inputs();

    for _ in 0..VERIFY_WARMUP {
        if !proving::verify(&proof, &pvk, &inputs).map_err(|e| e.to_string())? {
            return Err("fixture proof failed to verify".to_string());
        }
    }

    let mut samples = Vec::with_capacity(VERIFY_ITERS);
    for _ in 0..VERIFY_ITERS {
        let start = Instant::now();
        let ok = proving::verify(&proof, &pvk, &inputs).map_err(|e| e.to_string())?;
        samples.push(start.elapsed().as_micros());
        if !ok {
            return Err("verification returned false mid-run".to_string());
        }
    }
    let stats = Stats::from(&mut samples);

    Ok(format!(
        r#"{{"phase":"verify","process_vk_micros":{process_vk_micros},"process_vk_min_micros":{},"process_vk_max_micros":{},"process_vk_samples":{PROCESS_VK_ITERS},"verify_min_micros":{},"verify_median_micros":{},"verify_max_micros":{},"verify_samples":{VERIFY_ITERS},"public_inputs":{},"peak_rss_kib":{}}}"#,
        pvk_stats.min,
        pvk_stats.max,
        stats.min,
        stats.median,
        stats.max,
        inputs.len(),
        peak_rss_kib_json(),
    ))
}

struct Stats {
    min: u128,
    median: u128,
    max: u128,
}

impl Stats {
    fn from(samples: &mut [u128]) -> Self {
        samples.sort_unstable();
        Stats {
            min: samples[0],
            median: samples[samples.len() / 2],
            max: samples[samples.len() - 1],
        }
    }
}

/// Peak resident set size of *this* process, in KiB, as a JSON number or
/// `null` where the kernel does not expose it.
///
/// `VmHWM` is the kernel's high-water mark: it only ever increases, which is
/// exactly why each phase gets its own process.
fn peak_rss_kib_json() -> String {
    match peak_rss_kib() {
        Some(kib) => kib.to_string(),
        None => "null".to_string(),
    }
}

fn peak_rss_kib() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kib| kib.parse().ok())
}

// ---------------------------------------------------------------------------
// Parent: orchestrate children, aggregate, render.
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct Measurement {
    id: String,
    family: String,
    input_size: String,
    constraints: u64,
    synth_micros: u64,
    setup_micros: u64,
    setup_min_micros: u64,
    setup_median_micros: u64,
    setup_max_micros: u64,
    setup_samples: u64,
    prove_samples: u64,
    verify_samples: u64,
    pk_compressed_bytes: u64,
    pk_uncompressed_bytes: u64,
    vk_compressed_bytes: u64,
    vk_uncompressed_bytes: u64,
    setup_peak_rss_kib: u64,
    key_load_micros: u64,
    prove_min_micros: u64,
    prove_median_micros: u64,
    prove_max_micros: u64,
    proof_bytes: u64,
    envelope_bytes: u64,
    prove_peak_rss_kib: u64,
    process_vk_micros: u64,
    process_vk_min_micros: u64,
    process_vk_max_micros: u64,
    process_vk_samples: u64,
    verify_min_micros: u64,
    verify_median_micros: u64,
    verify_max_micros: u64,
    public_inputs: u64,
    verify_peak_rss_kib: u64,
}

/// Machine and build state at the moment of a sweep.
///
/// Committed alongside the numbers because the #2230 review's central point
/// was that a reader could not reproduce the tables from what was published.
/// A benchmark figure without the machine state that produced it is an
/// anecdote.
#[cfg_attr(test, derive(Default))]
struct Provenance {
    timestamp: String,
    commit: String,
    rustc: String,
    kernel: String,
    cpu: String,
    cores: String,
    rayon_threads: String,
    loadavg_before: String,
    loadavg_after: String,
}

fn shell(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn read_trim(path: &str) -> String {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn loadavg() -> String {
    read_trim("/proc/loadavg")
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

/// `HEAD`, suffixed `-dirty` when the working tree differs from it.
///
/// A bare `git rev-parse HEAD` stamp is a trap: run the harness with edits in
/// the tree and it records a commit that provably could not have produced the
/// output. The first committed sweeps hit exactly this — they carry
/// `commit: 8b83eae6c` alongside `setup_iters: 5`, but `8b83eae6c` is the
/// single-shot harness and has no such field. Anyone auditing #2176's inputs by
/// checking out that commit would get different output and reasonably conclude
/// the data was fabricated.
///
/// A provenance field that can quietly be wrong is worse than no field, so this
/// one announces the condition instead.
fn commit_stamp() -> String {
    let head = shell("git", &["rev-parse", "--short", "HEAD"]);
    if head == "unknown" {
        return head;
    }
    // Only tracked-file modifications matter; untracked build artifacts do not
    // change what the harness runs.
    match shell("git", &["status", "--porcelain", "--untracked-files=no"]).as_str() {
        "" | "unknown" => head,
        _ => format!("{head}-dirty"),
    }
}

impl Provenance {
    fn capture() -> Self {
        let cpu = fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("model name"))
                    .and_then(|l| l.split_once(':'))
                    .map(|(_, v)| v.trim().to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        Self {
            timestamp: shell("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]),
            commit: commit_stamp(),
            rustc: shell("rustc", &["--version"]),
            kernel: read_trim("/proc/sys/kernel/osrelease"),
            cpu,
            cores: std::thread::available_parallelism()
                .map(|n| n.to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
            rayon_threads: std::env::var("RAYON_NUM_THREADS")
                .unwrap_or_else(|_| "unset (rayon defaults to all cores)".to_string()),
            loadavg_before: loadavg(),
            loadavg_after: String::new(),
        }
    }

    fn markdown(&self, profile: &str) -> String {
        format!(
            "| Field | Value |\n|---|---|\n\
             | Profile | `{profile}` |\n\
             | Captured | {} |\n\
             | Commit | `{}` |\n\
             | rustc | {} |\n\
             | Kernel | {} |\n\
             | CPU | {} |\n\
             | Cores visible | {} |\n\
             | `RAYON_NUM_THREADS` | {} |\n\
             | Load avg before / after | {} / {} |\n\
             | Sampling | setup {SETUP_WARMUP}+{SETUP_ITERS}, prove {PROVE_WARMUP}+{PROVE_ITERS}, verify {VERIFY_WARMUP}+{VERIFY_ITERS}, process_vk {PROCESS_VK_WARMUP}+{PROCESS_VK_ITERS} (warmup+timed) |\n",
            self.timestamp,
            self.commit,
            self.rustc,
            self.kernel,
            self.cpu,
            self.cores,
            self.rayon_threads,
            self.loadavg_before,
            self.loadavg_after,
        )
    }

    fn json(&self, profile: &str) -> String {
        format!(
            r#""profile":"{profile}","timestamp":"{}","commit":"{}","rustc":"{}","kernel":"{}","cpu":"{}","cores":"{}","rayon_threads":"{}","loadavg_before":"{}","loadavg_after":"{}","setup_warmup":{SETUP_WARMUP},"setup_iters":{SETUP_ITERS},"prove_warmup":{PROVE_WARMUP},"prove_iters":{PROVE_ITERS},"verify_warmup":{VERIFY_WARMUP},"verify_iters":{VERIFY_ITERS},"process_vk_warmup":{PROCESS_VK_WARMUP},"process_vk_iters":{PROCESS_VK_ITERS}"#,
            self.timestamp,
            self.commit,
            self.rustc,
            self.kernel,
            self.cpu,
            self.cores,
            self.rayon_threads,
            self.loadavg_before,
            self.loadavg_after,
        )
    }
}

fn run_parent(opts: &Options) {
    let exe = std::env::current_exe().expect("current_exe");
    // Only ever create the work root, never blow it away: `--workdir` is
    // user-supplied and a blanket `remove_dir_all` on it is a footgun. The
    // per-case subdirectories are the only things this harness deletes, and
    // it deletes them by the exact name it just created.
    if let Err(e) = fs::create_dir_all(&opts.workdir) {
        eprintln!(
            "error: cannot use workdir {}: {e}\nhint: pass --workdir <dir>",
            opts.workdir.display()
        );
        std::process::exit(1);
    }

    let mut provenance = Provenance::capture();
    let mut measurements = Vec::new();
    for kind in &opts.cases {
        let id = kind.id();
        eprintln!("=> {id}");
        let mut m = Measurement {
            id: id.clone(),
            family: kind.family().to_string(),
            input_size: kind.input_size(),
            ..Default::default()
        };
        let mut ok = true;
        for phase in ["setup", "prove", "verify"] {
            match run_phase(&exe, &id, phase, &opts.workdir) {
                Ok(fields) => absorb(&mut m, &fields),
                Err(e) => {
                    eprintln!("   !! {phase} failed: {e}");
                    ok = false;
                    break;
                },
            }
        }
        // Proving keys are large; do not accumulate the whole matrix on disk.
        let _ = fs::remove_dir_all(opts.workdir.join(id.replace('/', "_")));
        if ok {
            eprintln!(
                "   constraints={} setup={:.2}s prove={:.0}ms verify={:.2}ms peak_rss(prove)={:.0}MiB",
                m.constraints,
                m.setup_micros as f64 / 1e6,
                m.prove_median_micros as f64 / 1e3,
                m.verify_median_micros as f64 / 1e3,
                m.prove_peak_rss_kib as f64 / 1024.0,
            );
            measurements.push(m);
        }
    }

    provenance.loadavg_after = loadavg();

    let markdown = render_markdown(&opts.profile, &measurements, &provenance);
    match &opts.out {
        Some(path) => {
            fs::write(path, &markdown).expect("write markdown");
            eprintln!("wrote {}", path.display());
        },
        None => print!("{markdown}"),
    }
    if let Some(path) = &opts.json {
        let json = render_json(&opts.profile, &measurements, &provenance);
        // Contract check BEFORE the write: a partial artifact that looks complete is worse
        // than no artifact, because it gets committed and quoted.
        if let Err(e) = assert_json_contract(&json, measurements.len()) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        fs::write(path, json).expect("write json");
        eprintln!("wrote {}", path.display());
    }
}

fn run_phase(
    exe: &Path,
    id: &str,
    phase: &str,
    workdir: &Path,
) -> Result<Vec<(String, u64)>, String> {
    let output = Command::new(exe)
        .args(["--case", id, "--phase", phase, "--workdir"])
        .arg(workdir)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_flat_json(stdout.trim()))
}

/// Minimal parser for the single-level, numbers-and-strings JSON the children
/// emit. Deliberately not `serde_json::from_str` into a struct: the child
/// output is also the on-disk record, and keeping the parse shape-agnostic
/// means adding a field to one phase does not require touching the other two.
fn parse_flat_json(s: &str) -> Vec<(String, u64)> {
    let body = s.trim().trim_start_matches('{').trim_end_matches('}');
    body.split(',')
        .filter_map(|pair| {
            let (k, v) = pair.split_once(':')?;
            let key = k.trim().trim_matches('"').to_string();
            let value = v.trim().parse::<u64>().ok()?;
            Some((key, value))
        })
        .collect()
}

fn absorb(m: &mut Measurement, fields: &[(String, u64)]) {
    for (k, v) in fields {
        let v = *v;
        match k.as_str() {
            "constraints" => m.constraints = v,
            "synth_micros" => m.synth_micros = v,
            "setup_micros" => m.setup_micros = v,
            "setup_min_micros" => m.setup_min_micros = v,
            "setup_median_micros" => m.setup_median_micros = v,
            "setup_max_micros" => m.setup_max_micros = v,
            "setup_samples" => m.setup_samples = v,
            "prove_samples" => m.prove_samples = v,
            "verify_samples" => m.verify_samples = v,
            "pk_compressed_bytes" => m.pk_compressed_bytes = v,
            "pk_uncompressed_bytes" => m.pk_uncompressed_bytes = v,
            "vk_compressed_bytes" => m.vk_compressed_bytes = v,
            "vk_uncompressed_bytes" => m.vk_uncompressed_bytes = v,
            "key_load_micros" => m.key_load_micros = v,
            "prove_min_micros" => m.prove_min_micros = v,
            "prove_median_micros" => m.prove_median_micros = v,
            "prove_max_micros" => m.prove_max_micros = v,
            "proof_bytes" => m.proof_bytes = v,
            "envelope_bytes" => m.envelope_bytes = v,
            "process_vk_micros" => m.process_vk_micros = v,
            "process_vk_min_micros" => m.process_vk_min_micros = v,
            "process_vk_max_micros" => m.process_vk_max_micros = v,
            "process_vk_samples" => m.process_vk_samples = v,
            "verify_min_micros" => m.verify_min_micros = v,
            "verify_median_micros" => m.verify_median_micros = v,
            "verify_max_micros" => m.verify_max_micros = v,
            "public_inputs" => m.public_inputs = v,
            // `peak_rss_kib` is emitted by all three phases. `"phase"` is a
            // string field and `parse_flat_json` keeps only numbers, so route
            // on a numeric field unique to each phase instead.
            "peak_rss_kib" => {
                if fields.iter().any(|(k, _)| k == "setup_micros") {
                    m.setup_peak_rss_kib = v;
                } else if fields.iter().any(|(k, _)| k == "proof_bytes") {
                    m.prove_peak_rss_kib = v;
                } else {
                    m.verify_peak_rss_kib = v;
                }
            },
            _ => {},
        }
    }
}

fn mib(kib: u64) -> f64 {
    kib as f64 / 1024.0
}

fn kib_of_bytes(bytes: u64) -> f64 {
    bytes as f64 / 1024.0
}

fn render_markdown(profile: &str, ms: &[Measurement], prov: &Provenance) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "### Profile: `{profile}`\n");
    let _ = writeln!(s, "{}", prov.markdown(profile));

    let _ = writeln!(
        s,
        "#### Setup (one-time, per circuit) — the #1833 cost\n\n\
         | Case | Input size | Constraints | Setup median | Setup min–max | Peak RSS | PK on disk (uncompressed) | PK (compressed) | VK (uncompressed) |\n\
         |---|---|---:|---:|---:|---:|---:|---:|---:|"
    );
    for m in ms {
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {:.0} ms | {:.0}–{:.0} ms | {:.0} MiB | {:.2} MiB | {:.2} MiB | {:.0} B |",
            m.id,
            m.input_size,
            m.constraints,
            m.setup_median_micros as f64 / 1e3,
            m.setup_min_micros as f64 / 1e3,
            m.setup_max_micros as f64 / 1e3,
            mib(m.setup_peak_rss_kib),
            m.pk_uncompressed_bytes as f64 / 1048576.0,
            m.pk_compressed_bytes as f64 / 1048576.0,
            m.vk_uncompressed_bytes,
        );
    }

    let _ = writeln!(
        s,
        "\nSetup is `{SETUP_WARMUP}` warmup + `{SETUP_ITERS}` timed runs per case.\n\
         \n#### Proving (per proof, key loaded from disk)\n\n\
         | Case | Input size | Key load | Prove (median) | Prove (min–max) | Peak RSS | Proof bytes | Envelope bytes |\n\
         |---|---|---:|---:|---:|---:|---:|---:|"
    );
    for m in ms {
        let _ = writeln!(
            s,
            "| `{}` | {} | {:.0} ms | {:.0} ms | {:.0}–{:.0} ms | {:.0} MiB | {} | {} |",
            m.id,
            m.input_size,
            m.key_load_micros as f64 / 1e3,
            m.prove_median_micros as f64 / 1e3,
            m.prove_min_micros as f64 / 1e3,
            m.prove_max_micros as f64 / 1e3,
            mib(m.prove_peak_rss_kib),
            m.proof_bytes,
            m.envelope_bytes,
        );
    }

    let _ = writeln!(
        s,
        "\n#### Verification (per proof, per node — the consensus-path cost)\n\n\
         | Case | Public inputs | `process_vk` | Verify (median) | Verify (min) | Peak RSS | VK on disk |\n\
         |---|---:|---:|---:|---:|---:|---:|"
    );
    for m in ms {
        let _ = writeln!(
            s,
            "| `{}` | {} | {:.2} ms | {:.3} ms | {:.3} ms | {:.1} MiB | {:.1} KiB |",
            m.id,
            m.public_inputs,
            m.process_vk_micros as f64 / 1e3,
            m.verify_median_micros as f64 / 1e3,
            m.verify_min_micros as f64 / 1e3,
            mib(m.verify_peak_rss_kib),
            kib_of_bytes(m.vk_uncompressed_bytes),
        );
    }

    let over_budget: Vec<&Measurement> = ms
        .iter()
        .filter(|m| {
            m.proof_bytes as usize > budgets::MAX_PROOF_BYTES
                || m.verify_min_micros > budgets::MAX_VERIFY_MILLIS * 1_000
        })
        .collect();
    let _ = writeln!(
        s,
        "\nBudgets (`zk-circuits/src/budgets.rs`): proof <= {} B, verify <= {} ms.",
        budgets::MAX_PROOF_BYTES,
        budgets::MAX_VERIFY_MILLIS
    );
    if over_budget.is_empty() {
        let _ = writeln!(
            s,
            "All {} cases are within budget on this profile.",
            ms.len()
        );
    } else {
        let _ = writeln!(s, "\n**OVER BUDGET on this profile:**");
        for m in over_budget {
            let _ = writeln!(
                s,
                "- `{}`: {} B proof, {:.3} ms verify (min)",
                m.id,
                m.proof_bytes,
                m.verify_min_micros as f64 / 1e3
            );
        }
    }
    s
}

fn render_json(profile: &str, ms: &[Measurement], prov: &Provenance) -> String {
    let rows: Vec<String> = ms
        .iter()
        .map(|m| {
            // Name and value adjacent, deliberately. The previous form was a format string
            // with 30 positional `{}` matched to 30 arguments by order, so adding a field
            // meant editing two places in lockstep — and when `process_vk` gained min/max/
            // samples, the child emitted them and this function silently did not (#2230
            // review). A drifting positional list cannot be reviewed; a key-value list can.
            let numeric: Vec<(&str, u64)> = vec![
                ("constraints", m.constraints),
                ("synth_micros", m.synth_micros),
                ("setup_micros", m.setup_micros),
                ("setup_min_micros", m.setup_min_micros),
                ("setup_median_micros", m.setup_median_micros),
                ("setup_max_micros", m.setup_max_micros),
                ("setup_samples", m.setup_samples),
                ("setup_peak_rss_kib", m.setup_peak_rss_kib),
                ("pk_compressed_bytes", m.pk_compressed_bytes),
                ("pk_uncompressed_bytes", m.pk_uncompressed_bytes),
                ("vk_compressed_bytes", m.vk_compressed_bytes),
                ("vk_uncompressed_bytes", m.vk_uncompressed_bytes),
                ("key_load_micros", m.key_load_micros),
                ("prove_min_micros", m.prove_min_micros),
                ("prove_median_micros", m.prove_median_micros),
                ("prove_max_micros", m.prove_max_micros),
                ("prove_samples", m.prove_samples),
                ("prove_peak_rss_kib", m.prove_peak_rss_kib),
                ("proof_bytes", m.proof_bytes),
                ("envelope_bytes", m.envelope_bytes),
                ("process_vk_micros", m.process_vk_micros),
                ("process_vk_min_micros", m.process_vk_min_micros),
                ("process_vk_max_micros", m.process_vk_max_micros),
                ("process_vk_samples", m.process_vk_samples),
                ("verify_min_micros", m.verify_min_micros),
                ("verify_median_micros", m.verify_median_micros),
                ("verify_max_micros", m.verify_max_micros),
                ("verify_samples", m.verify_samples),
                ("verify_peak_rss_kib", m.verify_peak_rss_kib),
                ("public_inputs", m.public_inputs),
            ];
            let tail: Vec<String> = numeric
                .iter()
                .map(|(k, v)| format!(r#""{k}":{v}"#))
                .collect();
            format!(
                r#"{{"id":"{}","family":"{}","input_size":"{}",{}}}"#,
                m.id,
                m.family,
                m.input_size,
                tail.join(","),
            )
        })
        .collect();
    format!(
        "{{{},\"measurements\":[\n  {}\n]}}\n",
        prov.json(profile),
        rows.join(",\n  ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_zk_circuits::report_contract::PROMISED_JSON_FIELDS;

    /// A rendered row must carry every field the document promises.
    ///
    /// Probes the failure, not just the success: the previous revision of this harness had a
    /// renderer that silently omitted three `process_vk` fields while every other check passed,
    /// so a test that only asserts the happy path would have stayed green through the defect.
    #[test]
    fn json_contract_catches_a_dropped_field() {
        let m = Measurement {
            id: "range-proof/8".to_string(),
            family: "range-proof".to_string(),
            input_size: "8 bits".to_string(),
            ..Default::default()
        };
        let prov = Provenance::default();
        let good = render_json("test", std::slice::from_ref(&m), &prov);
        assert!(
            assert_json_contract(&good, 1).is_ok(),
            "the real renderer must satisfy its own contract: {:?}",
            assert_json_contract(&good, 1)
        );

        // Drop one promised field, exactly as a careless renderer edit would.
        let mutated = good.replace(r#""process_vk_min_micros":0,"#, "");
        assert_ne!(mutated, good, "the probe must actually remove something");
        let err = assert_json_contract(&mutated, 1)
            .expect_err("a dropped promised field must fail the contract");
        assert!(err.contains("process_vk_min_micros"), "got: {err}");

        // And the three fields the #2230 review found missing are named in the contract.
        for f in [
            "process_vk_min_micros",
            "process_vk_max_micros",
            "process_vk_samples",
        ] {
            assert!(PROMISED_JSON_FIELDS.contains(&f), "{f} must be promised");
            assert!(good.contains(&format!(r#""{f}":"#)), "{f} must be rendered");
        }
    }

    /// Multi-row: a field present on one row but missing on another must still fail, or a
    /// partially-broken sweep passes.
    #[test]
    fn json_contract_is_per_row_not_per_file() {
        let m = Measurement::default();
        let prov = Provenance::default();
        let two = render_json("test", &[m.clone(), m], &prov);
        assert!(assert_json_contract(&two, 2).is_ok());
        let one_dropped = two.replacen(r#""process_vk_samples":0,"#, "", 1);
        assert!(
            assert_json_contract(&one_dropped, 2).is_err(),
            "a field missing from ONE of two rows must fail"
        );
    }
}
