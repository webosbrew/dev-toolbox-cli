use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde::Deserialize;

use common::{BinVerifyResult, BinaryInfo, Firmware, LibraryInfo, VerifyWithFirmware};

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
        for firmware in Firmware::list(Path::new("data")).unwrap() {
            println!("## On {}", firmware.info);
            let result = package.verify(&firmware);
            println!("### App {}", result.app.id);
            if let Some(app_exe) = result.app.exe {
                println!("main {:?}", app_exe);
                for (important, lib) in result.app.libs {
                    if important {
                        print!("required ");
                    }
                    println!("{:?}", lib);
                }
            } else {
                println!("Skipping non-native application");
            }

            for service in result.services {
                println!("### Service {}", service.id);
                if let Some(svc_exe) = service.exe {
                    println!("{}", svc_exe.name);
                } else {
                    println!("Skipping non-native service");
                }
            }
            break;
        }
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
