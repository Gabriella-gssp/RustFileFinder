use clap::{Parser, Subcommand};
use gag::Gag;
use once_cell::sync::Lazy;
use pdf_extract::extract_text;
use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use walkdir::WalkDir;

static PDF_EXTRACT_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static SILENCE_PANICS: Lazy<std::sync::atomic::AtomicBool> =
    Lazy::new(|| std::sync::atomic::AtomicBool::new(false));

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "RustFileFinder: search files by name and/or content recursively"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Search(SearchArgs),

    ConfigInit {
        #[arg(long)]
        path: Option<PathBuf>,
    },

    Presets {
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[derive(Parser, Debug, Clone)]
struct SearchArgs {
    #[arg(long)]
    config: Option<PathBuf>,

    #[arg(long)]
    preset: Option<String>,

    #[arg(long, default_value = ".")]
    dir: PathBuf,

    #[arg(long, default_value_t = false)]
    include_pdf: bool,

    #[arg(long)]
    name: Option<String>,

    #[arg(long)]
    content: Option<String>,

    #[arg(long, default_value = "md")]
    format: String,

    #[arg(long, default_value_t = 2_000_000)]
    max_bytes: u64,

    #[arg(long)]
    ext: Option<String>,

    #[arg(long)]
    limit: Option<usize>,

    #[arg(long, default_value_t = false)]
    verbose: bool,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct AppConfig {
    defaults: Option<SearchConfig>,
    presets: Option<std::collections::HashMap<String, SearchConfig>>,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct SearchConfig {
    dir: Option<PathBuf>,
    include_pdf: Option<bool>,
    name: Option<String>,
    content: Option<String>,
    format: Option<String>,
    max_bytes: Option<u64>,
    ext: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize, Debug, Clone)]
struct MatchResult {
    path: String,
    matched_name: bool,
    matched_content: bool,
    snippet: Option<String>,
}

struct Counters<'a> {
    scanned_text: &'a AtomicUsize,
    scanned_pdf: &'a AtomicUsize,
    skipped_non_text: &'a AtomicUsize,
    skipped_too_large: &'a AtomicUsize,
    skipped_non_utf8: &'a AtomicUsize,
    skipped_unreadable_text: &'a AtomicUsize,
    skipped_unreadable_pdf: &'a AtomicUsize,
}

#[derive(Serialize, Debug, Clone)]
struct RunStats {
    files_discovered: usize,

    files_scanned_text: usize,
    files_scanned_pdf: usize,

    files_skipped_non_text: usize,
    files_skipped_too_large: usize,
    files_skipped_non_utf8: usize,

    files_skipped_unreadable_text: usize,
    files_skipped_unreadable_pdf: usize,

    matches_total: usize,
    matches_printed: usize,
    elapsed_ms: u128,
}

fn main() {

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !SILENCE_PANICS.load(Ordering::Relaxed) {
            default_hook(info);
        }
    }));

    let cli = Cli::parse();

    match cli.command {
        Commands::ConfigInit { path } => {
            let p = path.unwrap_or_else(|| PathBuf::from("rustfilefinder.toml"));
            write_sample_config(&p).expect("failed to write config");
            eprintln!("Config created at: {}", p.display());
        }

        Commands::Presets { config } => {
            let cfg_path = resolve_config_path(&config);
            let cfg = cfg_path.as_deref().and_then(load_config);
            if let Some(cfg) = cfg {
                if let Some(presets) = cfg.presets {
                    for k in presets.keys() {
                        println!("{k}");
                    }
                }
            }
        }

        Commands::Search(mut args) => {
            let started = Instant::now();

            let cfg_path = resolve_config_path(&args.config);
            let cfg = cfg_path.as_deref().and_then(load_config);
            args = merge_search_args(args, cfg);

            if args.name.is_none() && args.content.is_none() {
                eprintln!("Error: you must provide at least --name or --content");
                std::process::exit(2);
            }

            run_search(args, started);
        }
    }
}

