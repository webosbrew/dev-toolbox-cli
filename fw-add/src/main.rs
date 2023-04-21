use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Error, ErrorKind, Read};
use std::path::{Path, PathBuf};

use clap::Parser;
use path_slash::PathBufExt as _;
use regex::Regex;

use common::{FirmwareInfo, LibraryInfo};

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    input: String,
    #[arg(short, long)]
    output: String,
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
}

fn main() {
    let args = Args::parse();
    run(args).unwrap();
}

fn run(args: Args) -> Result<(), Error> {
    let input = Path::new(&args.input);
    let output = Path::new(&args.output);
    let fw_info = extract_fw_info(input)?;
    println!("Extracting information from {fw_info}");
    let lib_paths = extract_lib_paths(input)?;
    let so_regex = Regex::new("^.+.so(\\.\\w+)*$").unwrap();
    let mut mappings: HashMap<String, String> = HashMap::new();
    for lib_path in lib_paths {
        if let Ok(dir) = lib_path.read_dir() {
            for ent in dir {
                match ent {
                    Ok(ent) => {
                        if let (Some(name), Some(metadata)) =
                            (ent.file_name().to_str(), ent.metadata().ok()) {
                            if !so_regex.is_match(name) {
                                continue;
                            }
                            if metadata.is_file() {
                                let lib_info = match LibraryInfo::parse(File::open(ent.path())?) {
                                    Ok(info) => info,
                                    Err(e) => {
                                        eprintln!("Ignoring library {name}: {e:?}");
                                        continue;
                                    }
                                };
                                println!("Saving symbols list for {name}");
                                let symbols_name = format!("{}.json", name);
                                let writer = BufWriter::new(File::create(output.join(&symbols_name))?);
                                serde_json::to_writer_pretty(writer, &lib_info)
                                    .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Failed to write {:?}", e)))?;
                                mappings.insert(String::from(name), symbols_name);
                            } else if metadata.is_symlink() {
                                if let Some(target_name) = ent.path().read_link().ok().as_ref().map(|target| target.file_name()
                                    .map(|target_name| target_name.to_str()).flatten()).flatten() {
                                    mappings.insert(String::from(name), format!("{}.json", target_name));
                                }
                            }
                        }
                    }
                    Err(e) => { eprintln!("{e:?}"); }
                }
            }
        }
    }
    let writer = BufWriter::new(File::create(output.join("index.json"))?);
    serde_json::to_writer_pretty(writer, &mappings)
        .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Failed to write {:?}", e)))?;
    return Ok(());
}

fn extract_fw_info(input: &Path) -> Result<FirmwareInfo, Error> {
    let (version, ota_id) = input.file_name().map(|name| name.to_str()).flatten()
        .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Bad input path {}", input.display())))?
        .split_once("-").unwrap();
    let mut starfish_release = String::new();

    File::open(input.join("rootfs.pak.unsquashfs").join("etc").join("starfish-release"))?
        .read_to_string(&mut starfish_release)?;
    let release_regex = Regex::new("release (\\d+\\.\\d+\\.\\d+)").unwrap();
    let cap = release_regex.captures(&starfish_release)
        .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Bad starfish-release: {starfish_release}")))?;
    return Ok(FirmwareInfo {
        version: String::from(version),
        ota_id: String::from(ota_id),
        release: String::from(cap.get(1).unwrap().as_str()),
    });
}

fn extract_lib_paths(input: &Path) -> Result<Vec<PathBuf>, Error> {
    let rootfs_path = input.join("rootfs.pak.unsquashfs");
    let ldconf_path = rootfs_path.join("etc").join("ld.so.conf");
    let reader = BufReader::new(File::open(ldconf_path)?);
    return Ok(vec!["lib", "usr/lib"].iter().map(|p| rootfs_path.join(PathBuf::from_slash(*p)))
        .chain(reader.lines()
            .filter_map(|line| {
                let trimmed = line.as_ref().map_or("", |r| r.trim().strip_prefix("/")
                    .unwrap_or(""));
                if trimmed.is_empty() {
                    return None;
                }
                if let Some(bsp) = trimmed.strip_prefix("mnt/bsppart/") {
                    return Some(input.join("bsppart.pak.unsquashfs").join(PathBuf::from_slash(bsp)));
                }
                return Some(rootfs_path.join(PathBuf::from_slash(trimmed)));
            }))
        .collect());
}