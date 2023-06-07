use std::collections::HashMap;
use std::fs::File;
use std::iter;
use std::path::{Path, PathBuf};

use clap::Parser;
use prettytable::{color, Attr, Cell, Row, Table};
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
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
}

fn main() {
    let args = Args::parse();
    for package in args.packages {
        let package = match File::open(&package) {
            Ok(file) => Package::parse(file).unwrap(),
            Err(e) => {
                eprintln!(
                    "Failed to open {}: {e:?}",
                    package.file_name().unwrap().to_string_lossy()
                );
                continue;
            }
        };
        println!("# Package {}", package.id);
        let results: Vec<(Firmware, PackageVerifyResult)> = Firmware::list(Path::new("data"))
            .unwrap()
            .into_iter()
            .map(|fw| {
                let verify = package.verify(&fw);
                return (fw, verify);
            })
            .collect();
        let (_, result) = results.first().unwrap();
        println!("### App {}", result.app.id);
        print_component_results(results.iter().map(|(fw, res)| (fw, &res.app)).collect());
        for idx in 0..result.services.len() {
            println!("### Service {}", result.services.get(idx).unwrap().id);
            print_component_results(
                results
                    .iter()
                    .map(|(fw, res)| (fw, res.services.get(idx).unwrap()))
                    .collect(),
            );
        }
    }
}

fn print_component_results(results: Vec<(&Firmware, &ComponentVerifyResult)>) {
    let (_, result) = *results.first().unwrap();
    if let Some(exe) = &result.exe {
        let mut table = Table::new();
        table.set_format(*prettytable::format::consts::FORMAT_BOX_CHARS);
        table.set_titles(Row::from_iter(
            iter::once("").chain(
                results
                    .iter()
                    .map(|(firmware, _result)| firmware.info.release.as_str()),
            ),
        ));
        table.add_row(Row::new(
            iter::once(Cell::new(&exe.name))
                .chain(results.iter().map(|(_, result)| {
                    return if result.exe.as_ref().unwrap().is_good() {
                        let mut cell = Cell::new("OK");
                        cell.style(Attr::ForegroundColor(color::BRIGHT_GREEN));
                        cell
                    } else {
                        println!("{:?}", result.exe);
                        let mut cell = Cell::new("NG");
                        cell.style(Attr::ForegroundColor(color::BRIGHT_RED));
                        cell
                    };
                }))
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
                        let result = &result.libs.get(idx).unwrap().1;
                        return if result.is_good() {
                            let mut cell = Cell::new("OK");
                            cell.style(Attr::ForegroundColor(color::BRIGHT_GREEN));
                            cell
                        } else {
                            let mut cell = Cell::new("NG");
                            cell.style(Attr::ForegroundColor(color::BRIGHT_RED));
                            cell
                        };
                    }))
                    .collect(),
            ));
        }
        table.print_tty(true).unwrap();
    } else {
        println!("Skip because this component is not native");
    }
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
    id: String,
    version: String,
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
