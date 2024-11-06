use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::io::{BufWriter, Error, ErrorKind};
use std::path::PathBuf;
use std::process::exit;

use clap::Parser;
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
    if let Err(e) = run(args) {
        eprintln!("{}", e.to_string());
        exit(1);
    }
}

fn run(args: Args) -> Result<(), Error> {
    for input in args.inputs {
        let Ok(extractor) = FirmwareExtractor::create(&input).map_err(|e| {
            let msg = format!("Failed to read input {}: {:?}", input.to_string_lossy(), e);
            eprintln!("{msg}");
            Error::new(e.kind(), msg)
        }) else {
            continue;
        };

        let output = args.output.join(format!(
            "{}-{}",
            extractor.fw_info.version, extractor.fw_info.ota_id
        ));
        if !output.exists() {
            fs::create_dir_all(output.clone()).map_err(|e| {
                Error::new(
                    e.kind(),
                    format!("Failed to create directory for output: {:?}", e),
                )
            })?;
        } else if !args.rewrite {
            println!("Skipping existing {}", extractor.fw_info);
            continue;
        }
        println!("Extracting information from {}", extractor.fw_info);

        let mut lib_index: BTreeMap<String, String> = BTreeMap::new();
        let mut files_pkg_index: BTreeMap<PathBuf, String> = BTreeMap::new();
        extractor.extract_pkgs(&mut files_pkg_index, &output);
        extractor.extract_libs(&files_pkg_index, &mut lib_index, &output);
        let writer =
            BufWriter::new(File::create(output.join("index.json")).map_err(|e| {
                Error::new(e.kind(), format!("Failed to open index.json: {:?}", e))
            })?);
        serde_json::to_writer_pretty(writer, &lib_index).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Failed to write index {:?}", e),
            )
        })?;
        let writer = BufWriter::new(
            File::create(output.join("info.json"))
                .map_err(|e| Error::new(e.kind(), format!("Failed to open info.json: {:?}", e)))?,
        );
        serde_json::to_writer_pretty(writer, &extractor.fw_info).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Failed to write firmware info {:?}", e),
            )
        })?;
    }
    Ok(())
}
