use std::fs::File;
use std::path::PathBuf;

use clap::Parser;
use semver::VersionReq;

use bin_lib::BinaryInfo;
use fw_lib::Firmware;
use verify_lib::Verify;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, num_args(1..), required = true)]
    executables: Vec<PathBuf>,
    #[arg(short, long, num_args(1..))]
    lib_paths: Vec<String>,
    #[arg(short = 'R', long, default_value = "false")]
    skip_rpath: bool,
    #[arg(short = 'r', long)]
    fw_releases: Option<VersionReq>,
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
}

fn main() {
    let args = Args::parse();
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
        return;
    }
    for executable in args.executables {
        let Ok(file) = File::open(&executable) else {
            eprintln!("Failed to open file {}", executable.to_string_lossy());
            continue;
        };
        let mut info = BinaryInfo::parse(
            file,
            executable.file_name().unwrap().to_string_lossy(),
            !args.skip_rpath,
        )
        .expect("parse error");
        info.rpath.extend(args.lib_paths.clone());
        let mut all_ok = true;
        for firmware in &firmwares {
            let result = info.verify(&|name| firmware.find_library(name));
            println!("Verify result for firmware {}:", firmware.info);
            for lib in result.missing_lib {
                println!("Missing library: {}", lib);
                all_ok = false;
            }
            for sym in result.undefined_sym {
                println!("Missing symbol: {}", sym);
                all_ok = false;
            }
        }
        if all_ok {
            println!("All OK.");
        }
    }
}
