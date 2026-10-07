//! Running cases with the current `zarrs` and previous releases.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;

use zarrs::array::{Array, ArrayBytes, ArrayMetadata, ArrayMetadataOptions, ArraySubset};
use zarrs::filesystem::FilesystemStore;

use crate::cases::{Case, DataTypeCase};
use crate::data::Data;
use crate::helper::{self, Request, Response};
use crate::releases::Release;

const ARRAY_PATH: &str = "/array";

/// The outcome of reading data in one direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Status {
    /// The data was read and matched the expected data.
    Ok,
    /// The writer could not write the data.
    NotWritten,
    /// The data could not be read or did not match.
    Fail(String),
}

/// The results of a case with a release.
#[derive(Debug, Clone)]
pub(crate) struct ReleaseResult {
    /// Data written by current read by the release.
    pub(crate) forward: Status,
    /// Data written by the release read by current.
    pub(crate) backward: Status,
    /// Data written by the release read by the release.
    pub(crate) roundtrip: Status,
}

/// The results of a case.
#[derive(Debug, Clone)]
pub(crate) struct CaseResult {
    /// The error if current could not write the case.
    pub(crate) current_write_error: Option<String>,
    /// Current reading its own data.
    pub(crate) current_roundtrip: Status,
    /// Results per release.
    pub(crate) releases: Vec<ReleaseResult>,
}

/// The work directory of a case written by `writer` (`current` or a release).
pub(crate) fn case_dir(work_dir: &Path, writer: &str, index: usize) -> PathBuf {
    work_dir.join(writer).join(index.to_string())
}

