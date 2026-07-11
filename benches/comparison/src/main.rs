use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::hint::black_box;
use std::mem;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use cedarwood::Cedar;
use crawdad::Trie as Crawdad;
use daachorse::DoubleArrayAhoCorasick;
use fst::Map as FstMap;
use sha2::{Digest, Sha256};
use yada::{builder::DoubleArrayBuilder, DoubleArray as Yada};

#[path = "../../support/mod.rs"]
mod support;

const CEDARWOOD_MANIFEST: &str = include_str!("../../../Cargo.toml");
const CEDARWOOD_SOURCE: &[u8] = include_bytes!("../../../src/lib.rs");
const HARNESS_SOURCE: &[u8] = include_bytes!("main.rs");
const SUPPORT_SOURCE: &[u8] = include_bytes!("../../support/mod.rs");
const COMPARISON_MANIFEST: &str = include_str!("../Cargo.toml");
const COMPARISON_LOCK: &str = include_str!("../Cargo.lock");
const DATASET_PATH: &str = "benches/macro-benchmark/dict.txt";
const BUILD_SAMPLES: usize = 3;
const EXACT_SAMPLES: usize = 7;
const EXACT_ROUNDS: usize = 1_024;
const SCAN_SAMPLES: usize = 5;
const SCAN_ROUNDS: usize = 32;

struct TrackingAllocator;

static TRACK_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static CURRENT_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() && TRACK_ALLOCATIONS.load(Ordering::Relaxed) {
            CURRENT_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if TRACK_ALLOCATIONS.load(Ordering::Relaxed) {
            CURRENT_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        System.dealloc(ptr, layout);
    }

    unsafe fn realloc(&self, ptr: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, old, new_size);
        if !new_ptr.is_null() && TRACK_ALLOCATIONS.load(Ordering::Relaxed) {
            if new_size >= old.size() {
                CURRENT_BYTES.fetch_add(new_size - old.size(), Ordering::Relaxed);
            } else {
                CURRENT_BYTES.fetch_sub(old.size() - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

fn manifest_package_version(manifest: &str) -> &str {
    let mut in_package = false;
    for line in manifest.lines() {
        match line.trim() {
            "[package]" => in_package = true,
            section if section.starts_with('[') => in_package = false,
            line if in_package && line.starts_with("version = ") => {
                return line.trim_start_matches("version = ").trim_matches('"');
            }
            _ => {}
        }
    }
    panic!("package version missing from cedarwood Cargo.toml");
}

fn locked_version(package: &str) -> &str {
    let mut current_name = None;
    for line in COMPARISON_LOCK.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            current_name = None;
        } else if let Some(name) = line.strip_prefix("name = \"").and_then(|value| value.strip_suffix('"')) {
            current_name = Some(name);
        } else if current_name == Some(package) {
            if let Some(version) = line
                .strip_prefix("version = \"")
                .and_then(|value| value.strip_suffix('"'))
            {
                return version;
            }
        }
    }
    panic!("{package} is missing from comparison Cargo.lock");
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn comparison_layout(manifest: &str) -> Result<&'static str, String> {
    let dependency = manifest
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("cedarwood ="))
        .ok_or_else(|| "comparison Cargo.toml is missing its cedarwood dependency".to_owned())?;
    for required in ["path = \"../..\"", "default-features = false", "features = [\"std\"]"] {
        if !dependency.contains(required) {
            return Err(format!(
                "comparison cedarwood dependency must explicitly contain `{required}` to measure the std, non-reduced layout; found `{dependency}`"
            ));
        }
    }
    if dependency.contains("reduced-trie") {
        return Err(format!(
            "comparison cedarwood dependency unexpectedly enables reduced-trie: `{dependency}`"
        ));
    }
    Ok("default")
}

struct CurrentProvenance {
    cedarwood_version: &'static str,
    cedarwood_layout: &'static str,
    cedarwood_manifest_sha256: String,
    cedarwood_source_sha256: String,
    harness_source_sha256: String,
    shared_support_sha256: String,
    comparison_manifest_sha256: String,
    comparison_lock_sha256: String,
    dataset_sha256: String,
}

impl CurrentProvenance {
    fn collect() -> Self {
        Self {
            cedarwood_version: manifest_package_version(CEDARWOOD_MANIFEST),
            cedarwood_layout: comparison_layout(COMPARISON_MANIFEST)
                .unwrap_or_else(|error| panic!("invalid comparison layout contract: {error}")),
            cedarwood_manifest_sha256: sha256(CEDARWOOD_MANIFEST.as_bytes()),
            cedarwood_source_sha256: sha256(CEDARWOOD_SOURCE),
            harness_source_sha256: sha256(HARNESS_SOURCE),
            shared_support_sha256: sha256(SUPPORT_SOURCE),
            comparison_manifest_sha256: sha256(COMPARISON_MANIFEST.as_bytes()),
            comparison_lock_sha256: sha256(COMPARISON_LOCK.as_bytes()),
            dataset_sha256: sha256(support::DICTIONARY_SOURCE.as_bytes()),
        }
    }

    fn fields(&self) -> [(&'static str, &str); 10] {
        [
            ("cedarwood_version", self.cedarwood_version),
            ("cedarwood_layout", self.cedarwood_layout),
            ("cedarwood_manifest_sha256", &self.cedarwood_manifest_sha256),
            ("cedarwood_source_sha256", &self.cedarwood_source_sha256),
            ("harness_source_sha256", &self.harness_source_sha256),
            ("shared_support_sha256", &self.shared_support_sha256),
            ("comparison_manifest_sha256", &self.comparison_manifest_sha256),
            ("comparison_lock_sha256", &self.comparison_lock_sha256),
            ("dataset", DATASET_PATH),
            ("dataset_sha256", &self.dataset_sha256),
        ]
    }
}

fn raw_field<'a>(raw: &'a str, name: &str) -> Result<&'a str, String> {
    let prefix = format!("{name}=");
    let mut matches = raw.lines().filter_map(|line| line.strip_prefix(&prefix));
    let value = matches
        .next()
        .ok_or_else(|| format!("raw comparison output is missing required provenance field `{name}`"))?;
    if matches.next().is_some() {
        return Err(format!(
            "raw comparison output contains duplicate provenance field `{name}`"
        ));
    }
    Ok(value)
}

