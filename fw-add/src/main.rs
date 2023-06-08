use std::collections::BTreeMap;
use std::fs;
use std::fs::{DirEntry, File};
use std::io::{BufRead, BufReader, BufWriter, Error, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::exit;

use clap::Parser;
use path_slash::PathBufExt as _;
use regex::Regex;

use common::{FirmwareInfo, LibraryInfo};

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, num_args(1..))]
    inputs: Vec<String>,
    #[arg(short, long)]
    output: String,
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
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
        let input = Path::new(&input);
        let fw_info = extract_fw_info(input)?;
        let output =
            Path::new(&args.output).join(format!("{}-{}", fw_info.version, fw_info.ota_id));
        if !output.exists() {
            std::fs::create_dir_all(output.clone())?;
        }
        println!("Extracting information from {fw_info}");
        let lib_paths = extract_lib_paths(input)?;
        let so_regex = Regex::new("^.+.so(\\.\\w+)*$").unwrap();
        let mut mappings: BTreeMap<String, String> = BTreeMap::new();
        for lib_path in lib_paths {
            if let Ok(dir) = lib_path.read_dir() {
                for ent in dir {
                    match ent {
                        Ok(ent) => {
                            handle_entry(ent, &so_regex, &mut mappings, &output, args.debug);
                        }
                        Err(e) => {
                            eprintln!("{e:?}");
                        }
                    }
                }
            }
        }
        let writer = BufWriter::new(File::create(output.join("index.json"))?);
        serde_json::to_writer_pretty(writer, &mappings).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Failed to write index {:?}", e),
            )
        })?;
        let writer = BufWriter::new(File::create(output.join("info.json"))?);
        serde_json::to_writer_pretty(writer, &fw_info).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Failed to write firmware info {:?}", e),
            )
        })?;
    }
    return Ok(());
}

fn handle_entry<P>(
    ent: DirEntry,
    so_regex: &Regex,
    mappings: &mut BTreeMap<String, String>,
    output: P,
    debug: u8,
) where
    P: AsRef<Path>,
{
    let ent_path = ent.path();
    if let (Some(name), Some(metadata)) =
        (ent_path.file_name().unwrap().to_str(), ent.metadata().ok())
    {
        if !so_regex.is_match(name) {
            return;
        }
        if metadata.is_file() {
            let lib_info = match File::open(&ent_path).and_then(|file| {
                LibraryInfo::parse(file, false, ent_path.file_name().unwrap().to_string_lossy())
                    .map_err(|e| {
                        Error::new(
                            ErrorKind::InvalidData,
                            format!("Failed to parse library {name}: {e:?}"),
                        )
                    })
            }) {
                Ok(info) => info,
                Err(e) => {
                    eprintln!("Ignoring library {name}: {e:?}");
                    return;
                }
            };
            if debug > 0 {
                println!("Saving symbols list for {name}");
            }
            let symbols_name = format!("{}.json", name);
            File::create(output.as_ref().join(&symbols_name))
                .and_then(|file| {
                    let writer = BufWriter::new(file);
                    return serde_json::to_writer_pretty(writer, &lib_info).map_err(|e| {
                        Error::new(ErrorKind::InvalidData, format!("Failed to write {:?}", e))
                    });
                })
                .unwrap();
            mappings.insert(String::from(name), symbols_name);
        } else if metadata.is_symlink() {
            if let Some(target_name) = ent_path
                .read_link()
                .ok()
                .as_ref()
                .map(|target| {
                    target
                        .file_name()
                        .map(|target_name| target_name.to_str())
                        .flatten()
                })
                .flatten()
            {
                mappings.insert(String::from(name), format!("{}.json", target_name));
            }
        }
    }
}

fn extract_fw_info(input: &Path) -> Result<FirmwareInfo, Error> {
    let (version, ota_id) = input
        .file_name()
        .map(|name| name.to_str())
        .flatten()
        .map(|s| s.split_once("-"))
        .flatten()
        .ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                format!("Bad input path {}", input.display()),
            )
        })?;
    let mut starfish_release = String::new();

    File::open(
        input
            .join("rootfs.pak.unsquashfs")
            .join("etc")
            .join("starfish-release"),
    )?
    .read_to_string(&mut starfish_release)?;
    let release_regex = Regex::new("release (\\d+\\.\\d+\\.\\d+)").unwrap();
    let cap = release_regex.captures(&starfish_release).ok_or_else(|| {
        Error::new(
            ErrorKind::NotFound,
            format!("Bad starfish-release: {starfish_release}"),
        )
    })?;
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
    return Ok(vec!["lib", "usr/lib"]
        .iter()
        .map(|p| rootfs_path.join(PathBuf::from_slash(*p)))
        .chain(reader.lines().filter_map(|line| {
            let trimmed = line
                .as_ref()
                .map_or("", |r| r.trim().strip_prefix("/").unwrap_or(""));
            if trimmed.is_empty() {
                return None;
            }
            if let Some(bsp) = trimmed.strip_prefix("mnt/bsppart/") {
                return fs::read_dir(input)
                    .and_then(|mut dir| {
                        return Ok(dir.find_map(|entry| {
                            if let Ok(entry) = entry {
                                let file_name = entry.file_name();
                                if Regex::new(r"bsppart(-\w+)?.pak.unsquashfs")
                                    .unwrap()
                                    .is_match(&*file_name.to_string_lossy())
                                {
                                    return Some(
                                        input.join(file_name).join(PathBuf::from_slash(bsp)),
                                    );
                                }
                            }
                            return None;
                        }));
                    })
                    .ok()
                    .flatten();
            }
            return Some(rootfs_path.join(PathBuf::from_slash(trimmed)));
        }))
        .collect());
}
