use std::collections::BTreeMap;
use std::fmt::Display;
use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use clap::Parser;
use cli_lib::ExitCode;
use fw_lib::FirmwareInfo;
use regex::Regex;

mod extractor;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, num_args(1..))]
    inputs: Vec<PathBuf>,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(short = 'w', long)]
    rewrite: bool,
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

struct FirmwareExtractor {
    fw_info: FirmwareInfo,
    rootfs_path: PathBuf,
    lib_paths: Vec<PathBuf>,
    opkg_info_paths: Vec<PathBuf>,
    so_regex: Regex,
}

fn main() {
    let args = Args::parse();
    if let Err((code, message)) = run(args) {
        eprintln!("{message}");
        code.exit();
    }
}

/// An output failure, with the code to exit with.
fn output_error<E: Display>(what: &str, e: &E) -> (ExitCode, String) {
    return (ExitCode::OutputError, format!("Failed to {what}: {e}"));
}

fn run(args: Args) -> Result<(), (ExitCode, String)> {
    let mut bad_input = false;
    for input in args.inputs {
        let extractor = match FirmwareExtractor::create(&input) {
            Ok(extractor) => extractor,
            Err(e) => {
                eprintln!("Failed to read input {}: {e}", input.to_string_lossy());
                bad_input = true;
                continue;
            }
        };

        let output = args.output.join(format!(
            "{}-{}",
            extractor.fw_info.version, extractor.fw_info.ota_id
        ));
        if !output.exists() {
            fs::create_dir_all(&output)
                .map_err(|e| output_error("create the output directory", &e))?;
        } else if !args.rewrite {
            println!("Skipping existing {}", extractor.fw_info);
            continue;
        }
        println!("Extracting information from {}", extractor.fw_info);

        let mut lib_index: BTreeMap<String, String> = BTreeMap::new();
        let mut files_pkg_index: BTreeMap<PathBuf, String> = BTreeMap::new();
        extractor.extract_pkgs(&mut files_pkg_index, &output)?;
        extractor.extract_libs(&files_pkg_index, &mut lib_index, &output)?;
        let writer = BufWriter::new(
            File::create(output.join("index.json"))
                .map_err(|e| output_error("open index.json", &e))?,
        );
        serde_json::to_writer_pretty(writer, &lib_index)
            .map_err(|e| output_error("write index.json", &e))?;
        let writer = BufWriter::new(
            File::create(output.join("info.json"))
                .map_err(|e| output_error("open info.json", &e))?,
        );
        serde_json::to_writer_pretty(writer, &extractor.fw_info)
            .map_err(|e| output_error("write info.json", &e))?;
    }
    if bad_input {
        ExitCode::BadInput.exit();
    }
    return Ok(());
}