fn verify_current_provenance(raw: &str) -> Result<(), String> {
    let current = CurrentProvenance::collect();
    for (name, expected) in current.fields() {
        let recorded = raw_field(raw, name)?;
        if recorded != expected {
            return Err(format!(
                "stale comparison provenance `{name}`: raw output records `{recorded}`, current benchmark input is `{expected}`; rerun `(cd benches/comparison && cargo run --release --locked)` and update the current 0.6 raw output and README table"
            ));
        }
    }

    let expected_row_name = format!("cedarwood {}", current.cedarwood_version);
    let mut cedarwood_rows = raw
        .lines()
        .skip_while(|line| !line.starts_with("implementation,mutable,build_ms,"))
        .skip(1)
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split(',').next())
        .filter(|name| name.starts_with("cedarwood "));
    let row_name = cedarwood_rows
        .next()
        .ok_or_else(|| "raw comparison output is missing the cedarwood implementation row".to_owned())?;
    if cedarwood_rows.next().is_some() {
        return Err("raw comparison output contains duplicate cedarwood implementation rows".to_owned());
    }
    if row_name != expected_row_name {
        return Err(format!(
            "cedarwood implementation row `{row_name}` does not match provenance version `{}`; rerun the current comparison",
            current.cedarwood_version
        ));
    }

    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum Implementation {
    Cedarwood,
    Fst,
    Daachorse,
    Crawdad,
    Yada,
    HashMap,
}

impl Implementation {
    const ALL: [Self; 6] = [
        Self::Cedarwood,
        Self::Fst,
        Self::Daachorse,
        Self::Crawdad,
        Self::Yada,
        Self::HashMap,
    ];

