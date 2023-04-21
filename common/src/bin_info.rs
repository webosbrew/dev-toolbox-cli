use elf::{abi, ElfBytes};
use elf::endian::AnyEndian;
use crate::BinaryInfo;

impl BinaryInfo {
    pub fn parse(data: &[u8]) -> Result<Self, elf::ParseError> {
        let mut rpath = Vec::<String>::new();
        let mut needed = Vec::<String>::new();
        let mut undefined = Vec::<String>::new();
        let elf = ElfBytes::<AnyEndian>::minimal_parse(data)?;
        let dynstr_table = elf.section_data_as_strtab(&elf
            .section_header_by_name(".dynstr")?.unwrap()).unwrap();
        if let Ok(Some(dyn_table)) = elf.dynamic() {
            dyn_table.iter().find(|entry| entry.d_tag == abi::DT_STRTAB)
                .map(|entry| entry.d_ptr());
            for entry in dyn_table {
                match entry.d_tag {
                    abi::DT_NEEDED => {
                        needed.push(String::from(dynstr_table.get(entry.d_val() as usize).unwrap()));
                    }
                    abi::DT_RPATH | abi::DT_RUNPATH => {
                        rpath.push(dynstr_table.get(entry.d_val() as usize).unwrap().split(":")
                            .map(|s| String::from(s)).collect())
                    }
                    _ => {}
                }
            }
        }
        let ver_table = elf.symbol_version_table().unwrap();
        if let Ok(Some((sym_table, str))) = elf.dynamic_symbol_table() {
            for (idx, sym) in sym_table.iter().enumerate() {
                if !sym.is_undefined() || sym.st_name == 0 {
                    continue;
                }
                let sym_name = str.get(sym.st_name as usize).unwrap();
                if let Some(ver_req) = ver_table.as_ref()
                    .map(|v| v.get_requirement(idx).unwrap())
                    .flatten() {
                    undefined.push(format!("{}@{}", sym_name, ver_req.name));
                } else {
                    undefined.push(format!("{}", sym_name));
                }
            }
        }
        return Ok(Self {
            rpath,
            needed,
            undefined,
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::BinaryInfo;

    #[test]
    fn test_parse() {
        let content = include_bytes!("fixtures/sample.bin");
        let info = BinaryInfo::parse(content).expect("should not have any error");
        assert_eq!(info.needed[0], "libc.so.6");
    }
}