fn run_search(args: SearchArgs, started: Instant) {
    let content_re: Option<Regex> = args.content.as_ref().map(|pat| {
        Regex::new(pat).unwrap_or_else(|e| {
            eprintln!("Invalid regex for --content: {e}");
            std::process::exit(2);
        })
    });

    let allowed_ext: Option<Vec<String>> = args.ext.as_ref().map(|s| {
        s.split(',')
            .map(|x| x.trim().to_lowercase())
            .filter(|x| !x.is_empty())
            .collect()
    });

    let files: Vec<PathBuf> = WalkDir::new(&args.dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_ignored_dir(e.path()))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| ext_allowed(p, allowed_ext.as_ref()))
        .collect();

    let files_discovered = files.len();

    let scanned_text = AtomicUsize::new(0);
    let scanned_pdf = AtomicUsize::new(0);

    let skipped_non_text = AtomicUsize::new(0);
    let skipped_too_large = AtomicUsize::new(0);
    let skipped_non_utf8 = AtomicUsize::new(0);

    let skipped_unreadable_text = AtomicUsize::new(0);
    let skipped_unreadable_pdf = AtomicUsize::new(0);

    let counters = Counters {
        scanned_text: &scanned_text,
        scanned_pdf: &scanned_pdf,
        skipped_non_text: &skipped_non_text,
        skipped_too_large: &skipped_too_large,
        skipped_non_utf8: &skipped_non_utf8,
        skipped_unreadable_text: &skipped_unreadable_text,
        skipped_unreadable_pdf: &skipped_unreadable_pdf,
    };

    let mut results: Vec<MatchResult> = files
        .par_iter()
        .filter_map(|path| {
            let attempt = catch_unwind(AssertUnwindSafe(|| {
                analyze_file(
                    path,
                    args.name.as_deref(),
                    content_re.as_ref(),
                    args.max_bytes,
                    allowed_ext.as_ref(),
                    args.include_pdf,
                    args.verbose,
                    &counters,
                )
            }));

            match attempt {
                Ok(v) => v,
                Err(_) => {
                    if is_pdf(path) {
                        counters
                            .skipped_unreadable_pdf
                            .fetch_add(1, Ordering::Relaxed);
                    } else {
                        counters
                            .skipped_unreadable_text
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    None
                }
            }
        })
        .collect();

    results.sort_by(|a, b| a.path.cmp(&b.path));

    let matches_total = results.len();

    let mut results_print = results;
    if let Some(limit) = args.limit {
        if results_print.len() > limit {
            results_print.truncate(limit);
        }
    }
    let matches_printed = results_print.len();

    let elapsed_ms = started.elapsed().as_millis();

    let stats = RunStats {
        files_discovered,
        files_scanned_text: scanned_text.load(Ordering::Relaxed),
        files_scanned_pdf: scanned_pdf.load(Ordering::Relaxed),
        files_skipped_non_text: skipped_non_text.load(Ordering::Relaxed),
        files_skipped_too_large: skipped_too_large.load(Ordering::Relaxed),
        files_skipped_non_utf8: skipped_non_utf8.load(Ordering::Relaxed),
        files_skipped_unreadable_text: skipped_unreadable_text.load(Ordering::Relaxed),
        files_skipped_unreadable_pdf: skipped_unreadable_pdf.load(Ordering::Relaxed),
        matches_total,
        matches_printed,
        elapsed_ms,
    };

    match args.format.as_str() {
        "json" => {
            #[derive(Serialize)]
            struct JsonOut<'a> {
                stats: RunStats,
                results: &'a [MatchResult],
            }
            let out = JsonOut {
                stats,
                results: &results_print,
            };
            let json = serde_json::to_string_pretty(&out).unwrap();
            println!("{json}");
        }
        _ => {
            print_markdown(&args, &stats, &results_print);
        }
    }
}

fn ext_allowed(path: &Path, allowed: Option<&Vec<String>>) -> bool {
    let Some(list) = allowed else { return true };
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    list.iter().any(|x| x == &ext.to_lowercase())
}