    fn name(self) -> String {
        match self {
            Self::Cedarwood => format!("cedarwood {}", manifest_package_version(CEDARWOOD_MANIFEST)),
            Self::Fst => format!("fst {}", locked_version("fst")),
            Self::Daachorse => format!("daachorse {}", locked_version("daachorse")),
            Self::Crawdad => format!("crawdad {}", locked_version("crawdad")),
            Self::Yada => format!("yada {}", locked_version("yada")),
            Self::HashMap => "std HashMap".to_owned(),
        }
    }

    const fn mutable(self) -> bool {
        matches!(self, Self::Cedarwood | Self::HashMap)
    }

    const fn exact_method(self) -> &'static str {
        match self {
            Self::Daachorse => "anchored full-span Aho-Corasick match",
            _ => "native exact lookup",
        }
    }

    const fn scan_method(self) -> Option<&'static str> {
        match self {
            Self::Cedarwood | Self::Crawdad | Self::Yada => {
                Some("common-prefix search at each UTF-8 character boundary")
            }
            Self::Daachorse => Some("native overlapping Aho-Corasick scan"),
            Self::Fst | Self::HashMap => None,
        }
    }
}

struct Dataset {
    keys: Vec<&'static str>,
    cedar_entries: Vec<(&'static str, i32)>,
    yada_entries: Vec<(&'static [u8], u32)>,
    hit_keys: Vec<&'static str>,
    hit_values: Vec<u32>,
    miss_keys: Vec<String>,
    scan_text: String,
    scan_offsets: Vec<usize>,
}

impl Dataset {
    fn load() -> Self {
        let keys = support::load_dictionary();
        assert!(keys.iter().all(|key| !key.is_empty() && !key.as_bytes().contains(&0)));
        assert!(keys.len() < i32::MAX as usize);

        let cedar_entries = support::entries(&keys);
        let yada_entries = keys
            .iter()
            .enumerate()
            .map(|(value, key)| (key.as_bytes(), u32::try_from(value).unwrap()))
            .collect();
        let hit_keys = support::evenly_sampled_keys(&keys, support::LOOKUP_SAMPLE_SIZE);
        let hit_values = hit_keys
            .iter()
            .map(|key| u32::try_from(keys.binary_search(key).unwrap()).unwrap())
            .collect();
        let miss_keys = support::exact_misses(&hit_keys);
        let scan_text = support::tokenizer_text(&hit_keys, support::SCAN_TEXT_MIN_BYTES);
        let scan_offsets = scan_text.char_indices().map(|(offset, _)| offset).collect();

        Self {
            keys,
            cedar_entries,
            yada_entries,
            hit_keys,
            hit_values,
            miss_keys,
            scan_text,
            scan_offsets,
        }
    }
}

enum Model {
    Cedarwood(Cedar),
    Fst(FstMap<Vec<u8>>),
    Daachorse(DoubleArrayAhoCorasick<u32>),
    Crawdad(Crawdad),
    Yada(Yada<Vec<u8>>),
    HashMap(HashMap<String, u32>),
}

fn build(kind: Implementation, data: &Dataset) -> Model {
    match kind {
        Implementation::Cedarwood => {
            let mut cedar = Cedar::new();
            cedar
                .build(&data.cedar_entries)
                .expect("validated dictionary must build a cedar trie");
            Model::Cedarwood(cedar)
        }
        Implementation::Fst => Model::Fst(
            FstMap::from_iter(data.keys.iter().enumerate().map(|(value, key)| (*key, value as u64)))
                .expect("sorted unique keys must build an fst map"),
        ),
        Implementation::Daachorse => Model::Daachorse(
            DoubleArrayAhoCorasick::<u32>::new(&data.keys).expect("unique keys must build a daachorse automaton"),
        ),
        Implementation::Crawdad => Model::Crawdad(
            Crawdad::from_records(data.keys.iter().enumerate().map(|(value, key)| (*key, value as u32)))
                .expect("sorted unique keys must build a crawdad trie"),
        ),
        Implementation::Yada => Model::Yada(
            Yada::new(
                DoubleArrayBuilder::build(&data.yada_entries)
                    .expect("sorted unique keys must build a yada double array"),
            )
            .expect("builder output must validate"),
        ),
        Implementation::HashMap => Model::HashMap(
            data.keys
                .iter()
                .enumerate()
                .map(|(value, key)| ((*key).to_owned(), value as u32))
                .collect(),
        ),
    }
}

fn exact_lookup(model: &Model, key: &str) -> Option<u32> {
    match model {
        Model::Cedarwood(model) => model.exact_match_search(key).map(|item| item.0 as u32),
        Model::Fst(model) => model.get(key).map(|value| value as u32),
        Model::Daachorse(model) => model
            .find_overlapping_iter(key)
            .find(|item| item.start() == 0 && item.end() == key.len())
            .map(|item| item.value()),
        Model::Crawdad(model) => model.exact_match(key.chars()),
        Model::Yada(model) => model.exact_match_search(key),
        Model::HashMap(model) => model.get(key).copied(),
    }
}

fn exact_hits(model: &Model, data: &Dataset) -> u64 {
    data.hit_keys
        .iter()
        .map(|key| u64::from(exact_lookup(model, black_box(key)).expect("verified hit")))
        .fold(0_u64, u64::wrapping_add)
}

fn exact_misses(model: &Model, data: &Dataset) -> usize {
    data.miss_keys
        .iter()
        .filter(|key| exact_lookup(model, black_box(key)).is_none())
        .count()
}

fn scan(model: &Model, data: &Dataset) -> Option<(usize, u64)> {
    match model {
        Model::Cedarwood(model) => Some(data.scan_offsets.iter().fold((0, 0_u64), |(count, sum), offset| {
            model
                .common_prefix_iter(black_box(&data.scan_text[*offset..]))
                .fold((count, sum), |(count, sum), (value, _)| {
                    (count + 1, sum.wrapping_add(value as u64))
                })
        })),
        Model::Daachorse(model) => Some(
            model
                .find_overlapping_iter(black_box(&data.scan_text))
                .fold((0, 0_u64), |(count, sum), item| {
                    (count + 1, sum.wrapping_add(u64::from(item.value())))
                }),
        ),
        Model::Crawdad(model) => Some(data.scan_offsets.iter().fold((0, 0_u64), |(count, sum), offset| {
            model
                .common_prefix_search(black_box(data.scan_text[*offset..].chars()))
                .fold((count, sum), |(count, sum), (value, _)| {
                    (count + 1, sum.wrapping_add(u64::from(value)))
                })
        })),
        Model::Yada(model) => Some(data.scan_offsets.iter().fold((0, 0_u64), |(count, sum), offset| {
            model
                .common_prefix_search(black_box(&data.scan_text.as_bytes()[*offset..]))
                .fold((count, sum), |(count, sum), (value, _)| {
                    (count + 1, sum.wrapping_add(u64::from(value)))
                })
        })),
        Model::Fst(_) | Model::HashMap(_) => None,
    }
}

fn verify(model: &Model, data: &Dataset) -> Option<(usize, u64)> {
    for (key, expected) in data.hit_keys.iter().zip(&data.hit_values) {
        assert_eq!(exact_lookup(model, key), Some(*expected));
    }
    assert!(data.miss_keys.iter().all(|key| exact_lookup(model, key).is_none()));
    scan(model, data)
}

fn median(mut durations: Vec<Duration>) -> Duration {
    durations.sort_unstable();
    durations[durations.len() / 2]
}

fn build_duration(kind: Implementation, data: &Dataset) -> Duration {
    let mut samples = Vec::with_capacity(BUILD_SAMPLES);
    for _ in 0..BUILD_SAMPLES {
        let start = Instant::now();
        let model = build(kind, data);
        samples.push(start.elapsed());
        black_box(&model);
        drop(model);
    }
    median(samples)
}

fn query_duration<F, T>(samples: usize, rounds: usize, mut operation: F) -> Duration
where
    F: FnMut() -> T,
{
    let mut durations = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        for _ in 0..rounds {
            black_box(operation());
        }
        durations.push(start.elapsed());
    }
    median(durations)
}

fn owned_heap_bytes(kind: Implementation, data: &Dataset) -> usize {
    assert!(!TRACK_ALLOCATIONS.swap(true, Ordering::SeqCst));
    CURRENT_BYTES.store(0, Ordering::SeqCst);
    let model = build(kind, data);
    let bytes = CURRENT_BYTES.load(Ordering::SeqCst);
    if let Model::Cedarwood(cedar) = &model {
        assert_eq!(
            bytes,
            cedar.allocated_bytes(),
            "public cedar allocation metric must match the harness definition"
        );
    }
    black_box(&model);
    drop(model);
    let remaining = CURRENT_BYTES.load(Ordering::SeqCst);
    TRACK_ALLOCATIONS.store(false, Ordering::SeqCst);
    assert_eq!(remaining, 0, "tracked model allocations must be released");
    bytes
}

fn command_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| "unavailable".to_owned())
}

