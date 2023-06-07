use elf::abi::STB_WEAK;
use elf::dynamic::Dyn;
use elf::endian::AnyEndian;
use elf::symbol::Symbol;
use elf::{abi, ElfStream};
use std::fs::File;
use std::io::{Error, ErrorKind};
use std::path::Path;

use crate::{BinVerifyResult, BinaryInfo, Firmware, LibraryInfo, VerifyResult, VerifyWithFirmware};

impl BinaryInfo {
    pub fn find_library(&self, name: &str) -> Option<LibraryInfo> {
        for rpath in &self.rpath {
            let path = Path::new(rpath).join(name);
            if let Ok(f) = File::open(path) {
                return LibraryInfo::parse(f, false)
                    .map_err(|e| {
                        Error::new(ErrorKind::InvalidData, format!("Bad library info: {e:?}"))
                    })
                    .ok();
            }
        }
        return None;
    }

    pub fn parse<S>(source: S, name: String) -> Result<Self, elf::ParseError>
    where
        S: std::io::Read + std::io::Seek,
    {
        let mut rpath = Vec::<String>::new();
        let mut needed = Vec::<String>::new();
        let mut elf = ElfStream::<AnyEndian, S>::open_stream(source)?;

        let dynamic_entries: Vec<Dyn> = elf
            .dynamic()?
            .map(|tbl| tbl.iter().collect())
            .unwrap_or_default();

        let dynstr_header = *elf.section_header_by_name(".dynstr")?.unwrap();
        let dynstr_table = elf.section_data_as_strtab(&dynstr_header).unwrap();
        for entry in dynamic_entries {
            match entry.d_tag {
                abi::DT_NEEDED => {
                    needed.push(String::from(
                        dynstr_table.get(entry.d_val() as usize).unwrap(),
                    ));
                }
                abi::DT_RPATH | abi::DT_RUNPATH => rpath.push(
                    dynstr_table
                        .get(entry.d_val() as usize)
                        .unwrap()
                        .split(":")
                        .map(|s| String::from(s))
                        .collect(),
                ),
                _ => {}
            }
        }

        let (sym_table, str) = elf.dynamic_symbol_table()?.unwrap();
        let symbols: Vec<(Symbol, String)> = sym_table
            .iter()
            .map(move |sym| {
                (
                    sym.clone(),
                    String::from(str.get(sym.st_name as usize).unwrap_or("")),
                )
            })
            .collect();
        let ver_table = elf.symbol_version_table()?;

        let undefined: Vec<String> = symbols
            .iter()
            .enumerate()
            .flat_map(|(index, (sym, name))| {
                if !sym.is_undefined() || sym.st_name == 0 || sym.st_bind() == STB_WEAK {
                    return vec![];
                }
                if let Some(ver_table) = &ver_table {
                    if let Some(ver) = ver_table.get_requirement(index).ok().flatten() {
                        return vec![format!("{name}@{}", ver.name)];
                    }
                }
                return vec![name.clone()];
            })
            .collect();

        return Ok(Self {
            name,
            rpath,
            needed,
            undefined,
        });
    }
}

impl VerifyWithFirmware<BinVerifyResult> for BinaryInfo {
    fn verify(&self, firmware: &Firmware) -> BinVerifyResult {
        let mut result = BinVerifyResult::new(self.name.clone());
        result.undefined_sym.extend(self.undefined.clone());
        for needed in &self.needed {
            if let Some(lib) = firmware.find_library(needed) {
                result.undefined_sym.retain(|sym| !lib.has_symbol(sym));
            } else if let Some(lib) = self.find_library(needed) {
                result.undefined_sym.retain(|sym| !lib.has_symbol(sym));
            } else {
                result.missing_lib.push(needed.clone());
            }
        }
        return result;
    }
}

impl BinVerifyResult {
    pub fn new(name: String) -> Self {
        return Self {
            name,
            missing_lib: Default::default(),
            undefined_sym: Default::default(),
        };
    }
}

impl VerifyResult for BinVerifyResult {
    fn is_good(&self) -> bool {
        return self.missing_lib.is_empty() && self.undefined_sym.is_empty();
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::BinaryInfo;

    #[test]
    fn test_parse() {
        let mut content = Cursor::new(include_bytes!("fixtures/sample.bin"));
        let info = BinaryInfo::parse(&mut content, String::from("sample.bin"))
            .expect("should not have any error");
        assert_eq!(info.needed[0], "libc.so.6");
    }
}
