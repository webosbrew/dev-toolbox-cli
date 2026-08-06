use std::fs::File;
use std::io::{Error, Write};
use std::iter;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use is_terminal::IsTerminal;
use prettytable::{Cell, Row, Table};
use semver::VersionReq;

use cli_lib::{ExitCode, file_label};
use fw_lib::Firmware;
use ipk_lib::Package;
use verify_lib::VerifyResult;
use verify_lib::bin::BinVerifyResult;
use verify_lib::ipk::{
    CompatVerdict, ComponentBinVerifyResult, ComponentVerifyResult, DetectionResult,
    PackageVerifyResult, VerifyForFirmware,
};
use webdetect_lib::{ServiceRuntimeDetection, WebAppDetection};

use crate::output::ReportOutput;

mod output;

#[derive(Parser, Debug)]
struct Args {
    #[arg(required = true, help = "Packages to verify")]
    packages: Vec<PathBuf>,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(short, long, value_enum)]
    format: Option<OutputFormat>,
    #[arg(short, long, required_if_eq("no_summary", "true"))]
    details: bool,
    #[arg(short = 'S', long)]
    no_summary: bool,
    #[arg(short = 'r', long)]
    fw_releases: Option<VersionReq>,
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[derive(Debug, Clone, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Markdown,
    Terminal,
    Plain,
}

impl Args {
    fn report_output(&self) -> Box<dyn ReportOutput> {
        let Some(path) = &self.output else {
            return Box::new(std::io::stdout());
        };
        return match File::create(path) {
            Ok(file) => Box::new(file),
            Err(e) => {
                eprintln!("Failed to create {}: {e}", path.to_string_lossy());
                ExitCode::OutputError.exit();
            }
        };
    }
}

fn main() {
    let args = Args::parse();
    let mut output = args.report_output();
    let format = if let Some(format) = args.format.clone() {
        format
    } else if std::io::stdout().is_terminal() {
        OutputFormat::Terminal
    } else {
        OutputFormat::Plain
    };
    let firmwares: Vec<Firmware> = match Firmware::list(Firmware::data_path()) {
        Ok(firmwares) => firmwares
            .into_iter()
            .filter(|fw| {
                if let Some(fw_releases) = &args.fw_releases {
                    return fw_releases.matches(&fw.info.release);
                }
                return true;
            })
            .collect(),
        Err(e) => {
            eprintln!("Failed to read firmware data: {e}");
            ExitCode::NoFirmware.exit();
        }
    };
    if firmwares.is_empty() {
        eprintln!("No firmware found");
        ExitCode::NoFirmware.exit();
    }
    let mut all_good = true;
    let mut bad_input = false;
    for package in &args.packages {
        eprintln!("Opening package {}...", package.to_string_lossy());
        let package = match Package::open(package) {
            Ok(package) => package,
            Err(e) => {
                eprintln!("Failed to open {}: {e}", file_label(package));
                bad_input = true;
                continue;
            }
        };
        eprintln!("Verifying package {}...", package.id);
        let results: Vec<(&Firmware, PackageVerifyResult)> = firmwares
            .iter()
            .map(|fw| {
                let verify = package.verify_for_firmware(
                    &|name| fw.find_library(name),
                    fw.node_version().as_ref(),
                    fw.web_engine().as_ref(),
                );
                return (fw, verify);
            })
            .collect();
        if all_good && !results.iter().all(|(_, r)| r.is_good()) {
            all_good = false;
        }
        if let Err(e) = print_package_report(&package, &results, &args, &mut output, &format) {
            eprintln!("Failed to write the report: {e}");
            ExitCode::OutputError.exit();
        }
    }
    // A package the tool could not read outranks an incompatibility: the run did
    // not answer the question that was asked.
    if bad_input {
        ExitCode::BadInput.exit();
    }
    if !all_good {
        ExitCode::Incompatible.exit();
    }
}

/// Write the report for one package: the app, then each of its services.
fn print_package_report(
    package: &Package,
    results: &[(&Firmware, PackageVerifyResult)],
    args: &Args,
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    let to_file = args.output.is_some();
    out.h2(&format!("Package {}", package.id))?;
    print_packager_warning(package.hand_rolled, out, out_fmt)?;
    print_install_hooks(&package.install_hooks, out, out_fmt)?;
    let (_, result) = results.first().unwrap();
    if to_file {
        eprintln!(" - App {}", result.app.id);
    }
    out.h3(&format!("App {}", result.app.id))?;
    let app: Vec<_> = results.iter().map(|(fw, res)| (*fw, &res.app)).collect();
    if !args.no_summary {
        print_component_summary(&app, out, out_fmt)?;
    }
    if args.details {
        print_component_details(&app, out, out_fmt)?;
    }
    for idx in 0..result.services.len() {
        if to_file {
            eprintln!(" - Service {}", result.services.get(idx).unwrap().id);
        }
        out.h3(&format!("Service {}", result.services.get(idx).unwrap().id))?;
        let service: Vec<_> = results
            .iter()
            .map(|(fw, res)| (*fw, res.services.get(idx).unwrap()))
            .collect();
        if !args.no_summary {
            print_component_summary(&service, out, out_fmt)?;
        }
        if args.details {
            print_component_details(&service, out, out_fmt)?;
        }
    }
    return Ok(());
}

/// Warn when the package was not built by a webOS packager. Which control
/// fields gave it away is of no use to the author — say what to do instead.
fn print_packager_warning(
    hand_rolled: bool,
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    if !hand_rolled {
        return Ok(());
    }
    let mark = if *out_fmt == OutputFormat::Markdown {
        ":warning:"
    } else {
        "Warning:"
    };
    out.write_fmt(format_args!(
        "{mark} This package looks hand-rolled. Please build it with `ares-package`.\n\n"
    ))?;
    return Ok(());
}

/// Warn about the maintainer scripts a package carries. The webOS installer
/// runs none of them, so a package that puts its setup in one installs and does
/// nothing it meant to do. A warning only — the package still installs, and the
/// tool cannot tell what the script was for.
fn print_install_hooks(
    hooks: &[String],
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    if hooks.is_empty() {
        return Ok(());
    }
    let mark = if *out_fmt == OutputFormat::Markdown {
        ":warning:"
    } else {
        "Warning:"
    };
    let what = if hooks.len() == 1 {
        format!("an install hook ({}). webOS does not run it", hooks[0])
    } else {
        format!(
            "install hooks ({}). webOS runs none of them",
            hooks.join(", ")
        )
    };
    out.write_fmt(format_args!("{mark} This package carries {what}.\n\n"))?;
    return Ok(());
}

fn print_component_summary(
    results: &[(&Firmware, &ComponentVerifyResult)],
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    let (_, result) = *results.first().unwrap();
    if result.detection.is_some() {
        return print_detection_summary(results, out, out_fmt);
    }
    if let ComponentBinVerifyResult::Skipped { .. } = &result.exe {
        out.write_fmt(format_args!("Skip because this component is not native\n"))?;
        return Ok(());
    }
    let mut table = Table::new();
    table.set_format(out.table_format(out_fmt));
    table.set_titles(
        iter::once(String::new())
            .chain(
                results
                    .iter()
                    .map(|(firmware, _result)| firmware.info.release.to_string()),
            )
            .collect(),
    );
    table.add_row(Row::new(
        iter::once(Cell::new(result.exe.name()))
            .chain(
                results
                    .iter()
                    .map(|(_, result)| out.result_cell(&result.exe, out_fmt)),
            )
            .collect(),
    ));
    for (idx, (required, lib)) in result.libs.iter().enumerate() {
        let name = if *required {
            Cell::new(&format!("required lib {}", lib.name()))
        } else {
            Cell::new(&format!("lib {}", lib.name()))
        };
        table.add_row(Row::new(
            iter::once(name)
                .chain(
                    results.iter().map(|(_, result)| {
                        out.result_cell(&result.libs.get(idx).unwrap().1, out_fmt)
                    }),
                )
                .collect(),
        ));
    }
    out.print_table(&table)?;
    return Ok(());
}

fn print_component_details(
    results: &[(&Firmware, &ComponentVerifyResult)],
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<bool, Error> {
    let (_, result) = *results.first().unwrap();
    if result.detection.is_some() {
        print_detection_details(results, out, out_fmt)?;
        return Ok(results.iter().all(|r| r.1.is_good()));
    }
    out.h4(result.exe.name())?;
    if results.iter().all(|r| r.1.is_good()) && !results.iter().any(|r| has_notes(&r.1.exe)) {
        out.write_fmt(format_args!("All OK\n"))?;
        return Ok(true);
    }
    for (fw, result) in results {
        if let Some(bin) = notes(&result.exe) {
            out.h5(&format!("On {}", fw.info))?;
            print_bin_verify_details(bin, out, out_fmt)?;
            out.write_fmt(format_args!("\n"))?;
        }
    }
    for (index, (required, lib)) in result.libs.iter().enumerate() {
        if !required {
            continue;
        }
        if !results
            .iter()
            .any(|(_, result)| has_notes(&result.libs.get(index).unwrap().1))
        {
            continue;
        }
        out.h4(lib.name())?;
        for (fw, result) in results {
            if let Some(bin) = notes(&result.libs.get(index).unwrap().1) {
                out.h5(&format!("On {}", fw.info))?;
                print_bin_verify_details(bin, out, out_fmt)?;
                out.write_fmt(format_args!("\n"))?;
            }
        }
    }
    return Ok(false);
}

/// A long list of undefined symbols is folded into a collapsible `<details>`
/// block rather than emitted inline.
const SYMBOL_FOLD_THRESHOLD: usize = 10;

fn print_bin_verify_details(
    result: &BinVerifyResult,
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    for lib in &result.missing_lib {
        out.write_fmt(format_args!("* Library {lib} is missing\n"))?;
    }
    print_symbol_list(&result.undefined_sym, "", "undefined symbols", out, out_fmt)?;
    // A lazily-bound import is a hint, not a failure: the loader resolves it on
    // the first call, so the binary still starts.
    print_symbol_list(
        &result.undefined_sym_lazy,
        " (bound lazily)",
        "undefined symbols bound lazily — the binary loads, a call to one aborts it",
        out,
        out_fmt,
    )?;
    return Ok(());
}

/// Write one bullet per symbol. `suffix` marks the kind on every line, `summary`
/// labels the fold.
fn print_symbol_list(
    symbols: &[String],
    suffix: &str,
    summary: &str,
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    if symbols.is_empty() {
        return Ok(());
    }
    // Collapse a long symbol list behind a <details>/<summary> so the report
    // stays scannable. GitHub renders raw HTML in Markdown; other formats keep
    // the plain bullet list (raw tags would be noise there).
    let fold = *out_fmt == OutputFormat::Markdown && symbols.len() > SYMBOL_FOLD_THRESHOLD;
    if fold {
        out.write_fmt(format_args!(
            "<details>\n<summary>{} {summary}</summary>\n\n",
            symbols.len()
        ))?;
    }
    for sym in symbols {
        out.write_fmt(format_args!("* Symbol {sym} is undefined{suffix}\n"))?;
    }
    if fold {
        out.write_fmt(format_args!("</details>\n"))?;
    }
    return Ok(());
}

/// The detail body of a result worth reporting: a failure or a warning.
fn notes(result: &ComponentBinVerifyResult) -> Option<&BinVerifyResult> {
    return match result {
        ComponentBinVerifyResult::Failed(bin) | ComponentBinVerifyResult::Warned(bin) => Some(bin),
        _ => None,
    };
}

fn has_notes(result: &ComponentBinVerifyResult) -> bool {
    return notes(result).is_some();
}

/// Render the summary for a non-native component: the firmware-independent
/// detected technology as a text line, then a per-firmware compatibility table.
fn print_detection_summary(
    results: &[(&Firmware, &ComponentVerifyResult)],
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    let (_, first) = results.first().unwrap();
    let detection = first.detection.as_ref().unwrap();

    let mut table = Table::new();
    table.set_format(out.table_format(out_fmt));
    table.set_titles(
        iter::once(String::new())
            .chain(results.iter().map(|(fw, _)| fw.info.release.to_string()))
            .collect(),
    );

    match detection {
        DetectionResult::WebApp { detection: web, .. } => {
            out.write_fmt(format_args!(
                "Web app — {}\n\n",
                join_notes(describe_web(web), describe_bundled(detection.bundled()))
            ))?;
            // Each firmware's web engine.
            table.add_row(Row::new(
                iter::once(Cell::new("Web engine"))
                    .chain(results.iter().map(|(_, r)| Cell::new(&web_engine_label(r))))
                    .collect(),
            ));
            // Whether that engine supports the app's ES syntax level (gating).
            let title = es_support_title(web.es_level);
            table.add_row(Row::new(
                iter::once(Cell::new(&title))
                    .chain(
                        results
                            .iter()
                            .map(|(_, r)| out.verdict_cell(component_verdict(r), out_fmt)),
                    )
                    .collect(),
            ));
            // Advisory: runtime APIs the app uses (may need polyfills). Suppressed
            // when the app bundles its own polyfills (the row would be moot).
            if !web.es_apis.is_empty() && web.polyfills.is_empty() {
                table.add_row(Row::new(
                    iter::once(Cell::new("Runtime APIs"))
                        .chain(
                            results
                                .iter()
                                .map(|(_, r)| out.advisory_cell(api_advisory(r), out_fmt)),
                        )
                        .collect(),
                ));
            }
        }
        DetectionResult::Service { detection: svc, .. } => {
            out.write_fmt(format_args!(
                "JS service — {}\n\n",
                join_notes(describe_service(svc), describe_bundled(detection.bundled()))
            ))?;
            // Each firmware's Node.js version.
            table.add_row(Row::new(
                iter::once(Cell::new("Node.js (firmware)"))
                    .chain(results.iter().map(|(_, r)| Cell::new(&node_label(r))))
                    .collect(),
            ));
            // Whether that Node.js supports the service's ES syntax level
            // (gating; derived from code, not engines.node).
            if svc.es_level.is_some() {
                let title = es_support_title(svc.es_level);
                table.add_row(Row::new(
                    iter::once(Cell::new(&title))
                        .chain(
                            results
                                .iter()
                                .map(|(_, r)| out.verdict_cell(component_verdict(r), out_fmt)),
                        )
                        .collect(),
                ));
            }
            if !svc.es_apis.is_empty() && svc.polyfills.is_empty() {
                table.add_row(Row::new(
                    iter::once(Cell::new("Runtime APIs"))
                        .chain(
                            results
                                .iter()
                                .map(|(_, r)| out.advisory_cell(api_advisory(r), out_fmt)),
                        )
                        .collect(),
                ));
            }
        }
    }
    out.print_table(&table)?;
    return Ok(());
}

fn es_support_title(level: Option<webdetect_lib::EsLevel>) -> String {
    match level {
        Some(level) => format!("{} support", level.label()),
        None => "ES support".to_string(),
    }
}

/// Render `--details` for a non-native component: syntax-feature evidence,
/// then any firmware on which it is incompatible and why.
fn print_detection_details(
    results: &[(&Firmware, &ComponentVerifyResult)],
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    let (_, first) = results.first().unwrap();
    let detection = first.detection.as_ref().unwrap();
    match detection {
        DetectionResult::WebApp {
            detection: web,
            bundled,
            ..
        } => {
            out.h4("Web app")?;
            if let Some(fw) = &web.framework {
                out.write_fmt(format_args!("* Framework: {}\n", framework_label(fw)))?;
            }
            for other in &web.also_present {
                out.write_fmt(format_args!("* Also present: {}\n", framework_label(other)))?;
            }
            if !web.es_features.is_empty() {
                let feats: Vec<&str> = web.es_features.iter().map(|f| f.label()).collect();
                out.write_fmt(format_args!(
                    "* Language features used: {}\n",
                    feats.join(", ")
                ))?;
            }
            print_api_details(&web.es_apis, &web.polyfills, out)?;
            for url in &web.remote_resources {
                out.write_fmt(format_args!("* Remote resource: {url}\n"))?;
            }
            print_bundled_artifacts(bundled, out, out_fmt)?;
            print_bundled_compat(results, out, out_fmt)?;
        }
        DetectionResult::Service {
            detection: svc,
            bundled,
            ..
        } => {
            out.h4("JS service")?;
            if !svc.es_features.is_empty() {
                let feats: Vec<&str> = svc.es_features.iter().map(|f| f.label()).collect();
                out.write_fmt(format_args!(
                    "* Language features used: {}\n",
                    feats.join(", ")
                ))?;
            }
            print_api_details(&svc.es_apis, &svc.polyfills, out)?;
            print_bundled_artifacts(bundled, out, out_fmt)?;
            print_bundled_compat(results, out, out_fmt)?;
        }
    }
    // Report incompatible firmwares with their reason (gating verdicts only).
    let mut any_fail = false;
    for (fw, r) in results {
        if let Some(detection) = &r.detection {
            if let CompatVerdict::Fail { reason } = detection.verdict() {
                if !any_fail {
                    out.h5("Incompatible on")?;
                    any_fail = true;
                }
                out.write_fmt(format_args!("* {}: {reason}\n", fw.info))?;
            }
        }
    }
    out.write_fmt(format_args!("\n"))?;
    return Ok(());
}

/// List the detected runtime APIs (advisory). When the component bundles its
/// own polyfills, note that instead — the API list would be moot.
fn print_api_details(
    apis: &[webdetect_lib::ApiUse],
    polyfills: &[String],
    out: &mut Box<dyn ReportOutput>,
) -> Result<(), Error> {
    if !polyfills.is_empty() {
        out.write_fmt(format_args!(
            "* Bundles polyfills ({}) — runtime APIs are self-provided\n",
            polyfills.join(", ")
        ))?;
        return Ok(());
    }
    if apis.is_empty() {
        return Ok(());
    }
    let names: Vec<&str> = apis.iter().map(|a| a.name.as_str()).collect();
    if let Some(level) = apis.iter().map(|a| a.level).max() {
        out.write_fmt(format_args!(
            "* Runtime APIs (up to {}, may need polyfills): {}\n",
            level.label(),
            names.join(", ")
        ))?;
    }
    return Ok(());
}

/// Summarize a component's bundled native binaries for the one-line
/// description: how many, and which architectures. `None` when it bundles none.
///
/// A package may ship binaries for more than one architecture and pick at run
/// time — `ares-package` labels every package `all` regardless — so several
/// architectures are normal, not a fault. Each gets its own count.
fn describe_bundled(bundled: &[bin_lib::BundledArtifact]) -> Option<String> {
    if bundled.is_empty() {
        return None;
    }
    // Count per architecture, in first-seen (path) order for a stable line.
    let mut arches: Vec<(&str, usize)> = Vec::new();
    for arch in bundled.iter().filter_map(|a| a.arch.as_deref()) {
        match arches.iter_mut().find(|(name, _)| *name == arch) {
            Some((_, count)) => *count += 1,
            None => arches.push((arch, 1)),
        }
    }
    let total = format!(
        "bundles {} native binar{}",
        bundled.len(),
        if bundled.len() == 1 { "y" } else { "ies" }
    );
    if arches.is_empty() {
        return Some(total);
    }
    // One architecture that accounts for every binary needs no count.
    let labels: Vec<String> = if arches.len() == 1 && arches[0].1 == bundled.len() {
        vec![arches[0].0.to_string()]
    } else {
        arches
            .iter()
            .map(|(name, count)| format!("{count} {name}"))
            .collect()
    };
    return Some(format!("{total} ({})", labels.join(", ")));
}

/// Append an optional note to a description, keeping the `; ` separator the
/// description itself uses.
fn join_notes(description: String, note: Option<String>) -> String {
    match note {
        Some(note) => format!("{description}; {note}"),
        None => description,
    }
}

/// List the native binaries a component bundles (a service's own node/ffmpeg,
/// or a web app's payload). Supplementary info — never affects the verdict. On
/// Markdown it is folded into a `<details>` block so the report stays scannable;
/// other formats get a plain bulleted list.
fn print_bundled_artifacts(
    bundled: &[bin_lib::BundledArtifact],
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    if bundled.is_empty() {
        return Ok(());
    }
    let fold = *out_fmt == OutputFormat::Markdown;
    if fold {
        out.write_fmt(format_args!(
            "<details>\n<summary>Bundles {} native binar{}</summary>\n\n",
            bundled.len(),
            if bundled.len() == 1 { "y" } else { "ies" },
        ))?;
    } else {
        out.write_fmt(format_args!("Bundled native binaries:\n"))?;
    }
    for a in bundled {
        match &a.arch {
            Some(arch) => {
                out.write_fmt(format_args!(
                    "* {} — {}, {}\n",
                    a.path,
                    a.kind.label(),
                    arch
                ))?;
            }
            None => out.write_fmt(format_args!("* {} — {}\n", a.path, a.kind.label()))?,
        }
    }
    if fold {
        out.write_fmt(format_args!("</details>\n"))?;
    }
    return Ok(());
}

/// Verify report for a component's bundled native binaries — a single
/// native-style table whose rows are the bundled executables followed by the
/// unique bundled libraries, each OK/failed per firmware, then the reason
/// behind every cell that is not OK. Folded into a `<details>` block on
/// Markdown. Supplementary: none of this changes the package verdict.
fn print_bundled_compat(
    results: &[(&Firmware, &ComponentVerifyResult)],
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    let (_, first) = results.first().unwrap();
    if first.bundled.is_empty() {
        return Ok(());
    }
    let fold = *out_fmt == OutputFormat::Markdown;
    if fold {
        out.write_fmt(format_args!(
            "<details>\n<summary>Bundled runtime — library compatibility</summary>\n\n"
        ))?;
    } else {
        out.write_fmt(format_args!("Bundled runtime — library compatibility:\n\n"))?;
    }

    let mut table = Table::new();
    table.set_format(out.table_format(out_fmt));
    table.set_titles(
        iter::once(String::new())
            .chain(results.iter().map(|(fw, _)| fw.info.release.to_string()))
            .collect(),
    );

    // Bundled executables (its own node/ffmpeg/...), one row each. The row is
    // named by path: a payload built for several architectures ships the same
    // file name more than once.
    for idx in 0..first.bundled.len() {
        let name = first.bundled[idx].id.clone();
        table.add_row(Row::new(
            iter::once(Cell::new(&name))
                .chain(
                    results
                        .iter()
                        .map(|(_, r)| out.result_cell(&r.bundled[idx].exe, out_fmt)),
                )
                .collect(),
        ));
    }

    let lib_names = bundled_lib_names(first);
    for lib_name in &lib_names {
        table.add_row(Row::new(
            iter::once(Cell::new(&format!("lib {lib_name}")))
                .chain(
                    results
                        .iter()
                        .map(|(_, r)| out.result_cell(worst_bundled_lib(r, lib_name), out_fmt)),
                )
                .collect(),
        ));
    }

    out.print_table(&table)?;
    print_bundled_reasons(results, &lib_names, out, out_fmt)?;
    if fold {
        out.write_fmt(format_args!("</details>\n"))?;
    }
    return Ok(());
}

/// The bundled libraries of a component, deduped by name (a versioned file and
/// its soname alias resolve to the same library) and sorted.
fn bundled_lib_names(result: &ComponentVerifyResult) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for comp in &result.bundled {
        for (_, lib) in &comp.libs {
            let name = lib.name().to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort();
    return names;
}

/// Explain every cell of the bundled table that is not OK: the missing
/// libraries and undefined symbols behind it, the same facts `elf-verify`
/// prints for a standalone binary. Rows and headings use the same names, so a
/// reader can match a cell to its reason. These are hints — a bundled binary
/// never gates the package verdict.
fn print_bundled_reasons(
    results: &[(&Firmware, &ComponentVerifyResult)],
    lib_names: &[String],
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    let (_, first) = results.first().unwrap();
    for idx in 0..first.bundled.len() {
        let name = first.bundled[idx].id.clone();
        print_bundled_bin_reasons(results, &name, |r| &r.bundled[idx].exe, out, out_fmt)?;
    }
    for lib_name in lib_names {
        let heading = format!("lib {lib_name}");
        print_bundled_bin_reasons(
            results,
            &heading,
            |r| worst_bundled_lib(r, lib_name),
            out,
            out_fmt,
        )?;
    }
    return Ok(());
}

/// One bundled row's reasons. `pick` selects that row's result out of a
/// firmware's component result. Writes nothing when every firmware is happy
/// with the row.
///
/// A bundled binary usually fails the same way on every firmware that lacks the
/// library, so firmwares that report the same thing share one block. Repeating
/// an identical list per column buries the finding.
fn print_bundled_bin_reasons<F>(
    results: &[(&Firmware, &ComponentVerifyResult)],
    heading: &str,
    pick: F,
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error>
where
    F: Fn(&ComponentVerifyResult) -> &ComponentBinVerifyResult,
{
    let mut groups: Vec<(&BinVerifyResult, Vec<String>)> = Vec::new();
    for (fw, result) in results {
        let Some(bin) = notes(pick(result)) else {
            continue;
        };
        let release = fw.info.release.to_string();
        match groups.iter_mut().find(|(seen, _)| *seen == bin) {
            Some((_, releases)) => releases.push(release),
            None => groups.push((bin, vec![release])),
        }
    }
    for (bin, releases) in groups {
        out.h5(&format!("{heading} on webOS {}", releases.join(", ")))?;
        print_bin_verify_details(bin, out, out_fmt)?;
        out.write_fmt(format_args!("\n"))?;
    }
    return Ok(());
}

/// The status of a bundled library across all of a component's bundled binaries:
/// `Failed` if it fails to resolve for any of them, otherwise the first result.
/// The same library file resolves identically for every binary on a given
/// firmware, so this only ever collapses duplicate rows.
fn worst_bundled_lib<'a>(
    result: &'a ComponentVerifyResult,
    name: &str,
) -> &'a ComponentBinVerifyResult {
    let mut found: Option<&ComponentBinVerifyResult> = None;
    let mut warned: Option<&ComponentBinVerifyResult> = None;
    for comp in &result.bundled {
        for (_, lib) in &comp.libs {
            if lib.name() == name {
                if matches!(lib, ComponentBinVerifyResult::Failed(_)) {
                    return lib;
                }
                if matches!(lib, ComponentBinVerifyResult::Warned(_)) {
                    warned.get_or_insert(lib);
                }
                found.get_or_insert(lib);
            }
        }
    }
    return warned
        .or(found)
        .expect("library is present in every firmware's bundled set");
}

fn framework_label(fw: &webdetect_lib::FrameworkInfo) -> String {
    match &fw.version {
        Some(v) => format!("{} {}", fw.kind.label(), v),
        None => fw.kind.label().to_string(),
    }
}

/// One-line description of the detected web app (framework, SDK, ES level).
fn describe_web(web: &WebAppDetection) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(fw) = &web.framework {
        parts.push(framework_label(fw));
    }
    for other in &web.also_present {
        parts.push(format!("+ {}", framework_label(other)));
    }
    if let Some(tv) = &web.webostvjs {
        parts.push(match tv {
            Some(v) => format!("webOSTV.js {v}"),
            None => "webOSTV.js".to_string(),
        });
    }
    if let Some(level) = web.es_level {
        parts.push(format!("requires {}", level.label()));
    }
    if !web.polyfills.is_empty() {
        parts.push(format!("bundles polyfills ({})", web.polyfills.join(", ")));
    }
    match web.remote_resources.len() {
        0 => {}
        1 => parts.push("loads 1 remote resource".to_string()),
        n => parts.push(format!("loads {n} remote resources")),
    }
    if parts.is_empty() {
        "no framework detected".to_string()
    } else {
        parts.join("; ")
    }
}

/// One-line description of the detected JS service: its ES language level
/// (checked against Node.js).
fn describe_service(svc: &ServiceRuntimeDetection) -> String {
    let mut parts: Vec<String> = vec!["Node.js service".to_string()];
    if let Some(level) = svc.es_level {
        parts.push(format!("requires {}", level.label()));
    }
    if !svc.polyfills.is_empty() {
        parts.push(format!("bundles polyfills ({})", svc.polyfills.join(", ")));
    }
    parts.join("; ")
}

/// The gating ES compatibility verdict for a component result (Unknown if
/// there is no detection).
fn component_verdict(result: &ComponentVerifyResult) -> &CompatVerdict {
    result.detection.as_ref().map_or(
        &CompatVerdict::Unknown,
        verify_lib::ipk::DetectionResult::verdict,
    )
}

/// The advisory runtime-API verdict for a component result.
fn api_advisory(result: &ComponentVerifyResult) -> &CompatVerdict {
    result.detection.as_ref().map_or(
        &CompatVerdict::Unknown,
        verify_lib::ipk::DetectionResult::api_advisory,
    )
}

fn web_engine_label(result: &ComponentVerifyResult) -> String {
    match &result.detection {
        Some(DetectionResult::WebApp {
            engine: Some(engine),
            ..
        }) => engine.label(),
        _ => "unknown".to_string(),
    }
}

fn node_label(result: &ComponentVerifyResult) -> String {
    match &result.detection {
        Some(DetectionResult::Service {
            available_node: Some(v),
            ..
        }) => format!("Node {v}"),
        _ => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::describe_bundled;
    use bin_lib::{ArtifactKind, BundledArtifact};

    fn artifact(path: &str, arch: &str) -> BundledArtifact {
        BundledArtifact {
            path: path.to_string(),
            kind: ArtifactKind::Executable,
            arch: Some(arch.to_string()),
        }
    }

    #[test]
    fn describes_no_bundled_binaries() {
        assert_eq!(describe_bundled(&[]), None);
    }

    #[test]
    fn describes_one_architecture_without_counts() {
        let bundled = [
            artifact("bin/wg", "AArch64 (64-bit)"),
            artifact("bin/wireguard-go", "AArch64 (64-bit)"),
        ];
        assert_eq!(
            describe_bundled(&bundled).unwrap(),
            "bundles 2 native binaries (AArch64 (64-bit))"
        );
    }

    #[test]
    fn describes_each_architecture_of_a_mixed_payload() {
        // Shipping one payload per architecture is normal: the app picks at run
        // time. Count them separately instead of calling it a fault.
        let bundled = [
            artifact("payload/arm/wg", "ARM (32-bit)"),
            artifact("payload/arm/wireguard-go", "ARM (32-bit)"),
            artifact("payload/arm64/wg", "AArch64 (64-bit)"),
        ];
        assert_eq!(
            describe_bundled(&bundled).unwrap(),
            "bundles 3 native binaries (2 ARM (32-bit), 1 AArch64 (64-bit))"
        );
    }
}