fn worktree_state() -> &'static str {
    match Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
    {
        Ok(output) if output.status.success() && output.stdout.is_empty() => "clean",
        Ok(output) if output.status.success() => "dirty",
        _ => "unavailable",
    }
}

fn cpu_name() -> String {
    if cfg!(target_os = "macos") {
        command_output("sysctl", &["-n", "machdep.cpu.brand_string"])
    } else {
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|cpuinfo| {
                cpuinfo
                    .lines()
                    .find_map(|line| line.strip_prefix("model name\t:").map(|value| value.trim().to_owned()))
            })
            .unwrap_or_else(|| "unavailable".to_owned())
    }
}

fn os_name() -> String {
    if cfg!(target_os = "macos") {
        format!(
            "{} {}; build {}; arch {}",
            command_output("sw_vers", &["-productName"]),
            command_output("sw_vers", &["-productVersion"]),
            command_output("sw_vers", &["-buildVersion"]),
            command_output("uname", &["-m"]),
        )
    } else {
        command_output("uname", &["-s", "-r", "-m"])
    }
}

fn cpp_cedar_status() -> &'static str {
    const STANDARD_HEADERS: [&str; 3] = [
        "/usr/local/include/cedarpp.h",
        "/opt/homebrew/include/cedarpp.h",
        "/usr/include/cedarpp.h",
    ];
    if STANDARD_HEADERS.iter().any(|path| std::path::Path::new(path).is_file()) {
        "not measured (shared-schema C++ adapter is not implemented; legacy cedarpp.h is available)"
    } else {
        "not measured (shared-schema C++ adapter is not implemented and cedarpp.h is unavailable)"
    }
}

