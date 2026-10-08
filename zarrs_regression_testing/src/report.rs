//! A self-contained HTML report of a run.
//!
//! The report opens with an overview matrix of every codec and data type combination, followed by collapsible details of each failure.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;
use std::path::Path;

use serde_json::Value;

use zarrs::array::FillValue;

use crate::cases::{DataTypeCase, Values};
use crate::data::Data;
use crate::releases::Release;
use crate::run::{CaseResult, Status, case_dir};
use crate::summary::{CombinationStatus, Failure, FailureKind, Run};

/// The parameters of a run.
pub(crate) struct Meta<'a> {
    pub(crate) seed: u64,
    pub(crate) samples: usize,
    pub(crate) all: bool,
    pub(crate) filter: Option<&'a str>,
}

/// The status of a combination in the overview matrix, most severe first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Cell {
    Regression,
    KnownIssue,
    Older,
    Other,
    New,
    Unsupported,
    /// Some combinations of a row are supported by a release (release views only).
    Partial,
    Compatible,
}

impl Cell {
    const ALL: [Self; 7] = [
        Self::Regression,
        Self::KnownIssue,
        Self::Older,
        Self::Other,
        Self::New,
        Self::Unsupported,
        Self::Compatible,
    ];

    fn class(self) -> &'static str {
        match self {
            Self::Regression => "regression",
            Self::KnownIssue => "known",
            Self::Older => "older",
            Self::Other => "other",
            Self::New => "new",
            Self::Unsupported => "unsupported",
            Self::Partial => "partial",
            Self::Compatible => "compatible",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Regression => "regression",
            Self::KnownIssue => "known issue",
            Self::Older => "incompatible with an older release",
            Self::Other => "other",
            Self::New => "new/fixed in current",
            Self::Unsupported => "unsupported",
            Self::Partial => "partially supported",
            Self::Compatible => "compatible",
        }
    }

    /// The prefix of the anchor of the details of a combination with this status.
    fn anchor(self) -> Option<&'static str> {
        match self {
            Self::Regression => Some("r"),
            Self::KnownIssue => Some("k"),
            Self::Older => Some("o"),
            _ => None,
        }
    }
}

const STYLE: &str = r"
:root { --fg: #1f2328; --muted: #656d76; --bg: #fff; --panel: #f6f8fa; --border: #d0d7de;
  --regression: #cf222e; --known: #e16f24; --older: #d4a72c; --other: #8250df; --new: #0969da;
  --unsupported: #d0d7de; --partial: #9be9a8; --compatible: #2da44e; }
@media (prefers-color-scheme: dark) {
  :root { --fg: #e6edf3; --muted: #8d96a0; --bg: #0d1117; --panel: #161b22; --border: #30363d;
    --unsupported: #30363d; --partial: #196c2e; }
}
body { font: 14px/1.45 system-ui, sans-serif; color: var(--fg); background: var(--bg); margin: 1.5em auto; max-width: 1400px; padding: 0 1em; }
h1 { margin: 0; font-size: 1.6em; }
h2 { display: inline; font-size: 1.2em; }
code, pre { font: 12px/1.4 ui-monospace, monospace; }
pre { background: var(--panel); border: 1px solid var(--border); border-radius: 4px; padding: .5em; overflow-x: auto; margin: .3em 0; }
.muted { color: var(--muted); }
.verdict { font-size: 1.15em; font-weight: 600; padding: .5em .8em; border-radius: 6px; margin: .8em 0; border-left: 6px solid; background: var(--panel); }
.verdict.pass { border-color: var(--compatible); }
.verdict.fail { border-color: var(--regression); }
.toolbar button { font: inherit; margin-right: .4em; }
details.section { margin: 1.2em 0; }
details.section > summary { cursor: pointer; padding: .3em 0; border-bottom: 1px solid var(--border); }
details.combination { border: 1px solid var(--border); border-radius: 4px; margin: .3em 0; }
details.combination > summary { cursor: pointer; padding: .3em .5em; }
details.combination[open] > summary { background: var(--panel); border-bottom: 1px solid var(--border); }
details.combination > .body { padding: .3em .8em .6em; }
.cross { color: var(--regression); font-weight: 700; }
.directions { margin-left: .6em; }
.message { color: var(--muted); margin-left: .6em; font-family: ui-monospace, monospace; font-size: 12px; }
.case { border-top: 1px dashed var(--border); padding: .4em 0; }
.case:first-child { border-top: none; }
.case details > summary { cursor: pointer; color: var(--muted); }
table.list { border-collapse: collapse; margin: .3em 0; }
table.list th, table.list td { text-align: left; vertical-align: top; padding: .2em .6em .2em 0; }
table.list td.error { font-family: ui-monospace, monospace; font-size: 12px; white-space: pre-wrap; }
.legend span.item { margin-right: 1em; white-space: nowrap; }
.swatch { display: inline-block; width: 12px; height: 12px; border-radius: 2px; vertical-align: -1px; margin-right: .3em; }
.matrix-wrap { overflow: auto; max-height: 85vh; border: 1px solid var(--border); }
table.matrix { border-collapse: separate; border-spacing: 1px; font-size: 11px; }
table.matrix thead th { position: sticky; top: 0; background: var(--bg); z-index: 1; vertical-align: bottom; font-weight: 400; }
table.matrix thead th div { writing-mode: vertical-rl; transform: rotate(180deg); white-space: nowrap; padding: 2px 0; }
table.matrix tbody th { position: sticky; left: 0; background: var(--bg); text-align: right; font-weight: 400; white-space: nowrap; padding-right: .5em; }
table.matrix thead th:first-child { left: 0; z-index: 2; }
table.matrix td { width: 13px; min-width: 13px; height: 13px; padding: 0; border-radius: 2px; }
table.matrix td a { display: block; width: 100%; height: 100%; }
table.matrix tr:hover th { color: var(--new); }
.since { display: inline-block; min-inline-size: 2.8em; text-align: end; color: var(--muted); font-size: 10px; }
thead .since { text-align: start; }
.since-new { color: var(--new); font-weight: 600; }
.regression { background: var(--regression); }
.known { background: var(--known); }
.older { background: var(--older); }
.other { background: var(--other); }
.new { background: var(--new); }
.unsupported { background: var(--unsupported); }
.partial { background: var(--partial); }
.compatible { background: var(--compatible); }
.views button { font: inherit; margin-right: .4em; }
.views button[aria-pressed=true] { font-weight: 600; }
table.releases td { width: 26px; min-width: 26px; }
table.releases thead th div { writing-mode: horizontal-tb; transform: none; padding: 0 2px; }
";

const SCRIPT: &str = r"
function setAll(open) {
  document.querySelectorAll(open ? 'details.section, details.combination' : 'details.combination')
    .forEach(details => details.open = open);
}
function showView(id) {
  document.querySelectorAll('.view').forEach(view => view.hidden = view.id !== id);
  document.querySelectorAll('.views button')
    .forEach(button => button.setAttribute('aria-pressed', button.dataset.view === id));
}
function reveal() {
  const target = location.hash && document.getElementById(location.hash.slice(1));
  if (!target) return;
  for (let element = target; element; element = element.parentElement) {
    if (element.tagName === 'DETAILS') element.open = true;
  }
  target.scrollIntoView();
}
addEventListener('hashchange', reveal);
reveal();
";

/// Escape text for HTML content and attribute values.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for char in text.chars() {
        match char {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(char),
        }
    }
    escaped
}

