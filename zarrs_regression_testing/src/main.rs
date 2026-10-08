//! Data compatibility testing between the current `zarrs` and previous releases.
//!
//! Arrays are written and read with the current `zarrs` and with previous releases (via helper binaries), for every combination of codec and data type.
//! Codec parameters, shapes and data are sampled with proptest.
//!
//! By default, only the latest release is tested and any regression results in a non-zero exit code.
//! With `--all`, every release is tested and a summary of how far back compatibility extends is printed.
//! With `--html <PATH>`, an HTML report with an overview of all combinations and the details of every failure is written.

mod cases;
mod data;
mod helper;
mod releases;
mod report;
mod run;
mod summary;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use crate::releases::RELEASES;

/// Test data compatibility between the current zarrs and previous releases.
#[derive(Parser)]
struct Args {
    /// Test all previous releases and summarise how far back compatibility extends.
    ///
    /// By default, only the latest release is tested.
    #[arg(long)]
    all: bool,
    /// The number of samples of each codec/data type combination.
    #[arg(long, default_value_t = 2)]
    samples: usize,
    /// The seed for sampling cases (random by default).
    #[arg(long)]
    seed: Option<u64>,
    /// Only test combinations whose codec or data type contains this string.
    #[arg(long)]
    filter: Option<String>,
    /// Print every failure rather than one per combination.
    #[arg(long, short)]
    verbose: bool,
    /// Write an HTML report with an overview of all combinations and the details of every failure.
    #[arg(long, value_name = "PATH")]
    html: Option<PathBuf>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Returns `true` if there are no failures with the latest release.
fn run(args: &Args) -> Result<bool, String> {
    let seed = args.seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| {
                duration.as_secs() ^ u64::from(duration.subsec_nanos())
            })
    });
    let latest = RELEASES[0];
    let releases = if args.all { RELEASES } else { &RELEASES[..1] };

    let data_types = cases::data_types();
    let combinations: Vec<_> = cases::combinations(&cases::codec_kinds(), &data_types)
        .into_iter()
        .filter(|combination| {
            args.filter.as_ref().is_none_or(|filter| {
                combination.codec.to_string().contains(filter)
                    || data_types[combination.data_type].label.contains(filter)
            })
        })
        .collect();
    let cases = cases::sample_cases(&combinations, &data_types, args.samples, seed)?;
    let work_dir = helper::root_dir()
        .join("work")
        .join(if args.all { "all" } else { "latest" });
    let results = run::run(&cases, &combinations, &data_types, releases, &work_dir)?;

    let run = summary::Run {
        data_types: &data_types,
        combinations: &combinations,
        cases: &cases,
        results: &results,
        releases,
        work_dir: &work_dir,
    };
    let (failures, known_issues) = run.failures();
    let latest_failures: Vec<_> = failures
        .iter()
        .filter(|failure| failure.release.is_none_or(|release| release == latest))
        .collect();

    println!(
        "zarrs regression testing: current zarrs vs {}",
        if args.all {
            format!("releases {}–{latest}", releases[releases.len() - 1])
        } else {
            format!("latest release {latest}")
        }
    );
    println!(
        "{} combinations × {} samples = {} cases (--seed {seed})\n",
        combinations.len(),
        args.samples,
        cases.len()
    );
    if args.all {
        print!("{}", run.compatibility());
        let older_failures: Vec<_> = failures
            .iter()
            .filter(|failure| failure.release.is_some_and(|release| release != latest))
            .collect();
        if !older_failures.is_empty() {
            println!(
                "\nIncompatibilities with older releases that support the combination themselves:"
            );
            if args.verbose {
                print!("{}", run.format_failures(&older_failures, true));
            } else {
                print!("{}", run.format_failures_concise(&older_failures));
            }
        }
        println!();
    }

    if !known_issues.is_empty() {
        let known_issues: Vec<_> = known_issues.iter().collect();
        println!("Known issues (current and {latest} cannot read back data they wrote):");
        if args.verbose {
            print!("{}", run.format_failures(&known_issues, true));
        } else {
            print!("{}", run.format_failures_concise(&known_issues));
        }
        println!();
    }

    println!("{}", run.counts());
    if latest_failures.is_empty() {
        println!("✓ no regressions: current zarrs and zarrs {latest} read each other's data");
    } else {
        println!("Regressions with the latest release ({latest}):");
        print!("{}", run.format_failures(&latest_failures, args.verbose));
        println!("\n✗ {} failing cases", latest_failures.len());
    }

    if let Some(path) = &args.html {
        let meta = report::Meta {
            seed,
            samples: args.samples,
            all: args.all,
            filter: args.filter.as_deref(),
        };
        let html = report::html(&run, &failures, &known_issues, &meta);
        std::fs::write(path, html).map_err(|err| format!("write {}: {err}", path.display()))?;
        println!("report written to {}", path.display());
    }
    Ok(latest_failures.is_empty())
}