fn verify_readme(raw_path: &OsStr, readme_path: &OsStr) {
    let raw = std::fs::read_to_string(raw_path).expect("read raw comparison output");
    let readme = std::fs::read_to_string(readme_path).expect("read root README");
    if let Err(error) = verify_current_provenance(&raw) {
        panic!("cannot verify README from {}: {error}", raw_path.to_string_lossy());
    }
    let mut rows = Vec::new();
    let mut in_results = false;

    for line in raw.lines() {
        if line.starts_with("implementation,mutable,build_ms,") {
            in_results = true;
            continue;
        }
        if in_results && line.is_empty() {
            break;
        }
        if !in_results {
            continue;
        }

        let fields: Vec<_> = line.split(',').collect();
        assert_eq!(fields.len(), 10, "unexpected raw result row: {line}");
        let implementation = if fields[0] == "std HashMap" {
            "`std::collections::HashMap`"
        } else {
            fields[0]
        };
        let mutable = match fields[1] {
            "true" => "yes",
            "false" => "no",
            value => panic!("unexpected mutable value: {value}"),
        };
        rows.push(format!(
            "| {implementation} | {mutable} | {} | {} | {} | {} | {} |",
            fields[2], fields[3], fields[4], fields[5], fields[6]
        ));
    }

    assert_eq!(
        rows.len(),
        Implementation::ALL.len(),
        "raw output must contain every Rust implementation"
    );
    rows.push(
        "| C++ cedar | yes | not measured | not measured | not measured | not measured | not measured |".to_owned(),
    );
    let expected = format!(
        "| Implementation | Mutable | Build (ms) | Exact hit (M/s) | Exact miss (M/s) | Prefix scan (MiB/s) | Owned heap (MiB) |\n|---|:---:|---:|---:|---:|---:|---:|\n{}",
        rows.join("\n")
    );
    assert!(
        readme.contains(&expected),
        "README comparison table differs from {}; regenerate the table from the raw CSV rows",
        raw_path.to_string_lossy()
    );
    println!("README comparison table matches {}", raw_path.to_string_lossy());
}

