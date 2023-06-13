use std::fs::File;
use std::io::{Error, Write};
use std::iter;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use is_terminal::IsTerminal;
use prettytable::{Cell, Row, Table};
use semver::VersionReq;

use fw_lib::Firmware;
use ipk_lib::Package;
use verify_lib::bin::BinVerifyResult;
use verify_lib::ipk::{ComponentBinVerifyResult, ComponentVerifyResult, PackageVerifyResult};
use verify_lib::{VerifyResult, VerifyWithFirmware};

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
    #[arg(long)]
    fw_releases: Option<VersionReq>,
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
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
    for package in args.packages {
        let package = match Package::open(&package) {
            Ok(package) => package,
            Err(e) => {
                eprintln!(
                    "Failed to open {}: {e}",
                    package.file_name().unwrap().to_string_lossy()
                );
                continue;
            }
        };
        if to_file {
            eprintln!("Verifying package {}...", package.id);
        }
        output.h2(&format!("Package {}", package.id)).unwrap();
        let results: Vec<(&Firmware, PackageVerifyResult)> = firmwares
            .iter()
            .map(|fw| {
                let verify = package.verify(&fw);
                return (fw, verify);
            })
            .collect();
        let (_, result) = results.first().unwrap();
        if to_file {
            eprintln!(" - App {}", result.app.id);
        }
        output.h3(&format!("App {}", result.app.id)).unwrap();
        print_component_summary(
            results.iter().map(|(fw, res)| (*fw, &res.app)).collect(),
            &mut output,
            &format,
        )
        .unwrap();
        print_component_details(
            results.iter().map(|(fw, res)| (*fw, &res.app)).collect(),
            &mut output,
        )
        .unwrap();
        for idx in 0..result.services.len() {
            if to_file {
                eprintln!(" - Service {}", result.services.get(idx).unwrap().id);
            }
            output
                .h3(&format!("Service {}", result.services.get(idx).unwrap().id))
                .unwrap();
            print_component_summary(
                results
                    .iter()
                    .map(|(fw, res)| (*fw, res.services.get(idx).unwrap()))
                    .collect(),
                &mut output,
                &format,
            )
            .unwrap();
            print_component_details(
                results
                    .iter()
                    .map(|(fw, res)| (*fw, res.services.get(idx).unwrap()))
                    .collect(),
                &mut output,
            )
            .unwrap();
        }
    }
}

fn print_component_summary(
    results: Vec<(&Firmware, &ComponentVerifyResult)>,
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    let (_, result) = *results.first().unwrap();
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
) -> Result<(), Error> {
    let (_, result) = *results.first().unwrap();
    if results.iter().all(|r| r.1.is_good()) {
        out.write_fmt(format_args!("All OK\n"))?;
        return Ok(());
    }
    out.h4(result.exe.name())?;
    for (fw, result) in &results {
        if let ComponentBinVerifyResult::Failed(result) = &result.exe {
            out.h5(&format!("On {}", fw.info))?;
            print_bin_verify_details(result, out)?;
            out.write_fmt(format_args!("\n"))?;
        }
    }
    for (index, (required, lib)) in result.libs.iter().enumerate() {
        if !required {
            continue;
        }
        if results.iter().all(|(fw, result)| {
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
                print_bin_verify_details(result, out)?;
                out.write_fmt(format_args!("\n"))?;
            }
        }
    }
    return Ok(());
}

fn print_bin_verify_details(
    result: &BinVerifyResult,
    out: &mut Box<dyn ReportOutput>,
) -> Result<(), Error> {
    for lib in &result.missing_lib {
        out.write_fmt(format_args!("* Library {lib} is missing\n"))?;
    }
    for sym in &result.undefined_sym {
        out.write_fmt(format_args!("* Symbol {sym} is undefined\n"))?;
    }
    return Ok(());
}
