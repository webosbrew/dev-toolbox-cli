use std::collections::HashMap;
use std::fs::File;
use std::io::{Error, Write};
use std::iter;
use std::path::PathBuf;

use clap::Parser;
use prettytable::format::{FormatBuilder, LinePosition, LineSeparator};
use prettytable::{color, Attr, Cell, Row, Table};
use semver::VersionReq;
use serde::Deserialize;

use common::{
    BinVerifyResult, BinaryInfo, Firmware, LibraryInfo, VerifyResult, VerifyWithFirmware,
};

mod component;
mod links;
mod package;

#[derive(Parser, Debug)]
struct Args {
    #[arg(required = true, help = "Packages to verify")]
    packages: Vec<PathBuf>,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(short, long, value_enum, default_value = "plain")]
    format: OutputFormat,
    #[arg(long)]
    fw_releases: Option<VersionReq>,
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
}

#[derive(Debug, Clone, PartialEq, clap::ValueEnum)]
enum OutputFormat {
    Markdown,
    Terminal,
    Plain,
}

fn main() {
    let args = Args::parse();
    let to_file: bool = args.output.is_some();
    let mut output: Box<dyn Write> = if let Some(path) = args.output {
        Box::new(File::create(path).unwrap())
    } else {
        Box::new(std::io::stdout())
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
        let package = match File::open(&package).and_then(|file| Package::parse(file)) {
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
            &args.format,
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
                &args.format,
            )
            .unwrap();
        }
    }
}

fn print_component_results<Output>(
    results: Vec<(&Firmware, &ComponentVerifyResult)>,
    out: &mut Output,
    out_fmt: &OutputFormat,
) -> Result<(), Error>
where
    Output: Write + ?Sized,
{
    let (_, result) = *results.first().unwrap();
    fn result_cell(result: &BinVerifyResult) -> Cell {
        return if result.is_good() {
            let mut cell = Cell::new("OK");
            cell.style(Attr::ForegroundColor(color::BRIGHT_GREEN));
            cell
        } else {
            let mut cell = Cell::new("NG");
            cell.style(Attr::ForegroundColor(color::BRIGHT_RED));
            cell
        };
    }
    if let Some(exe) = &result.exe {
        let mut table = Table::new();
        match out_fmt {
            OutputFormat::Markdown => {
                table.set_format(
                    FormatBuilder::new()
                        .column_separator('|')
                        .borders('|')
                        .padding(1, 1)
                        .separator(LinePosition::Title, LineSeparator::new('-', '|', '|', '|'))
                        .build(),
                );
            }
            OutputFormat::Terminal => {
                table.set_format(*prettytable::format::consts::FORMAT_BOX_CHARS);
            }
            OutputFormat::Plain => {
                table.set_format(*prettytable::format::consts::FORMAT_DEFAULT);
            }
        }
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
                        .map(|(_, result)| result_cell(result.exe.as_ref().unwrap())),
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
                    .chain(
                        results
                            .iter()
                            .map(|(_, result)| result_cell(&result.libs.get(idx).unwrap().1)),
                    )
                    .collect(),
            ));
        }
        if *out_fmt == OutputFormat::Terminal {
            table.print_tty(true)?;
        } else {
            table.print(out)?;
        }
        out.write_all(b"\n")?;
    } else {
        println!("Skip because this component is not native");
    }
    return Ok(());
}

#[derive(Debug)]
struct Package {
    id: String,
    app: Component,
    services: Vec<Component>,
}

#[derive(Debug)]
struct Component {
    id: String,
    exe: Option<BinaryInfo>,
    libs: Vec<LibraryInfo>,
}

#[derive(Debug)]
struct Symlinks {
    mapping: HashMap<PathBuf, PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct PackageInfo {
    app: String,
    #[serde(default)]
    services: Vec<String>,
}

#[derive(Debug)]
struct PackageVerifyResult {
    app: ComponentVerifyResult,
    services: Vec<ComponentVerifyResult>,
}

#[derive(Debug)]
struct ComponentVerifyResult {
    id: String,
    exe: Option<BinVerifyResult>,
    libs: Vec<(bool, BinVerifyResult)>,
}
