use std::fs::File;
use std::io::{Error, Write};
use std::iter;
use std::path::PathBuf;
use std::process::exit;

use clap::{Parser, ValueEnum};
use is_terminal::IsTerminal;
use prettytable::{Cell, Row, Table};
use semver::VersionReq;

use fw_lib::Firmware;
use ipk_lib::Package;
use verify_lib::bin::BinVerifyResult;
use verify_lib::ipk::{
    ComponentBinVerifyResult, ComponentVerifyResult, CompatVerdict, DetectionResult,
    PackageVerifyResult, VerifyForFirmware,
};
use verify_lib::VerifyResult;
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
        return if let Some(path) = &self.output {
            Box::new(File::create(path).unwrap())
        } else {
            Box::new(std::io::stdout())
        };
    }
}

fn main() {
    let args = Args::parse();
    let to_file: bool = args.output.is_some();
    let mut output = args.report_output();
    let format = if let Some(format) = args.format {
        format
    } else if std::io::stdout().is_terminal() {
        OutputFormat::Terminal
    } else {
        OutputFormat::Plain
    };
    let firmwares: Vec<Firmware> = Firmware::list(Firmware::data_path())
        .unwrap()
        .into_iter()
        .filter(|fw| {
            if let Some(fw_releases) = &args.fw_releases {
                return fw_releases.matches(&fw.info.release);
            }
            return true;
        })
        .collect();
    if firmwares.is_empty() {
        eprintln!("No firmware found");
    }
    let mut all_good = true;
    for package in &args.packages {
        eprintln!("Opening package {}...", package.to_string_lossy());
        let package = match Package::open(&package) {
            Ok(package) => package,
            Err(e) => {
                eprintln!(
                    "Failed to open {}: {e}",
                    package.file_name().unwrap().to_string_lossy()
                );
                all_good = false;
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
        output.h2(&format!("Package {}", package.id)).unwrap();

        if all_good && !results.iter().all(|(_, r)| r.is_good()) {
            all_good = false;
        }
        let (_, result) = results.first().unwrap();
        if to_file {
            eprintln!(" - App {}", result.app.id);
        }
        output.h3(&format!("App {}", result.app.id)).unwrap();
        if !args.no_summary {
            print_component_summary(
                results.iter().map(|(fw, res)| (*fw, &res.app)).collect(),
                &mut output,
                &format,
            )
            .unwrap();
        }
        if args.details {
            print_component_details(
                results.iter().map(|(fw, res)| (*fw, &res.app)).collect(),
                &mut output,
                &format,
            )
            .unwrap();
        }
        for idx in 0..result.services.len() {
            if to_file {
                eprintln!(" - Service {}", result.services.get(idx).unwrap().id);
            }
            output
                .h3(&format!("Service {}", result.services.get(idx).unwrap().id))
                .unwrap();
            if !args.no_summary {
                print_component_summary(
                    results
                        .iter()
                        .map(|(fw, res)| (*fw, res.services.get(idx).unwrap()))
                        .collect(),
                    &mut output,
                    &format,
                )
                .unwrap();
            }
            if args.details {
                print_component_details(
                    results
                        .iter()
                        .map(|(fw, res)| (*fw, res.services.get(idx).unwrap()))
                        .collect(),
                    &mut output,
                    &format,
                )
                .unwrap();
            }
        }
    }
    if !all_good {
        exit(1);
    }
}

fn print_component_summary(
    results: Vec<(&Firmware, &ComponentVerifyResult)>,
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    let (_, result) = *results.first().unwrap();
    if result.detection.is_some() {
        return print_detection_summary(&results, out, out_fmt);
    }
    if let ComponentBinVerifyResult::Skipped { .. } = &result.exe {
        out.write_fmt(format_args!("Skip because this component is not native\n"))?;
        return Ok(());
    }
    let mut table = Table::new();
    table.set_format(out.table_format(out_fmt));
    table.set_titles(Row::from_iter(
        iter::once(String::new()).chain(
            results
                .iter()
                .map(|(firmware, _result)| firmware.info.release.to_string()),
        ),
    ));
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
    results: Vec<(&Firmware, &ComponentVerifyResult)>,
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<bool, Error> {
    let (_, result) = *results.first().unwrap();
    if result.detection.is_some() {
        print_detection_details(&results, out)?;
        return Ok(results.iter().all(|r| r.1.is_good()));
    }
    out.h4(result.exe.name())?;
    if results.iter().all(|r| r.1.is_good()) {
        out.write_fmt(format_args!("All OK\n"))?;
        return Ok(true);
    }
    for (fw, result) in &results {
        if let ComponentBinVerifyResult::Failed(result) = &result.exe {
            out.h5(&format!("On {}", fw.info))?;
            print_bin_verify_details(result, out, out_fmt)?;
            out.write_fmt(format_args!("\n"))?;
        }
    }
    for (index, (required, lib)) in result.libs.iter().enumerate() {
        if !required {
            continue;
        }
        if results.iter().all(|(_, result)| {
            if let ComponentBinVerifyResult::Failed(_) = result.libs.get(index).unwrap().1 {
                false
            } else {
                true
            }
        }) {
            continue;
        }
        out.h4(lib.name())?;
        for (fw, result) in &results {
            if let ComponentBinVerifyResult::Failed(result) = &result.libs.get(index).unwrap().1 {
                out.h5(&format!("On {}", fw.info))?;
                print_bin_verify_details(result, out, out_fmt)?;
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
    // Collapse a long symbol list behind a <details>/<summary> so the report
    // stays scannable. GitHub renders raw HTML in Markdown; other formats keep
    // the plain bullet list (raw tags would be noise there).
    let fold = *out_fmt == OutputFormat::Markdown
        && result.undefined_sym.len() > SYMBOL_FOLD_THRESHOLD;
    if fold {
        out.write_fmt(format_args!(
            "<details>\n<summary>{} undefined symbols</summary>\n\n",
            result.undefined_sym.len()
        ))?;
    }
    for sym in &result.undefined_sym {
        out.write_fmt(format_args!("* Symbol {sym} is undefined\n"))?;
    }
    if fold {
        out.write_fmt(format_args!("</details>\n"))?;
    }
    return Ok(());
}

/// Render the summary for a non-native component: the firmware-independent
/// detected technology as a text line, then a per-firmware compatibility table.
fn print_detection_summary(
    results: &Vec<(&Firmware, &ComponentVerifyResult)>,
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    let (_, first) = results.first().unwrap();
    let detection = first.detection.as_ref().unwrap();

    let mut table = Table::new();
    table.set_format(out.table_format(out_fmt));
    table.set_titles(Row::from_iter(iter::once(String::new()).chain(
        results.iter().map(|(fw, _)| fw.info.release.to_string()),
    )));

    match detection {
        DetectionResult::WebApp { detection: web, .. } => {
            out.write_fmt(format_args!("Web app — {}\n\n", describe_web(web)))?;
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
                        .chain(results.iter().map(|(_, r)| out.advisory_cell(api_advisory(r), out_fmt)))
                        .collect(),
                ));
            }
        }
        DetectionResult::Service { detection: svc, .. } => {
            out.write_fmt(format_args!("JS service — {}\n\n", describe_service(svc)))?;
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
                        .chain(results.iter().map(|(_, r)| out.advisory_cell(api_advisory(r), out_fmt)))
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
    results: &Vec<(&Firmware, &ComponentVerifyResult)>,
    out: &mut Box<dyn ReportOutput>,
) -> Result<(), Error> {
    let (_, first) = results.first().unwrap();
    let detection = first.detection.as_ref().unwrap();
    match detection {
        DetectionResult::WebApp { detection: web, .. } => {
            out.h4("Web app")?;
            if let Some(fw) = &web.framework {
                out.write_fmt(format_args!("* Framework: {}\n", framework_label(fw)))?;
            }
            for other in &web.also_present {
                out.write_fmt(format_args!("* Also present: {}\n", framework_label(other)))?;
            }
            if !web.es_features.is_empty() {
                let feats: Vec<&str> = web.es_features.iter().map(|f| f.label()).collect();
                out.write_fmt(format_args!("* Language features used: {}\n", feats.join(", ")))?;
            }
            print_api_details(&web.es_apis, &web.polyfills, out)?;
            for url in &web.remote_resources {
                out.write_fmt(format_args!("* Remote resource: {url}\n"))?;
            }
        }
        DetectionResult::Service { detection: svc, .. } => {
            out.h4("JS service")?;
            if !svc.es_features.is_empty() {
                let feats: Vec<&str> = svc.es_features.iter().map(|f| f.label()).collect();
                out.write_fmt(format_args!("* Language features used: {}\n", feats.join(", ")))?;
            }
            print_api_details(&svc.es_apis, &svc.polyfills, out)?;
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
    result
        .detection
        .as_ref()
        .map(|d| d.verdict())
        .unwrap_or(&CompatVerdict::Unknown)
}

/// The advisory runtime-API verdict for a component result.
fn api_advisory(result: &ComponentVerifyResult) -> &CompatVerdict {
    result
        .detection
        .as_ref()
        .map(|d| d.api_advisory())
        .unwrap_or(&CompatVerdict::Unknown)
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
