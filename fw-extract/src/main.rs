use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufWriter, Error, ErrorKind, Read};
use std::path::PathBuf;
use std::process::exit;

use clap::Parser;
use path_slash::PathBufExt as _;
use regex::Regex;

use common::FirmwareInfo;

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
    debug: u8,
}

struct FirmwareExtractor {
    fw_info: FirmwareInfo,
    rootfs_path: PathBuf,
    lib_paths: Vec<PathBuf>,
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
        let extractor = FirmwareExtractor::create(&input)?;

        let output = args.output.join(format!(
            "{}-{}",
            extractor.fw_info.version, extractor.fw_info.ota_id
        ));
        if !output.exists() {
            fs::create_dir_all(output.clone())?;
        } else if !args.rewrite {
            println!("Skipping existing {}", extractor.fw_info);
            continue;
        }
        println!("Extracting information from {}", extractor.fw_info);

        let mut mappings: BTreeMap<String, String> = BTreeMap::new();
        extractor.extract_libs(&mut mappings, &output);
        let writer = BufWriter::new(File::create(output.join("index.json"))?);
        serde_json::to_writer_pretty(writer, &mappings).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Failed to write index {:?}", e),
            )
        })?;
        let writer = BufWriter::new(File::create(output.join("info.json"))?);
        serde_json::to_writer_pretty(writer, &extractor.fw_info).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Failed to write firmware info {:?}", e),
            )
        })?;
    }
    return Ok(());
}