fn analyze_file(
    path: &Path,
    name_query: Option<&str>,
    content_re: Option<&Regex>,
    max_bytes: u64,
    allowed_ext: Option<&Vec<String>>,
    include_pdf: bool,
    verbose: bool,
    counters: &Counters,
) -> Option<MatchResult> {
    let file_name = path.file_name()?.to_string_lossy().to_string();

    let matched_name = name_query
        .map(|q| file_name.to_lowercase().contains(&q.to_lowercase()))
        .unwrap_or(false);

    let mut matched_content = false;
    let mut snippet: Option<String> = None;

    if let Some(re) = content_re {
        let pdf = is_pdf(path);

        if allowed_ext.is_none() {
            if pdf {
                if !include_pdf {
                    counters.skipped_non_text.fetch_add(1, Ordering::Relaxed);
                    return some_if_name_only(path, name_query, matched_name);
                }
            } else if !is_probably_text(path) {
                counters.skipped_non_text.fetch_add(1, Ordering::Relaxed);
                return some_if_name_only(path, name_query, matched_name);
            }
        } else {
            if pdf && !include_pdf {
                counters.skipped_non_text.fetch_add(1, Ordering::Relaxed);
                return some_if_name_only(path, name_query, matched_name);
            }
        }

        if pdf {
            counters.scanned_pdf.fetch_add(1, Ordering::Relaxed);

            let _lock = match PDF_EXTRACT_LOCK.lock() {
                Ok(g) => g,
                Err(_) => {
                    counters
                        .skipped_unreadable_pdf
                        .fetch_add(1, Ordering::Relaxed);
                    return some_if_name_only(path, name_query, matched_name);
                }
            };

            SILENCE_PANICS.store(true, Ordering::Relaxed);
            let pdf_text_result = catch_unwind(AssertUnwindSafe(|| {
                let _gag_out = Gag::stdout().ok();
                let _gag_err = gag::Gag::stderr().ok();
                extract_text(path)
            }));
            SILENCE_PANICS.store(false, Ordering::Relaxed);

            let pdf_text = match pdf_text_result {
                Ok(Ok(t)) => t,
                _ => {
                    counters
                        .skipped_unreadable_pdf
                        .fetch_add(1, Ordering::Relaxed);

                    if verbose {
                        eprintln!("[pdf] unreadable: {}", path.display());
                    }

                    return some_if_name_only(path, name_query, matched_name);
                }
            };

            if let Some(m) = re.find(&pdf_text) {
                matched_content = true;
                snippet = Some(snippet_around_match(&pdf_text, m.start(), m.end(), 40, 120));
            }
        } else {
            let meta = match fs::metadata(path) {
                Ok(v) => v,
                Err(_) => {
                    counters
                        .skipped_unreadable_text
                        .fetch_add(1, Ordering::Relaxed);
                    return some_if_name_only(path, name_query, matched_name);
                }
            };

            if meta.len() > max_bytes {
                counters.skipped_too_large.fetch_add(1, Ordering::Relaxed);
                return some_if_name_only(path, name_query, matched_name);
            }

            let f = match fs::File::open(path) {
                Ok(v) => v,
                Err(_) => {
                    counters
                        .skipped_unreadable_text
                        .fetch_add(1, Ordering::Relaxed);
                    return some_if_name_only(path, name_query, matched_name);
                }
            };

            counters.scanned_text.fetch_add(1, Ordering::Relaxed);

            let mut buf = Vec::new();
            if f.take(max_bytes).read_to_end(&mut buf).is_ok() {
                match std::str::from_utf8(&buf) {
                    Ok(text) => {
                        if let Some(m) = re.find(text) {
                            matched_content = true;
                            snippet = Some(snippet_around_match(text, m.start(), m.end(), 40, 120));
                        }
                    }
                    Err(_) => {
                        counters.skipped_non_utf8.fetch_add(1, Ordering::Relaxed);
                    }
                }
            } else {
                counters
                    .skipped_unreadable_text
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    let any = (name_query.is_some() && matched_name) || (content_re.is_some() && matched_content);

    if any {
        Some(MatchResult {
            path: path.to_string_lossy().to_string(),
            matched_name,
            matched_content,
            snippet,
        })
    } else {
        None
    }
}

fn some_if_name_only(
    path: &Path,
    name_query: Option<&str>,
    matched_name: bool,
) -> Option<MatchResult> {
    let any = name_query.is_some() && matched_name;

    if any {
        Some(MatchResult {
            path: path.to_string_lossy().to_string(),
            matched_name,
            matched_content: false,
            snippet: None,
        })
    } else {
        None
    }
}

fn print_markdown(args: &SearchArgs, stats: &RunStats, results: &[MatchResult]) {
    println!("# RustFileFinder results\n");
    println!("- Base dir: `{}`", args.dir.to_string_lossy());
    if let Some(n) = &args.name {
        println!("- Name query: `{}`", n);
    }
    if let Some(c) = &args.content {
        println!("- Content regex: `{}`", c);
    }
    if let Some(ext) = &args.ext {
        println!("- Extensions: `{}`", ext);
    } else if args.content.is_some() {
        println!("- Extensions: *(default text set for content search)*");
    }
    if args.include_pdf {
        println!("- PDF content search: `enabled`");
    } else {
        println!("- PDF content search: `disabled`");
    }

    println!();
    println!("## Run statistics");
    println!("- Files discovered: **{}**", stats.files_discovered);
    println!(
        "- Files scanned for content (text): **{}**",
        stats.files_scanned_text
    );
    println!(
        "- Files scanned for content (pdf): **{}**",
        stats.files_scanned_pdf
    );
    println!("- Skipped (non-text): **{}**", stats.files_skipped_non_text);
    println!(
        "- Skipped (too large): **{}**",
        stats.files_skipped_too_large
    );
    println!("- Skipped (non-UTF8): **{}**", stats.files_skipped_non_utf8);
    println!(
        "- Skipped (unreadable text): **{}**",
        stats.files_skipped_unreadable_text
    );
    println!(
        "- Skipped (unreadable pdf): **{}**",
        stats.files_skipped_unreadable_pdf
    );
    println!("- Matches total: **{}**", stats.matches_total);
    if let Some(l) = args.limit {
        println!(
            "- Matches printed: **{}** (limit = {})",
            stats.matches_printed, l
        );
    } else {
        println!("- Matches printed: **{}**", stats.matches_printed);
    }
    println!("- Elapsed: **{} ms**", stats.elapsed_ms);
    println!();

    println!("## Matches\n");
    for r in results {
        println!("### `{}`", r.path);
        println!("- matched_name: `{}`", r.matched_name);
        println!("- matched_content: `{}`", r.matched_content);
        if let Some(s) = &r.snippet {
            println!("- snippet: `{}`", s);
        }
        println!();
    }
}

fn is_ignored_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    matches!(name, ".git" | "target" | "node_modules")
}

fn is_probably_text(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_lowercase().as_str(),
        "txt"
            | "md"
            | "rs"
            | "js"
            | "css"
            | "html"
            | "htm"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
            | "py"
            | "java"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "ts"
            | "tsx"
    )
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

fn clamp_to_char_boundary(s: &str, mut i: usize) -> usize {
    if i > s.len() {
        i = s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn snippet_around_match(
    s: &str,
    m_start: usize,
    m_end: usize,
    context: usize,
    max_chars: usize,
) -> String {
    let start = m_start.saturating_sub(context);
    let end = (m_end + context).min(s.len());

    let start = clamp_to_char_boundary(s, start);
    let end = clamp_to_char_boundary(s, end);

    let mut out = s[start..end].replace('\n', " ");
    if out.chars().count() > max_chars {
        out = out.chars().take(max_chars).collect();
    }
    out
}

fn resolve_config_path(cli_path: &Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = cli_path {
        return Some(p.clone());
    }

    let local = PathBuf::from("rustfilefinder.toml");
    if local.exists() {
        return Some(local);
    }

    if let Some(dir) = dirs::config_dir() {
        let p = dir.join("rustfilefinder").join("config.toml");
        if p.exists() {
            return Some(p);
        }
    }

    None
}

fn load_config(path: &Path) -> Option<AppConfig> {
    let s = std::fs::read_to_string(path).ok()?;
    toml::from_str(&s).ok()
}

fn write_sample_config(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        eprintln!(
            "Config file already exists: {}\nNothing was changed.",
            path.display()
        );
        return Ok(());
    }

    let sample = r#"
[defaults]
format = "json"
max_bytes = 2000000

[presets.demo_text]
dir = "samples"
include_pdf = false
ext = "txt,md"
content = "(?i)compilatore|interprete|semantica|tipi|grammatica|parser|rust|python"
format = "json"

[presets.demo_pdf]
dir = "samples_pdf"
include_pdf = true
ext = "pdf"
content = "(?i)compilatore|interprete|semantica|tipi|grammatica|parser|rust|python"
format = "json"
"#;

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    std::fs::write(path, sample.trim_start())?;
    eprintln!("Config file created at: {}", path.display());
    Ok(())
}

fn merge_search_args(mut args: SearchArgs, cfg: Option<AppConfig>) -> SearchArgs {
    let Some(cfg) = cfg else { return args };

    if let Some(d) = cfg.defaults.clone() {
        apply_cfg(&mut args, &d);
    }

    if let Some(preset_name) = args.preset.clone() {
        if let Some(presets) = cfg.presets {
            if let Some(p) = presets.get(&preset_name) {
                apply_cfg(&mut args, p);
            }
        }
    }

    args
}

fn apply_cfg(args: &mut SearchArgs, c: &SearchConfig) {
    if args.dir == Path::new(".") {
        if let Some(v) = &c.dir {
            args.dir = v.clone();
        }
    }
    if !args.include_pdf {
        if let Some(v) = c.include_pdf {
            args.include_pdf = v;
        }
    }
    if args.name.is_none() {
        args.name = c.name.clone();
    }
    if args.content.is_none() {
        args.content = c.content.clone();
    }
    if args.format == "md" {
        if let Some(v) = &c.format {
            args.format = v.clone();
        }
    }
    if args.max_bytes == 2_000_000 {
        if let Some(v) = c.max_bytes {
            args.max_bytes = v;
        }
    }
    if args.ext.is_none() {
        args.ext = c.ext.clone();
    }
    if args.limit.is_none() {
        args.limit = c.limit;
    }
}
