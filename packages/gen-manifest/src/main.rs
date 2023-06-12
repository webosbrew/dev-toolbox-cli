mod hash;

use std::fs::File;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};
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
    icon_url: String,
    source_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    root_required: Option<RootRequired>,
    ipk_url: String,
    ipk_hash: IpkHash,
}

fn main() {
    let args = Args::parse();
    if args.appinfo.is_some() {
        eprintln!("--appinfo option is not needed anymore.");
    }
    let package = Package::open(&args.pkgfile).unwrap();
    let app_info = package.app.info;
    let manifest = HomebrewManifest {
        id: app_info.id,
        version: app_info.version,
        r#type: app_info.r#type,
        title: app_info.title,
        app_description: app_info.app_description,
        icon_url: args.icon,
        source_url: args.link,
        root_required: args.root,
        ipk_url: String::from(args.pkgfile.file_name().unwrap().to_string_lossy()),
        ipk_hash: IpkHash::from(&args.pkgfile).unwrap(),
    };
    if let Some(output) = args.output {
        serde_json::to_writer_pretty(&mut File::create(&output).unwrap(), &manifest).unwrap();
    } else {
        serde_json::to_writer_pretty(&mut std::io::stdout(), &manifest).unwrap();
    }
}
