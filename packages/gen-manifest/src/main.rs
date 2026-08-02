mod hash;

use std::fs::File;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use cli_lib::{file_label, ExitCode};
use ipk_lib::Package;
use serde::{Serialize, Serializer};

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    appinfo: Option<String>,
    #[arg(short, long)]
    pkgfile: PathBuf,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(short, long)]
    icon: String,
    #[arg(short, long)]
    link: String,
    #[arg(short, long, value_enum)]
    root: Option<RootRequired>,
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[derive(Debug, Clone, PartialEq, ValueEnum)]
enum RootRequired {
    True,
    False,
    Optional,
}

impl Serialize for RootRequired {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        return match self {
            RootRequired::True => serializer.serialize_bool(true),
            RootRequired::False => serializer.serialize_bool(false),
            RootRequired::Optional => serializer.serialize_str("optional"),
        };
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IpkHash {
    sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HomebrewManifest {
    id: String,
    version: String,
    r#type: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_description: Option<String>,
    icon_uri: String,
    source_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    root_required: Option<RootRequired>,
    ipk_url: String,
    ipk_hash: IpkHash,
    ipk_size: u64,
    installed_size: Option<u64>,
}

fn main() {
    let args = Args::parse();
    if args.appinfo.is_some() {
        eprintln!("--appinfo option is not needed anymore.");
    }
    if let Err((code, message)) = run(args) {
        eprintln!("{message}");
        code.exit();
    }
}

/// Build the manifest and write it. The error carries the code to exit with.
fn run(args: Args) -> Result<(), (ExitCode, String)> {
    let name = file_label(&args.pkgfile).to_string();
    let bad_input = |what: &str, e: &dyn std::fmt::Display| {
        return (ExitCode::BadInput, format!("Failed to {what} {name}: {e}"));
    };
    let package = Package::open(&args.pkgfile).map_err(|e| bad_input("open", &e))?;
    let hash = IpkHash::from(&args.pkgfile).map_err(|e| bad_input("hash", &e))?;
    let size = args
        .pkgfile
        .metadata()
        .map_err(|e| bad_input("read", &e))?
        .len();
    let app_info = package.app.info;
    let manifest = HomebrewManifest {
        id: app_info.id,
        version: app_info.version,
        r#type: app_info.r#type,
        title: app_info.title,
        app_description: app_info.app_description,
        icon_uri: args.icon,
        source_url: args.link,
        root_required: args.root,
        ipk_url: name.clone(),
        ipk_hash: hash,
        ipk_size: size,
        installed_size: package.installed_size,
    };
    let write = if let Some(output) = &args.output {
        File::create(output)
            .map_err(|e| {
                (
                    ExitCode::OutputError,
                    format!("Failed to create {}: {e}", file_label(output)),
                )
            })
            .and_then(|mut file| {
                return serde_json::to_writer_pretty(&mut file, &manifest).map_err(|e| {
                    (
                        ExitCode::OutputError,
                        format!("Failed to write {}: {e}", file_label(output)),
                    )
                });
            })
    } else {
        serde_json::to_writer_pretty(&mut std::io::stdout(), &manifest).map_err(|e| {
            (
                ExitCode::OutputError,
                format!("Failed to write the manifest: {e}"),
            )
        })
    };
    return write;
}
