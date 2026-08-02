use std::fs::File;
use std::path::PathBuf;

use clap::Parser;
use semver::VersionReq;

use bin_lib::BinaryInfo;
use fw_lib::Firmware;
use verify_lib::exit::ExitCode;
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
    for executable in args.executables {
        let Ok(file) = File::open(&executable) else {
            eprintln!("Failed to open file {}", executable.to_string_lossy());
            bad_input = true;
            continue;
        };
        let parsed = BinaryInfo::parse(
            file,
            executable.file_name().unwrap().to_string_lossy(),
            !args.skip_rpath,
        );
        let mut info = match parsed {
            Ok(info) => info,
            Err(e) => {
                eprintln!(
                    "Failed to parse {}: {e}",
                    executable.file_name().unwrap().to_string_lossy()
                );
                bad_input = true;
                continue;
            }
        };
        info.rpath.extend(args.lib_paths.clone());
        let mut all_ok = true;
        for firmware in &firmwares {
            let result = info.verify(&|name| firmware.find_library(name));
            println!("Verify result for firmware {}:", firmware.info);
            for lib in result.missing_lib {
                println!("Missing library: {lib}");
                all_ok = false;
            }
            for sym in result.undefined_sym {
                println!("Missing symbol: {sym}");
                all_ok = false;
            }
            // The loader resolves these on the first call, so the binary still
            // loads. Report them, but do not fail.
            for sym in result.undefined_sym_lazy {
                println!("Warning: missing symbol {sym} is bound lazily");
            }
        }
        if all_ok {
            println!("All OK.");
        } else {
            all_good = false;
        }
    }
    // A file the tool could not read outranks an incompatibility: the run did
    // not answer the question that was asked.
    if bad_input {
        ExitCode::BadInput.exit();
    }
    if !all_good {
        ExitCode::Incompatible.exit();
    }
}
