//! Helper binaries that read and write arrays with previous `zarrs` releases.
//!
//! Each helper is a generated crate depending on a single `zarrs` release, built into a shared target directory.
//! Previous releases are built with the dependencies available when they were released (see [`Release::publish_time`]), which needs a nightly toolchain to generate the lockfile.
//! A helper processes a batch of requests (a JSON array on stdin) and responds with a JSON array on stdout.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;
use serde_json::Value;

use crate::data::Data;
use crate::releases::Release;

/// A helper request.
#[derive(Debug, Serialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub(crate) enum Request<'a> {
    Write {
        path: PathBuf,
        metadata: &'a Value,
        shape: &'a [u64],
        data: &'a Data,
    },
    Read {
        path: PathBuf,
        shape: &'a [u64],
    },
}

/// A helper response: the data read (if a read request) or an error.
pub(crate) type Response = Result<Option<Data>, String>;

/// The regression testing directory in the cargo target directory.
pub(crate) fn root_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../target"),
            PathBuf::from,
        )
        .join("zarrs_regression_testing")
}

fn name(release: Release) -> String {
    format!("zarrs-{}", release.to_string().replace('.', "-"))
}

/// Generate (if changed) and build the helper for `release`, returning the path to its binary.
///
/// # Errors
/// Returns an error if the helper project cannot be written or fails to build.
pub(crate) fn build(release: Release) -> Result<PathBuf, String> {
    let root = root_dir();
    let name = name(release);
    let publish_time = release.publish_time();
    let project = root.join("helpers").join(&name);
    let target = root.join("target");
    let features = release
        .features()
        .iter()
        .map(|feature| format!("\"{feature}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let manifest = include_str!("../helper/Cargo.toml.template")
        .replace("__NAME__", &name.replace('-', "_"))
        .replace("__BIN__", &name)
        .replace("__VERSION__", &release.to_string())
        .replace("__FEATURES__", &format!("[{features}]"))
        .replace("__PUBLISH_TIME__", publish_time.unwrap_or("none"));
    let main = include_str!("../helper/main.rs.template").replace("__ADAPTER__", release.adapter());
    let manifest_path = project.join("Cargo.toml");
    let lockfile = project.join("Cargo.lock");
    // The lockfile was resolved for the previous manifest
    if write_if_changed(&manifest_path, &manifest)?
        && let Err(err) = std::fs::remove_file(&lockfile)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        return Err(format!("remove {}: {err}", lockfile.display()));
    }
    write_if_changed(&project.join("src/main.rs"), &main)?;
    if let Some(publish_time) = publish_time
        && !lockfile.exists()
    {
        // `--publish-time` is unstable, and `$CARGO` is the current toolchain's cargo which cannot select another toolchain
        run_cargo(
            Command::new("cargo")
                .args(["+nightly", "generate-lockfile", "-Zunstable-options"])
                .arg("--publish-time")
                .arg(publish_time)
                .arg("--manifest-path")
                .arg(&manifest_path),
            &format!("generate lockfile for {name} (a nightly toolchain is required)"),
        )?;
    }

    run_cargo(
        Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
            .args(["build", "--quiet", "--manifest-path"])
            .arg(&manifest_path)
            .arg("--target-dir")
            .arg(&target)
            .args(publish_time.map(|_| "--locked")),
        &format!("build {name}"),
    )?;
    Ok(target
        .join("debug")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
}

/// Run a cargo command to `what`, returning its stderr in the error if it fails.
fn run_cargo(command: &mut Command, what: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|err| format!("run cargo to {what}: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "{what} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// Write `contents` to `path` if they differ from the existing contents, returning whether the file was written.
fn write_if_changed(path: &Path, contents: &str) -> Result<bool, String> {
    if std::fs::read_to_string(path).is_ok_and(|existing| existing == contents) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("create {}: {err}", parent.display()))?;
    }
    std::fs::write(path, contents).map_err(|err| format!("write {}: {err}", path.display()))?;
    Ok(true)
}

/// Run a batch of requests with a helper, returning a response for each request.
///
/// If the helper crashes (e.g. a segfault in a codec library), the requests are retried one per process.
pub(crate) fn run(binary: &Path, requests: &[Request]) -> Vec<Response> {
    match run_process(binary, requests) {
        Ok(responses) if responses.len() == requests.len() => responses,
        _ if requests.len() > 1 => requests
            .iter()
            .flat_map(|request| run(binary, std::slice::from_ref(request)))
            .collect(),
        Ok(_) => vec![Err("unexpected number of helper responses".to_string())],
        Err(err) => vec![Err(err)],
    }
}

fn run_process(binary: &Path, requests: &[Request]) -> Result<Vec<Response>, String> {
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn {}: {err}", binary.display()))?;
    let input = serde_json::to_vec(requests).map_err(|err| format!("serialise requests: {err}"))?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(&input)
        .map_err(|err| format!("write requests: {err}"))?;
    let output = child
        .wait_with_output()
        .map_err(|err| format!("wait for helper: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "helper crashed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|err| format!("parse helper responses: {err}"))
}