/// A `file://` URL of a path.
fn file_url(path: &Path) -> String {
    let mut url = "file://".to_string();
    for char in path.to_string_lossy().chars() {
        match char {
            ' ' => url.push_str("%20"),
            '#' => url.push_str("%23"),
            '%' => url.push_str("%25"),
            '?' => url.push_str("%3F"),
            '\\' => url.push('/'),
            _ => url.push(char),
        }
    }
    url
}

/// Truncate `text` to its first line and at most `max` characters.
fn truncate(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() > max || line.len() < text.len() {
        format!("{}…", line.chars().take(max).collect::<String>())
    } else {
        line.to_string()
    }
}

/// Format the elements and bytes of data as grids in the shape of the array.
fn format_data(data: &Data, shape: &[u64], data_type: &DataTypeCase) -> String {
    let element_bytes: Vec<&[u8]> = if let Some(offsets) = &data.offsets {
        offsets
            .windows(2)
            .map(|window| &data.bytes[window[0]..window[1]])
            .collect()
    } else if let Some(size) = data_type.values.element_size().filter(|&size| size > 0) {
        data.bytes.chunks(size).collect()
    } else {
        vec![&data.bytes]
    };
    let elements: Vec<String> = element_bytes
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            // A null at depth `n` (outermost first) is wrapped `n` times, as in fill value metadata
            match data
                .masks
                .iter()
                .position(|mask| mask.get(index) == Some(&0))
            {
                Some(depth) => format!("{}null{}", "[".repeat(depth), "]".repeat(depth)),
                None => format_element(data_type, bytes),
            }
        })
        .collect();
    let bytes: Vec<String> = element_bytes.iter().map(|bytes| hex(bytes)).collect();
    let columns = shape
        .last()
        .and_then(|&columns| usize::try_from(columns).ok())
        .filter(|&columns| columns > 0)
        .unwrap_or(elements.len().max(1));
    let grid = |elements: &[String]| {
        let width = elements
            .iter()
            .map(|element| element.chars().count())
            .max()
            .unwrap_or_default();
        elements
            .chunks(columns)
            .map(|row| {
                row.iter()
                    .map(|element| format!("{element:>width$}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "{} elements ({})\n{}\n\n{} bytes{} (hex)\n{}",
        elements.len(),
        data_type.label,
        grid(&elements),
        data.bytes.len(),
        if data.offsets.is_some() {
            ", variable length"
        } else {
            ""
        },
        grid(&bytes)
    )
}

/// Format the bytes of a (non-null) element as in fill value metadata (e.g. `-1`, `1.5`, `NaN`, `"text"`), with complex numbers as `1.5-2j`.
///
/// Raw bits and bytes, and elements that cannot be formatted, are formatted in hex.
fn format_element(data_type: &DataTypeCase, bytes: &[u8]) -> String {
    let mut inner = &data_type.data_type;
    while let Some(optional_inner) = inner.optional_inner() {
        inner = optional_inner;
    }
    let mut values = &data_type.values;
    while let Values::Optional(optional_values) = values {
        values = optional_values;
    }
    if matches!(values, Values::Raw(_) | Values::Bytes) {
        return hex(bytes);
    }
    let Some(value) = inner
        .metadata_fill_value(&FillValue::new(bytes.to_vec()))
        .ok()
        .and_then(|metadata| serde_json::to_value(metadata).ok())
    else {
        return hex(bytes);
    };
    // Strings are quoted with non-printable characters (e.g. bidirectional overrides) escaped, but not special values (e.g. `NaN`, `NaT`)
    let quote = matches!(values, Values::String | Values::Utf32(_));
    let scalar = |value: &Value| match value {
        Value::String(string) if quote => format!("{string:?}"),
        Value::String(string) => string.clone(),
        value => value.to_string(),
    };
    match value.as_array().map(Vec::as_slice) {
        Some([re, im]) => {
            let (re, im) = (scalar(re), scalar(im));
            match im.strip_prefix('-') {
                Some(im) => format!("{re}-{im}j"),
                None => format!("{re}+{im}j"),
            }
        }
        _ => scalar(&value),
    }
}

/// Format JSON with indentation, keeping short arrays and objects on one line.
fn format_json(value: &Value, indent: usize, out: &mut String) {
    let compact = value.to_string();
    let (open, close, items): (char, char, Vec<(Option<&String>, &Value)>) = match value {
        Value::Array(items) => ('[', ']', items.iter().map(|item| (None, item)).collect()),
        Value::Object(map) => (
            '{',
            '}',
            map.iter().map(|(key, item)| (Some(key), item)).collect(),
        ),
        _ => ('\0', '\0', vec![]),
    };
    if items.is_empty() || indent + compact.len() <= 80 {
        out.push_str(&compact);
        return;
    }
    out.push(open);
    for (index, (key, item)) in items.iter().enumerate() {
        let _ = write!(out, "\n{:width$}", "", width = indent + 2);
        if let Some(key) = key {
            let _ = write!(out, "{}: ", Value::from(key.as_str()));
        }
        format_json(item, indent + 2, out);
        if index + 1 < items.len() {
            out.push(',');
        }
    }
    let _ = write!(out, "\n{:indent$}{close}", "");
}

fn hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "∅".to_string();
    }
    bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// Generate an HTML report of a run.
#[must_use]
pub(crate) fn html(
    run: &Run,
    failures: &[Failure],
    known_issues: &[Failure],
    meta: &Meta,
) -> String {
    // Link to the work directory without `..`
    let work_dir =
        std::fs::canonicalize(run.work_dir).unwrap_or_else(|_| run.work_dir.to_path_buf());
    let run = &Run {
        work_dir: &work_dir,
        ..*run
    };
    let latest = run.releases[0];
    let (latest_failures, older_failures): (Vec<&Failure>, Vec<&Failure>) = failures
        .iter()
        .partition(|failure| failure.release.is_none_or(|release| release == latest));
    let known_issues: Vec<&Failure> = known_issues.iter().collect();

    let mut out = String::new();
    let _ = write!(
        out,
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>zarrs regression testing</title>\n<style>{STYLE}</style>\n</head>\n<body>\n"
    );
    header(&mut out, run, &latest_failures, known_issues.len(), meta);
    overview(
        &mut out,
        run,
        [
            (&older_failures, Cell::Older),
            (&known_issues, Cell::KnownIssue),
            (&latest_failures, Cell::Regression),
        ],
        meta,
    );

    if latest_failures.is_empty() {
        failure_section(
            &mut out,
            run,
            &format!("Regressions with the latest release ({latest})"),
            &format!("✓ None: current zarrs and zarrs {latest} read each other's data."),
            &latest_failures,
            true,
        );
    } else {
        failure_section(
            &mut out,
            run,
            &format!("Regressions with the latest release ({latest})"),
            &format!(
                "Current zarrs cannot read back its own data, or current zarrs and {latest} cannot read each other's data (where {latest} reads back its own)."
            ),
            &latest_failures,
            true,
        );
        failure_details(&mut out, run, &latest_failures, "r");
    }
    out.push_str("</details>\n");

    if !known_issues.is_empty() {
        failure_section(
            &mut out,
            run,
            "Known issues",
            &format!("Neither current zarrs nor {latest} can read back data they wrote."),
            &known_issues,
            false,
        );
        failure_details(&mut out, run, &known_issues, "k");
        out.push_str("</details>\n");
    }

    if meta.all {
        if !older_failures.is_empty() {
            failure_section(
                &mut out,
                run,
                "Incompatibilities with older releases",
                "Older releases that read back their own data, but cannot read data written by current zarrs or vice versa.",
                &older_failures,
                false,
            );
            out.push_str(
                "<table class=\"list\"><tr><th>direction</th><th>releases</th><th>combinations</th></tr>\n",
            );
            for row in run.concise_rows(&older_failures) {
                let _ = writeln!(
                    out,
                    "<tr><td>{}</td><td>{}</td><td><b>{}</b>: {}</td></tr>",
                    row.kind.label(),
                    escape(&row.releases),
                    escape(&row.codecs),
                    escape(&row.data_types)
                );
            }
            out.push_str("</table>\n");
            failure_details(&mut out, run, &older_failures, "o");
            out.push_str("</details>\n");
        }
        compatibility(&mut out, run);
    }

    let _ = write!(out, "<script>{SCRIPT}</script>\n</body>\n</html>\n");
    out
}

/// Start a collapsible section of failures, with the number of combinations affected.
fn failure_section(
    out: &mut String,
    run: &Run,
    title: &str,
    description: &str,
    failures: &[&Failure],
    open: bool,
) {
    let combinations: BTreeSet<usize> = failures
        .iter()
        .map(|failure| run.cases[failure.case].combination)
        .collect();
    let _ = writeln!(
        out,
        "<details class=\"section\"{}><summary><h2>{}</h2> <span class=\"muted\">{} combinations</span></summary>\n<p class=\"muted\">{}</p>",
        if open { " open" } else { "" },
        escape(title),
        combinations.len(),
        escape(description)
    );
}

fn header(
    out: &mut String,
    run: &Run,
    latest_failures: &[&Failure],
    known_issues: usize,
    meta: &Meta,
) {
    let latest = run.releases[0];
    let releases = if meta.all {
        format!("releases {}–{latest}", run.releases[run.releases.len() - 1])
    } else {
        format!("latest release {latest}")
    };
    let mut command = format!(
        "cargo run -p zarrs_regression_testing -- --seed {} --samples {}",
        meta.seed, meta.samples
    );
    if meta.all {
        command.push_str(" --all");
    }
    if let Some(filter) = meta.filter {
        let _ = write!(command, " --filter '{filter}'");
    }
    let _ = write!(
        out,
        "<h1>zarrs regression testing</h1>\n<p class=\"muted\">current zarrs vs {releases}: {} combinations × {} samples = {} cases</p>\n",
        run.combinations.len(),
        meta.samples,
        run.cases.len()
    );
    if latest_failures.is_empty() {
        let _ = writeln!(
            out,
            "<div class=\"verdict pass\">✓ No regressions: current zarrs and zarrs {latest} read each other's data</div>"
        );
    } else {
        let cases: BTreeSet<usize> = latest_failures.iter().map(|failure| failure.case).collect();
        let _ = writeln!(
            out,
            "<div class=\"verdict fail\">✗ {} failing cases with the latest release ({latest})</div>",
            cases.len()
        );
    }
    let _ = write!(
        out,
        "<p>{}{}</p>\n<p>Reproduce: <code>{}</code><br>Data: <a href=\"{}\"><code>{}</code></a> <span class=\"muted\">(replaced by the next run)</span></p>\n<p class=\"toolbar\"><button onclick=\"setAll(true)\">Expand all</button><button onclick=\"setAll(false)\">Collapse all</button></p>\n",
        escape(&run.counts()),
        if known_issues > 0 {
            format!(", {known_issues} known issues")
        } else {
            String::new()
        },
        escape(&command),
        escape(&file_url(run.work_dir)),
        escape(&run.work_dir.display().to_string())
    );
}

/// The status of each combination: that of its most severe failure, otherwise its status with the latest release.
fn cells(
    run: &Run,
    by_combination: &[Vec<&CaseResult>],
    failures: [(&[&Failure], Cell); 3],
) -> Vec<Cell> {
    let mut cells: Vec<Cell> = by_combination
        .iter()
        .map(|results| match Run::status(results) {
            CombinationStatus::Compatible => Cell::Compatible,
            CombinationStatus::New => Cell::New,
            CombinationStatus::Unsupported => Cell::Unsupported,
            CombinationStatus::Other => Cell::Other,
        })
        .collect();
    for (failures, cell) in failures {
        for failure in failures {
            let combination = run.cases[failure.case].combination;
            cells[combination] = cells[combination].min(cell);
        }
    }
    cells
}

/// The overview: matrices of codecs × data types, codecs × releases, and data types × releases.
fn overview(out: &mut String, run: &Run, failures: [(&[&Failure], Cell); 3], meta: &Meta) {
    let by_combination = run.by_combination();
    // (combination, release or current) -> most severe failure
    let mut failed: HashMap<(usize, Option<Release>), Cell> = HashMap::new();
    for (failures, cell) in failures {
        for failure in failures {
            let release = failure
                .release
                .filter(|_| failure.kind != FailureKind::CurrentToCurrent);
            let entry = failed
                .entry((run.cases[failure.case].combination, release))
                .or_insert(cell);
            *entry = (*entry).min(cell);
        }
    }
    let cells = cells(run, &by_combination, failures);

    let mut codecs: Vec<(String, Vec<usize>)> = Vec::new();
    let mut data_types: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (combination_index, combination) in run.combinations.iter().enumerate() {
        let codec = combination.codec.to_string();
        match codecs.last_mut() {
            Some((last, combinations)) if *last == codec => combinations.push(combination_index),
            _ => codecs.push((codec, vec![combination_index])),
        }
        data_types
            .entry(combination.data_type)
            .or_default()
            .push(combination_index);
    }
    let data_types: Vec<(String, Vec<usize>)> = data_types
        .into_iter()
        .map(|(data_type, combinations)| {
            (run.data_types[data_type].label.to_string(), combinations)
        })
        .collect();

    out.push_str(
        "<details class=\"section\" open><summary><h2>Overview</h2></summary>\n<p class=\"views\">",
    );
    for (id, label, pressed) in [
        ("view-combinations", "codecs × data types", true),
        ("view-codecs", "codecs × releases", false),
        ("view-data-types", "data types × releases", false),
    ] {
        let _ = write!(
            out,
            "<button data-view=\"{id}\" aria-pressed=\"{pressed}\" onclick=\"showView(this.dataset.view)\">{label}</button>"
        );
    }
    out.push_str("</p>\n<div class=\"view\" id=\"view-combinations\">\n");
    combination_matrix(
        out,
        run,
        &by_combination,
        &cells,
        &codecs,
        &data_types,
        meta,
    );
    out.push_str("</div>\n<div class=\"view\" id=\"view-codecs\" hidden>\n");
    release_matrix(
        out,
        run,
        &by_combination,
        &failed,
        &codecs,
        "data types",
        meta,
    );
    out.push_str("</div>\n<div class=\"view\" id=\"view-data-types\" hidden>\n");
    release_matrix(
        out,
        run,
        &by_combination,
        &failed,
        &data_types,
        "codecs",
        meta,
    );
    out.push_str("</div>\n</details>\n");
}

/// A matrix of `codecs` (rows) and `data_types` (columns), with their combinations.
fn combination_matrix(
    out: &mut String,
    run: &Run,
    by_combination: &[Vec<&CaseResult>],
    cells: &[Cell],
    codecs: &[(String, Vec<usize>)],
    data_types: &[(String, Vec<usize>)],
    meta: &Meta,
) {
    let labels = if meta.all {
        "the oldest tested release that supports the codec or data type, or <span class=\"since since-new\">new</span> if none do".to_string()
    } else {
        format!(
            "<span class=\"since since-new\">new</span> if not supported by {} (run with <code>--all</code> for when each was introduced)",
            run.releases[0]
        )
    };
    let _ = write!(
        out,
        "<p class=\"muted\">Compared with the latest release; labelled with {labels}.</p>\n<p class=\"legend\">"
    );
    for cell in Cell::ALL {
        let count = cells.iter().filter(|&&other| other == cell).count();
        let optional = match cell {
            Cell::KnownIssue => true,
            Cell::Older => !meta.all,
            _ => false,
        };
        if count > 0 || !optional {
            let _ = write!(
                out,
                "<span class=\"item\"><span class=\"swatch {}\"></span>{} ({count})</span>",
                cell.class(),
                cell.label()
            );
        }
    }
    out.push_str("<span class=\"item muted\">blank: not applicable</span></p>\n<div class=\"matrix-wrap\"><table class=\"matrix\">\n<thead><tr><th></th>");
    for (data_type, combinations) in data_types {
        let _ = write!(
            out,
            "<th><div>{} {}</div></th>",
            since(run, by_combination, combinations.iter().copied(), meta.all),
            escape(data_type)
        );
    }
    out.push_str("</tr></thead>\n<tbody>\n");
    for (codec, codec_combinations) in codecs {
        let _ = write!(
            out,
            "<tr><th>{} {}</th>",
            escape(codec),
            since(
                run,
                by_combination,
                codec_combinations.iter().copied(),
                meta.all
            )
        );
        for (data_type, combinations) in data_types {
            let Some(&combination) = combinations
                .iter()
                .find(|combination| codec_combinations.contains(combination))
            else {
                out.push_str("<td></td>");
                continue;
            };
            let cell = cells[combination];
            let mut title = format!("{codec} × {data_type}: {}", cell.label());
            if meta.all
                && let Some(((_, forward), (_, backward))) =
                    run.bounds(&by_combination[combination])
            {
                let _ = write!(
                    title,
                    "\ncurrent→release: {forward}\nrelease→current: {backward}"
                );
            }
            cell_html(out, cell, &title, combination);
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody></table></div>\n");
}

/// A matrix of `rows` (with their combinations, e.g. a codec and its data types) and releases (oldest first, then current).
///
/// A cell has the status of the most severe failure of its combinations, otherwise whether the release reads back its own data for all, some, or none of the combinations that current supports.
/// Current is compared with the combinations that any release supports.
fn release_matrix(
    out: &mut String,
    run: &Run,
    by_combination: &[Vec<&CaseResult>],
    failed: &HashMap<(usize, Option<Release>), Cell>,
    rows: &[(String, Vec<usize>)],
    noun: &str,
    meta: &Meta,
) {
    let columns: Vec<Option<usize>> = (0..run.releases.len())
        .rev()
        .map(Some)
        .chain([None])
        .collect();
    let _ = write!(
        out,
        "<p class=\"muted\">Whether each release reads back its own data for the {noun} of each row that current zarrs supports, and whether it is compatible with current zarrs. Current zarrs is compared with the {noun} supported by any tested release.{}</p>\n<p class=\"legend\">",
        if meta.all {
            ""
        } else {
            " Run with <code>--all</code> to test every release."
        }
    );
    for cell in [
        Cell::Regression,
        Cell::KnownIssue,
        Cell::Older,
        Cell::Compatible,
        Cell::Partial,
        Cell::Unsupported,
    ] {
        if cell == Cell::KnownIssue && !failed.values().any(|&failed| failed == cell) {
            continue;
        }
        let label = match cell {
            Cell::Compatible => format!("all {noun} supported"),
            Cell::Partial => format!("some {noun} unsupported"),
            Cell::Unsupported => format!("no {noun} supported"),
            cell => cell.label().to_string(),
        };
        let _ = write!(
            out,
            "<span class=\"item\"><span class=\"swatch {}\"></span>{label}</span>",
            cell.class()
        );
    }
    out.push_str(
        "</p>\n<div class=\"matrix-wrap\"><table class=\"matrix releases\">\n<thead><tr><th></th>",
    );
    for column in &columns {
        let _ = write!(
            out,
            "<th><div>{}</div></th>",
            column.map_or_else(
                || "current".to_string(),
                |index| run.releases[index].to_string()
            )
        );
    }
    out.push_str("</tr></thead>\n<tbody>\n");
    for (row, combinations) in rows {
        let _ = write!(out, "<tr><th>{}</th>", escape(row));
        for &column in &columns {
            let release = column.map(|index| run.releases[index]);
            let (supported, missing) = support(run, by_combination, combinations, column);
            let failing: Vec<(Cell, usize)> = combinations
                .iter()
                .filter_map(|&combination| {
                    failed
                        .get(&(combination, release))
                        .map(|&cell| (cell, combination))
                })
                .collect();
            let (cell, combination) = failing.iter().min().copied().unwrap_or_else(|| {
                let cell = if supported == 0 {
                    Cell::Unsupported
                } else if missing == 0 {
                    Cell::Compatible
                } else {
                    Cell::Partial
                };
                (cell, 0)
            });
            let mut title = format!(
                "{row} × {}: {supported} {noun} supported",
                release.map_or_else(|| "current".to_string(), |release| release.to_string())
            );
            if missing > 0 {
                let _ = write!(
                    title,
                    ", {missing} more supported by {}",
                    if column.is_some() {
                        "current"
                    } else {
                        "a release"
                    }
                );
            }
            if !failing.is_empty() {
                let _ = write!(title, ", {} failing", failing.len());
            }
            cell_html(out, cell, &title, combination);
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody></table></div>\n");
}

/// The number of `combinations` supported by a release (by index, or current if [`None`]), and the number missing that are supported by current (or by any release for current).
///
/// A release (or current) supports a combination if it reads back its own data for all cases.
fn support(
    run: &Run,
    by_combination: &[Vec<&CaseResult>],
    combinations: &[usize],
    column: Option<usize>,
) -> (usize, usize) {
    let supports = |combination: usize, column: Option<usize>| {
        by_combination[combination].iter().all(|result| {
            let roundtrip = column.map_or(&result.current_roundtrip, |index| {
                &result.releases[index].roundtrip
            });
            *roundtrip == Status::Ok
        })
    };
    let supported = combinations
        .iter()
        .filter(|&&combination| supports(combination, column))
        .count();
    let missing = combinations
        .iter()
        .filter(|&&combination| {
            !supports(combination, column)
                && if column.is_some() {
                    supports(combination, None)
                } else {
                    (0..run.releases.len()).any(|index| supports(combination, Some(index)))
                }
        })
        .count();
    (supported, missing)
}

/// A matrix cell, linking to the details of `combination` if it failed.
fn cell_html(out: &mut String, cell: Cell, title: &str, combination: usize) {
    let _ = write!(
        out,
        "<td class=\"{}\" title=\"{}\">",
        cell.class(),
        escape(title)
    );
    if let Some(prefix) = cell.anchor() {
        let _ = write!(out, "<a href=\"#{prefix}{combination}\"></a>");
    }
    out.push_str("</td>");
}

/// A label of the oldest tested release that reads back its own data for any of `combinations` (if `all` releases are tested), or `new` if none do but current does.
///
/// Labels have a minimum width, so that labels before or after them are aligned.
fn since(
    run: &Run,
    by_combination: &[Vec<&CaseResult>],
    combinations: impl Iterator<Item = usize>,
    all: bool,
) -> String {
    let results: Vec<&CaseResult> = combinations
        .flat_map(|combination| by_combination[combination].iter().copied())
        .collect();
    let oldest = results
        .iter()
        .filter_map(|result| {
            result
                .releases
                .iter()
                .rposition(|release| release.roundtrip == Status::Ok)
        })
        .max();
    match oldest {
        Some(oldest) if all => format!("<span class=\"since\">{}</span>", run.releases[oldest]),
        None if results
            .iter()
            .any(|result| result.current_roundtrip == Status::Ok) =>
        {
            "<span class=\"since since-new\">new</span>".to_string()
        }
        // Keep labels aligned
        _ if all => "<span class=\"since\"></span>".to_string(),
        _ => String::new(),
    }
}

/// Describe the failures of a kind, e.g. `current→0.13–0.20`.
fn direction(run: &Run, kind: FailureKind, releases: &BTreeSet<Release>) -> String {
    let include: Vec<bool> = run
        .releases
        .iter()
        .map(|release| releases.contains(release))
        .collect();
    let releases = run.ranges(&include).join(", ");
    match kind {
        FailureKind::CurrentToCurrent => "current→current".to_string(),
        FailureKind::CurrentToRelease => format!("current→{releases}"),
        FailureKind::ReleaseToCurrent => format!("{releases}→current"),
        FailureKind::CurrentCannotWrite => format!("current cannot write (unlike {releases})"),
    }
}

/// The failures of each case (of a combination).
type CaseFailures<'a> = BTreeMap<usize, Vec<&'a Failure>>;

/// The releases and cases affected by `failures`, by kind.
fn kinds<'a>(
    failures: impl IntoIterator<Item = &'a &'a Failure>,
) -> BTreeMap<FailureKind, (BTreeSet<Release>, BTreeSet<usize>)> {
    let mut kinds: BTreeMap<FailureKind, (BTreeSet<Release>, BTreeSet<usize>)> = BTreeMap::new();
    for failure in failures {
        let (releases, cases) = kinds.entry(failure.kind).or_default();
        releases.extend(failure.release);
        cases.insert(failure.case);
    }
    kinds
}

/// Collapsible details of failures, one per combination (with id `{prefix}{combination}`).
///
/// The combinations of a codec are grouped if there is more than one.
fn failure_details(out: &mut String, run: &Run, failures: &[&Failure], prefix: &str) {
    // combination -> case -> failures
    let mut grouped: BTreeMap<usize, CaseFailures> = BTreeMap::new();
    for &failure in failures {
        grouped
            .entry(run.cases[failure.case].combination)
            .or_default()
            .entry(failure.case)
            .or_default()
            .push(failure);
    }
    // Combinations are ordered by codec
    let mut codecs: Vec<(String, Vec<(usize, CaseFailures)>)> = Vec::new();
    for (combination, cases) in grouped {
        let (codec, _) = run.label(combination);
        match codecs.last_mut() {
            Some((last, combinations)) if *last == codec => {
                combinations.push((combination, cases));
            }
            _ => codecs.push((codec, vec![(combination, cases)])),
        }
    }
    for (codec, combinations) in codecs {
        if combinations.len() == 1 {
            let (combination, cases) = &combinations[0];
            combination_details(out, run, *combination, cases, prefix);
            continue;
        }
        let data_types: Vec<&str> = combinations
            .iter()
            .map(|(combination, _)| run.label(*combination).1)
            .collect();
        let directions: Vec<String> = kinds(
            combinations
                .iter()
                .flat_map(|(_, cases)| cases.values().flatten()),
        )
        .iter()
        .map(|(&kind, (releases, _))| direction(run, kind, releases))
        .collect();
        let _ = write!(
            out,
            "<details class=\"combination\"><summary><span class=\"cross\">✗</span> <b>{}</b> · {} data types<span class=\"directions\">{}</span><span class=\"message\">{}</span></summary>\n<div class=\"body\">\n",
            escape(&codec),
            data_types.len(),
            escape(&directions.join(", ")),
            escape(&truncate(&data_types.join(" "), 100))
        );
        for (combination, cases) in &combinations {
            combination_details(out, run, *combination, cases, prefix);
        }
        out.push_str("</div></details>\n");
    }
}

/// Collapsible details of the failures of a combination, by case.
fn combination_details(
    out: &mut String,
    run: &Run,
    combination: usize,
    cases: &CaseFailures,
    prefix: &str,
) {
    let (codec, data_type) = run.label(combination);
    let first_case = run
        .cases
        .iter()
        .position(|case| case.combination == combination)
        .unwrap_or_default();
    let samples = run
        .cases
        .iter()
        .filter(|case| case.combination == combination)
        .count();
    let directions: Vec<String> = kinds(cases.values().flatten())
        .iter()
        .map(|(&kind, (releases, cases))| {
            format!(
                "{} {}/{samples}",
                direction(run, kind, releases),
                cases.len()
            )
        })
        .collect();
    let message = cases
        .values()
        .flatten()
        .next()
        .map(|failure| truncate(&failure.message, 100))
        .unwrap_or_default();
    let _ = write!(
        out,
        "<details class=\"combination\" id=\"{prefix}{combination}\"><summary><span class=\"cross\">✗</span> <b>{}</b> · {}<span class=\"directions\">{}</span><span class=\"message\">{}</span></summary>\n<div class=\"body\">\n",
        escape(&codec),
        escape(data_type),
        escape(&directions.join(", ")),
        escape(&message)
    );
    for (&case_index, failures) in cases {
        case_details(
            out,
            run,
            case_index,
            case_index - first_case,
            samples,
            failures,
        );
    }
    out.push_str("</div></details>\n");
}

/// The details of the failures of a case.
fn case_details(
    out: &mut String,
    run: &Run,
    case_index: usize,
    sample: usize,
    samples: usize,
    failures: &[&Failure],
) {
    let case = &run.cases[case_index];
    let data_type = &run.data_types[run.combinations[case.combination].data_type];
    let _ = write!(
        out,
        "<div class=\"case\">\n<div><b>Sample {}/{samples}</b> <span class=\"muted\">(case {case_index})</span> · shape {:?} · chunks {:?}{}</div>\n",
        sample + 1,
        case.shape,
        case.chunk_shape,
        if case.lossy {
            " · lossy (compared with the writer's own decoding)"
        } else {
            ""
        }
    );

    // (kind, message) -> releases
    let mut grouped: BTreeMap<(FailureKind, &str), BTreeSet<Release>> = BTreeMap::new();
    for failure in failures {
        grouped
            .entry((failure.kind, &failure.message))
            .or_default()
            .extend(failure.release);
    }
    out.push_str(
        "<table class=\"list\"><tr><th>direction</th><th>error</th><th>data written by</th></tr>\n",
    );
    for ((kind, message), releases) in grouped {
        let writers: Vec<String> = if kind == FailureKind::ReleaseToCurrent {
            releases.iter().map(ToString::to_string).collect()
        } else {
            vec!["current".to_string()]
        };
        let links: Vec<String> = writers
            .iter()
            .map(|writer| {
                let path = case_dir(run.work_dir, writer, case_index);
                format!(
                    "<a href=\"{}\" title=\"{}\">{writer}</a>",
                    escape(&file_url(&path)),
                    escape(&path.display().to_string())
                )
            })
            .collect();
        let _ = writeln!(
            out,
            "<tr><td>{}</td><td class=\"error\">{}</td><td>{}</td></tr>",
            escape(&direction(run, kind, &releases)),
            escape(message),
            links.join(" ")
        );
    }
    out.push_str("</table>\n");

    let codecs = case.metadata["codecs"].to_string();
    let mut metadata = String::new();
    format_json(&case.metadata, 0, &mut metadata);
    let _ = write!(
        out,
        "<div>codecs <code>{}</code></div>\n<details><summary>input data</summary><pre>{}</pre></details>\n<details><summary>array metadata</summary><pre>{}</pre></details>\n</div>\n",
        escape(&codecs),
        escape(&format_data(&case.data, &case.shape, data_type)),
        escape(&metadata)
    );
}

/// How far back data compatibility extends for each combination.
fn compatibility(out: &mut String, run: &Run) {
    let (rows, unsupported) = run.compatibility_rows();
    out.push_str("<details class=\"section\"><summary><h2>Compatibility with previous releases</h2></summary>\n<p class=\"muted\">current→release: the oldest release that reads data written by current zarrs (as do all newer releases).<br>release→current: the oldest release whose data current zarrs reads (as for all newer releases).</p>\n<table class=\"list\"><tr><th>current→release</th><th>release→current</th><th>combinations</th></tr>\n");
    for row in rows {
        let span = row.combinations.len();
        for (index, (codecs, data_types)) in row.combinations.into_iter().enumerate() {
            out.push_str("<tr>");
            if index == 0 {
                let _ = write!(
                    out,
                    "<td rowspan=\"{span}\">{}</td><td rowspan=\"{span}\">{}</td>",
                    escape(&row.forward),
                    escape(&row.backward)
                );
            }
            let _ = writeln!(
                out,
                "<td><b>{}</b>: {}</td></tr>",
                escape(&codecs),
                escape(&data_types)
            );
        }
    }
    out.push_str("</table>\n");
    if unsupported > 0 {
        let _ = writeln!(
            out,
            "<p class=\"muted\">{unsupported} combinations are not supported by current zarrs or any tested release.</p>"
        );
    }
    out.push_str("</details>\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cases;
    use crate::run::{CaseResult, ReleaseResult, Status};

    #[test]
    fn report_links_and_escapes_failures() {
        let data_types = cases::data_types();
        let codecs: Vec<_> = cases::codec_kinds()
            .into_iter()
            .filter(|codec| ["bytes", "gzip"].contains(&codec.to_string().as_str()))
            .collect();
        let combinations = cases::combinations(&codecs, &data_types);
        let cases = cases::sample_cases(&combinations, &data_types, 2, 0).unwrap();
        let ok = || ReleaseResult {
            forward: Status::Ok,
            backward: Status::Ok,
            roundtrip: Status::Ok,
        };
        let mut results: Vec<CaseResult> = cases
            .iter()
            .map(|_| CaseResult {
                current_write_error: None,
                current_roundtrip: Status::Ok,
                releases: vec![ok()],
            })
            .collect();
        let failing = 3;
        results[failing].releases[0].forward = Status::Fail("<oops> & \"stuff\"".to_string());
        let releases = [Release(23)];
        let run = Run {
            data_types: &data_types,
            combinations: &combinations,
            cases: &cases,
            results: &results,
            releases: &releases,
            work_dir: Path::new("/work dir"),
        };
        let (failures, known_issues) = run.failures();
        let meta = Meta {
            seed: 0,
            samples: 2,
            all: false,
            filter: None,
        };
        let html = html(&run, &failures, &known_issues, &meta);

        let combination = cases[failing].combination;
        assert!(html.contains(&format!("id=\"r{combination}\"")));
        assert!(html.contains("<td class=\"regression\""));
        assert!(html.contains(&format!("href=\"#r{combination}\"")));
        assert!(html.contains("&lt;oops&gt; &amp; &quot;stuff&quot;"));
        assert!(!html.contains("<oops>"));
        assert!(html.contains("current→0.23 1/2"));
        assert!(html.contains("file:///work%20dir/current/3"));
        assert!(html.contains("✗ 1 failing cases"));
    }

    #[test]
    fn json_short_inline() {
        let value = serde_json::json!({
            "shape": [4, 3],
            "codecs": [{"name": "bytes", "configuration": {"endian": "little"}}, {"name": "crc32c"}, {"name": "gzip", "configuration": {"level": 5}}],
        });
        let mut out = String::new();
        format_json(&value, 0, &mut out);
        assert_eq!(
            out,
            r#"{
  "shape": [4,3],
  "codecs": [
    {"name":"bytes","configuration":{"endian":"little"}},
    {"name":"crc32c"},
    {"name":"gzip","configuration":{"level":5}}
  ]
}"#
        );
    }

    #[test]
    fn data_grid() {
        let data_types = cases::data_types();
        let data_type = |label: &str| {
            data_types
                .iter()
                .find(|data_type| data_type.label == label)
                .unwrap()
        };
        let elements = |label: &str, elements: &[&[u8]], masks: Vec<Vec<u8>>| {
            let variable = data_type(label).values.element_size().is_none();
            let elements: Vec<Vec<u8>> = elements.iter().map(|element| element.to_vec()).collect();
            let data = Data::from_elements(&elements, variable, masks);
            let shape = [1, elements.len() as u64];
            let formatted = format_data(&data, &shape, data_type(label));
            formatted.lines().nth(1).unwrap().to_string()
        };
        let int16 = |value: i16| value.to_ne_bytes();
        assert_eq!(
            elements("int16", &[&int16(-2), &int16(300)], vec![]),
            " -2 300"
        );
        assert_eq!(elements("int4", &[&[0xf9]], vec![]), "-7");
        assert_eq!(elements("bool", &[&[0], &[1]], vec![]), "false  true");
        let float32 = |value: f32| value.to_ne_bytes();
        assert_eq!(
            elements("float32", &[&float32(1.5), &float32(f32::NAN)], vec![]),
            "1.5 NaN"
        );
        assert_eq!(
            elements("float16", &[&[0x00, 0x3c], &[0x00, 0xfc]], vec![]),
            "      1.0 -Infinity"
        );
        assert_eq!(elements("float8_e4m3", &[&[0x38]], vec![]), "1.0");
        assert_eq!(
            elements(
                "complex64",
                &[&[float32(1.5), float32(-2.0)].concat()],
                vec![]
            ),
            "1.5-2.0j"
        );
        assert_eq!(
            elements(
                "numpy.datetime64",
                &[&i64::MIN.to_ne_bytes(), &7_i64.to_ne_bytes()],
                vec![]
            ),
            "NaT   7"
        );
        assert_eq!(elements("r24", &[&[1, 2, 255]], vec![]), "0102ff");
        assert_eq!(elements("bytes", &[b"", b"ab"], vec![]), "   ∅ 6162");
        assert_eq!(
            elements("string", &[b"a\"b", b""], vec![]),
            "\"a\\\"b\"     \"\""
        );
        assert_eq!(
            elements("string", &["a\u{202e}".as_bytes()], vec![]),
            "\"a\\u{202e}\""
        );
        let utf32: Vec<u8> = "ab\0"
            .chars()
            .flat_map(|char| u32::from(char).to_ne_bytes())
            .collect();
        assert_eq!(elements("fixed_length_utf32", &[&utf32], vec![]), "\"ab\"");
        assert_eq!(
            elements(
                "optional<optional<float32>>",
                &[&float32(0.0), &float32(0.0), &float32(2.5)],
                vec![vec![0, 1, 1], vec![1, 0, 1]],
            ),
            "  null [null]    2.5"
        );

        let data = Data::from_elements(&[int16(1).to_vec(), int16(-1).to_vec()], false, vec![]);
        assert_eq!(
            format_data(&data, &[2, 1], data_type("int16")),
            "2 elements (int16)\n 1\n-1\n\n4 bytes (hex)\n0100\nffff"
        );
    }
}