fn mib(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn rate(count: usize, rounds: usize, duration: Duration) -> f64 {
    count as f64 * rounds as f64 / duration.as_secs_f64() / 1_000_000.0
}

fn main() {
    let args: Vec<_> = env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == "--verify-readme") {
        if args.len() != 4 {
            eprintln!("usage: cargo run --locked -- --verify-readme RAW_OUTPUT ROOT_README");
            std::process::exit(2);
        }
        verify_readme(&args[2], &args[3]);
        return;
    }
    if args.len() != 1 {
        eprintln!("usage: cargo run --release --locked");
        std::process::exit(2);
    }

    let data = Dataset::load();
    let provenance = CurrentProvenance::collect();
    let expected_hit_sum = data
        .hit_values
        .iter()
        .map(|value| u64::from(*value))
        .fold(0_u64, u64::wrapping_add);

    println!("cedarwood comparison raw output");
    println!("date_utc={}", command_output("date", &["-u", "+%Y-%m-%d"]));
    println!("os={}", os_name());
    println!("cpu={}", cpu_name());
    println!("rustc={}", command_output("rustc", &["-Vv"]).replace('\n', "; "));
    println!("cargo={}", command_output("cargo", &["-V"]));
    println!("profile=release opt-level=3 lto=thin codegen-units=1 debug=false incremental=false");
    println!("cedarwood_revision={}", command_output("git", &["rev-parse", "HEAD"]));
    println!("cedarwood_worktree={}", worktree_state());
    for (name, value) in provenance.fields() {
        println!("{name}={value}");
    }
    println!("unique_keys={}", data.keys.len());
    println!("hit_sample={}", data.hit_keys.len());
    println!("miss_sample={}", data.miss_keys.len());
    println!("scan_bytes={}", data.scan_text.len());
    println!("scan_character_boundaries={}", data.scan_offsets.len());
    println!("build_statistic=median of {BUILD_SAMPLES} fresh constructions");
    println!("exact_statistic=median of {EXACT_SAMPLES} samples x {EXACT_ROUNDS} rounds");
    println!("scan_statistic=median of {SCAN_SAMPLES} samples x {SCAN_ROUNDS} rounds");
    println!("memory_method=net live heap bytes requested from System by the constructed data structure; corpus and benchmark inputs excluded");
    println!("cpp_cedar={}", cpp_cedar_status());
    println!();
    println!("implementation,mutable,build_ms,exact_hit_mops,exact_miss_mops,prefix_scan_mib_s,owned_heap_mib,exact_method,scan_method,verification");

    let mut expected_scan = None;
    for kind in Implementation::ALL {
        let model = build(kind, &data);
        let scan_result = verify(&model, &data);
        assert_eq!(exact_hits(&model, &data), expected_hit_sum);
        assert_eq!(exact_misses(&model, &data), data.miss_keys.len());
        if let Some(result) = scan_result {
            if let Some(expected) = expected_scan {
                assert_eq!(result, expected, "prefix scan mismatch for {}", kind.name());
            } else {
                expected_scan = Some(result);
            }
        }

        let hit_duration = query_duration(EXACT_SAMPLES, EXACT_ROUNDS, || exact_hits(&model, &data));
        let miss_duration = query_duration(EXACT_SAMPLES, EXACT_ROUNDS, || exact_misses(&model, &data));
        let scan_duration = kind
            .scan_method()
            .map(|_| query_duration(SCAN_SAMPLES, SCAN_ROUNDS, || scan(&model, &data).unwrap()));
        drop(model);

        let build_time = build_duration(kind, &data);
        let heap_bytes = owned_heap_bytes(kind, &data);
        let scan_rate = scan_duration
            .map(|duration| {
                data.scan_text.len() as f64 * SCAN_ROUNDS as f64 / duration.as_secs_f64() / (1024.0 * 1024.0)
            })
            .map(|value| format!("{value:.2}"))
            .unwrap_or_else(|| "unsupported".to_owned());

        println!(
            "{},{},{:.3},{:.3},{:.3},{},{:.3},{},{},passed",
            kind.name(),
            kind.mutable(),
            build_time.as_secs_f64() * 1_000.0,
            rate(data.hit_keys.len(), EXACT_ROUNDS, hit_duration),
            rate(data.miss_keys.len(), EXACT_ROUNDS, miss_duration),
            scan_rate,
            mib(heap_bytes),
            kind.exact_method(),
            kind.scan_method().unwrap_or("unsupported"),
        );
    }

    let (scan_count, scan_sum) = expected_scan.expect("at least one scan implementation");
    println!();
    println!("verification_exact_hit_checksum={expected_hit_sum}");
    println!("verification_exact_misses={}", data.miss_keys.len());
    println!("verification_scan_matches={scan_count}");
    println!("verification_scan_value_checksum={scan_sum}");
    println!("allocator_word_size={}", mem::size_of::<usize>() * 8);
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURRENT_RAW: &str = include_str!("../results/2026-07-11-apple-m4-pro-cedarwood-0.6.0.txt");

    #[test]
    fn current_raw_provenance_matches_benchmark_inputs() {
        verify_current_provenance(CURRENT_RAW).unwrap();
    }

    #[test]
    fn comparison_manifest_selects_std_non_reduced_layout() {
        assert_eq!(comparison_layout(COMPARISON_MANIFEST).unwrap(), "default");
    }

    #[test]
    fn stale_provenance_is_rejected() {
        let current = CurrentProvenance::collect();
        let recorded = format!("harness_source_sha256={}", current.harness_source_sha256);
        let stale = CURRENT_RAW.replacen(&recorded, "harness_source_sha256=stale", 1);
        assert_ne!(stale, CURRENT_RAW, "fixture must contain the current harness hash");

        let error = verify_current_provenance(&stale).unwrap_err();
        assert!(error.contains("stale comparison provenance `harness_source_sha256`"));
        assert!(error.contains("rerun"));
    }

    #[test]
    fn stale_root_manifest_provenance_is_rejected() {
        let current = CurrentProvenance::collect();
        let recorded = format!("cedarwood_manifest_sha256={}", current.cedarwood_manifest_sha256);
        let stale = CURRENT_RAW.replacen(&recorded, "cedarwood_manifest_sha256=stale", 1);
        assert_ne!(stale, CURRENT_RAW, "fixture must contain the current manifest hash");

        let error = verify_current_provenance(&stale).unwrap_err();
        assert!(error.contains("stale comparison provenance `cedarwood_manifest_sha256`"));
        assert!(error.contains("rerun"));
    }

    #[test]
    fn stale_comparison_manifest_provenance_is_rejected() {
        let current = CurrentProvenance::collect();
        let recorded = format!("comparison_manifest_sha256={}", current.comparison_manifest_sha256);
        let stale = CURRENT_RAW.replacen(&recorded, "comparison_manifest_sha256=stale", 1);
        assert_ne!(stale, CURRENT_RAW, "fixture must contain the current manifest hash");

        let error = verify_current_provenance(&stale).unwrap_err();
        assert!(error.contains("stale comparison provenance `comparison_manifest_sha256`"));
        assert!(error.contains("rerun"));
    }
}
