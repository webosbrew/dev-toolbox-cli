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
use verify_lib::ipk::{ComponentVerifyResult, PackageVerifyResult};
use verify_lib::VerifyWithFirmware;

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
        output
            .write_fmt(format_args!("## Package {}\n\n", package.id))
            .unwrap();
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
        output
            .write_fmt(format_args!("### App {}\n\n", result.app.id))
            .unwrap();
        print_component_results(
            results.iter().map(|(fw, res)| (*fw, &res.app)).collect(),
            &mut output,
            &format,
        )
        .unwrap();
        for idx in 0..result.services.len() {
            if to_file {
                eprintln!(" - Service {}", result.services.get(idx).unwrap().id);
            }
            output
                .write_fmt(format_args!(
                    "### Service {}",
                    result.services.get(idx).unwrap().id
                ))
                .unwrap();
            print_component_results(
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

fn print_component_results(
    results: Vec<(&Firmware, &ComponentVerifyResult)>,
    out: &mut Box<dyn ReportOutput>,
    out_fmt: &OutputFormat,
) -> Result<(), Error> {
    let (_, result) = *results.first().unwrap();
    if let Some(exe) = &result.exe {
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
            iter::once(Cell::new(&exe.name))
                .chain(
                    results
                        .iter()
                        .map(|(_, result)| out.result_cell(result.exe.as_ref().unwrap(), out_fmt)),
                )
                .collect(),
        ));
        for (idx, (required, lib)) in result.libs.iter().enumerate() {
            let name = if *required {
                Cell::new(&format!("required lib {}", lib.name))
            } else {
                Cell::new(&format!("lib {}", lib.name))
            };
            table.add_row(Row::new(
                iter::once(name)
                    .chain(results.iter().map(|(_, result)| {
                        out.result_cell(&result.libs.get(idx).unwrap().1, out_fmt)
                    }))
                    .collect(),
            ));
        }
        out.print_table(&table)?;
    } else {
        out.write_fmt(format_args!("Skip because this component is not native\n"))?;
    }
    return Ok(());
}
