use common::BinaryInfo;
use common::FirmwareInfo;

fn main() {
    println!("Hello, world!");

    let data = include_bytes!("ihsplay");
    let firmware = FirmwareInfo::list().first().unwrap();
    let info = BinaryInfo::parse(data).expect("parse error");
    verify_elf(&info, &Vec::new(), firmware, None);
}

fn verify_elf(info: &BinaryInfo, lib_dirs: &[&str], firmware: &FirmwareInfo,
              main_bin: Option<&BinaryInfo>) {
    println!("{:?}", info);
}