/// Run `cases` with the current `zarrs` and each of `releases` (in parallel).
///
/// # Errors
/// Returns an error if the work directory cannot be prepared or a helper fails to build.
pub(crate) fn run(
    cases: &[Case],
    combinations: &[crate::cases::Combination],
    data_types: &[DataTypeCase],
    releases: &[Release],
    work_dir: &Path,
) -> Result<Vec<CaseResult>, String> {
    if work_dir.exists() {
        std::fs::remove_dir_all(work_dir)
            .map_err(|err| format!("remove {}: {err}", work_dir.display()))?;
    }
    let case_data_types: Vec<&DataTypeCase> = cases
        .iter()
        .map(|case| &data_types[combinations[case.combination].data_type])
        .collect();

    let mut binaries = Vec::with_capacity(releases.len());
    for (index, release) in releases.iter().enumerate() {
        eprintln!(
            "building helper for zarrs {release} ({}/{})",
            index + 1,
            releases.len()
        );
        binaries.push(helper::build(*release)?);
    }

    eprintln!("writing {} cases with current zarrs", cases.len());
    let current: Vec<(Option<String>, Result<Data, String>)> = cases
        .par_iter()
        .enumerate()
        .map(|(index, case)| {
            let path = case_dir(work_dir, "current", index);
            match write_current(&path, case) {
                Ok(()) => (None, read_current(&path, &case.shape)),
                Err(err) => (Some(err), Err("not written".to_string())),
            }
        })
        .collect();

    eprintln!("testing {} releases", releases.len());
    let release_results: Vec<Vec<ReleaseResult>> = std::thread::scope(|scope| {
        let handles: Vec<_> = releases
            .iter()
            .zip(&binaries)
            .map(|(release, binary)| {
                let (current, case_data_types) = (&current, &case_data_types);
                scope.spawn(move || {
                    run_release(*release, binary, cases, current, case_data_types, work_dir)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("release thread panicked"))
            .collect()
    });

    Ok(cases
        .iter()
        .enumerate()
        .map(|(index, case)| {
            let (current_write_error, current_read) = &current[index];
            let current_roundtrip = if current_write_error.is_some() {
                Status::NotWritten
            } else {
                compare(current_read, &case.data, case.lossy, case_data_types[index])
            };
            CaseResult {
                current_write_error: current_write_error.clone(),
                current_roundtrip,
                releases: release_results
                    .iter()
                    .map(|results| results[index].clone())
                    .collect(),
            }
        })
        .collect())
}

fn run_release(
    release: Release,
    binary: &Path,
    cases: &[Case],
    current: &[(Option<String>, Result<Data, String>)],
    data_types: &[&DataTypeCase],
    work_dir: &Path,
) -> Vec<ReleaseResult> {
    let release_name = release.to_string();
    let requests: Vec<Request> = cases
        .iter()
        .enumerate()
        .flat_map(|(index, case)| {
            let release_dir = case_dir(work_dir, &release_name, index);
            [
                Request::Read {
                    path: case_dir(work_dir, "current", index),
                    shape: &case.shape,
                },
                Request::Write {
                    path: release_dir.clone(),
                    metadata: &case.metadata,
                    shape: &case.shape,
                    data: &case.data,
                },
                Request::Read {
                    path: release_dir,
                    shape: &case.shape,
                },
            ]
        })
        .collect();
    // Split the requests over multiple helper processes
    let chunk_size = 3 * cases.len().div_ceil(rayon::current_num_threads()).max(64);
    let responses: Vec<Response> = requests
        .par_chunks(chunk_size)
        .flat_map_iter(|requests| helper::run(binary, requests))
        .collect();

    cases
        .par_iter()
        .zip(current)
        .zip(responses.par_chunks_exact(3))
        .enumerate()
        .map(
            |(index, ((case, (current_write_error, current_read)), responses))| {
                let data_type = data_types[index];
                let [release_read, release_write, release_self_read] = responses else {
                    unreachable!()
                };
                let release_read = release_read.clone().map(Option::unwrap_or_default);
                let release_self_read = release_self_read.clone().map(Option::unwrap_or_default);

                // Lossy data is compared against the writer's own decoding
                let forward = if current_write_error.is_some() {
                    Status::NotWritten
                } else if case.lossy {
                    match current_read {
                        Ok(expected) => compare(&release_read, expected, false, data_type),
                        Err(err) => {
                            Status::Fail(format!("current failed to read its own data: {err}"))
                        }
                    }
                } else {
                    compare(&release_read, &case.data, false, data_type)
                };

                let (backward, roundtrip) = if release_write.is_err() {
                    (Status::NotWritten, Status::NotWritten)
                } else {
                    let read = read_current(&case_dir(work_dir, &release_name, index), &case.shape);
                    let backward = match (&release_self_read, case.lossy) {
                        (Ok(expected), true) => compare(&read, expected, false, data_type),
                        (Err(_), true) => compare(&read, &Data::default(), true, data_type),
                        (_, false) => compare(&read, &case.data, false, data_type),
                    };
                    let roundtrip = compare(&release_self_read, &case.data, case.lossy, data_type);
                    (backward, roundtrip)
                };

                ReleaseResult {
                    forward,
                    backward,
                    roundtrip,
                }
            },
        )
        .collect()
}

/// Compare read data with the expected data. If `lossy`, any successfully read data matches.
fn compare(
    read: &Result<Data, String>,
    expected: &Data,
    lossy: bool,
    data_type: &DataTypeCase,
) -> Status {
    match read {
        Err(err) => Status::Fail(err.clone()),
        Ok(_) if lossy => Status::Ok,
        Ok(read) => {
            let read = data_type.canonical(read);
            let expected = data_type.canonical(expected);
            if read == expected {
                Status::Ok
            } else {
                Status::Fail(mismatch(&read, &expected))
            }
        }
    }
}

fn mismatch(read: &Data, expected: &Data) -> String {
    if read.masks != expected.masks {
        "decoded validity mask differs".to_string()
    } else if read.offsets != expected.offsets {
        "decoded element offsets differ".to_string()
    } else if read.bytes.len() != expected.bytes.len() {
        format!(
            "decoded length differs: {} bytes, expected {}",
            read.bytes.len(),
            expected.bytes.len()
        )
    } else {
        let offset = read
            .bytes
            .iter()
            .zip(&expected.bytes)
            .position(|(read, expected)| read != expected)
            .unwrap_or_default();
        format!(
            "decoded bytes differ at byte {offset}: 0x{:02x}, expected 0x{:02x}",
            read.bytes[offset], expected.bytes[offset]
        )
    }
}

fn write_current(path: &Path, case: &Case) -> Result<(), String> {
    let store = Arc::new(FilesystemStore::new(path).map_err(|err| format!("create store: {err}"))?);
    let metadata = serde_json::from_value::<ArrayMetadata>(case.metadata.clone())
        .map_err(|err| format!("parse metadata: {err}"))?;
    let array = Array::new_with_metadata(store, ARRAY_PATH, metadata)
        .map_err(|err| format!("create array: {err}"))?
        .with_metadata_options(ArrayMetadataOptions::default().with_include_zarrs_metadata(false));
    array
        .store_metadata()
        .map_err(|err| format!("store metadata: {err}"))?;
    let bytes = case.data.to_array_bytes()?;
    array
        .store_array_subset(&ArraySubset::new_with_shape(case.shape.clone()), bytes)
        .map_err(|err| format!("store array subset: {err}"))
}

fn read_current(path: &Path, shape: &[u64]) -> Result<Data, String> {
    let store = Arc::new(FilesystemStore::new(path).map_err(|err| format!("create store: {err}"))?);
    let array = Array::open(store, ARRAY_PATH).map_err(|err| format!("open array: {err}"))?;
    let bytes = array
        .retrieve_array_subset::<ArrayBytes>(&ArraySubset::new_with_shape(shape.to_vec()))
        .map_err(|err| format!("retrieve array subset: {err}"))?;
    Ok(Data::from_array_bytes(bytes))
}
