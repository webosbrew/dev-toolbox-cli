use std::fs::File;
use std::path::{Path, PathBuf};

use clap::Parser;

use common::{BinaryInfo, Firmware, VerifyWithFirmware};

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
        let mut info = BinaryInfo::parse(
            file,
            String::from(executable.file_name().unwrap().to_string_lossy()),
        )
        .expect("parse error");
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
