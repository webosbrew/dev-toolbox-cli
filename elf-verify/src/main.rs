use std::fs::File;
use std::path::{Path, PathBuf};

use clap::Parser;

use common::{BinaryInfo, Firmware};

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, num_args(1..))]
    executables: Vec<PathBuf>,
    #[arg(short, long, num_args(1..))]
    lib_paths: Vec<String>,
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
}

fn main() {
    let args = Args::parse();
    for executable in args.executables {
        let file = File::open(&executable).unwrap();
        let mut info = BinaryInfo::parse(file).expect("parse error");
        info.rpath.extend(args.lib_paths.clone());
        for firmware in Firmware::list(Path::new("data")).unwrap() {
            println!(
                "Verify result for firmware {} {:?}",
                firmware.info,
                info.verify(&firmware)
            );
        }
    }
}

trait VerifyElf {
    fn verify(&self, firmware: &Firmware) -> BinVerifyResult;
}

#[derive(Default, Debug)]
struct BinVerifyResult {
    missing_lib: Vec<String>,
    undefined_sym: Vec<String>,
}

impl VerifyElf for BinaryInfo {
    fn verify(&self, firmware: &Firmware) -> BinVerifyResult {
        let mut result = BinVerifyResult::default();
        result.undefined_sym.extend(self.undefined.clone());
        for needed in &self.needed {
            if let Some(lib) = firmware.find_library(needed) {
                result.undefined_sym.retain(|sym| !lib.has_symbol(sym));
            } else if let Some(lib) = self.find_library(needed) {
                result.undefined_sym.retain(|sym| !lib.has_symbol(sym));
            } else {
                result.missing_lib.push(needed.clone());
            }
        }
        return result;
    }
}
