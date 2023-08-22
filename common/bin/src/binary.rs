use elf::dynamic::Dyn;
use elf::endian::AnyEndian;
use elf::symbol::Symbol;
use elf::{abi, ElfStream};

use crate::BinaryInfo;

impl BinaryInfo {
    pub fn parse<S, N>(source: S, name: N) -> Result<Self, elf::ParseError>
    where
        S: std::io::Read + std::io::Seek,
        N: AsRef<str>,
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
                abi::DT_RPATH | abi::DT_RUNPATH => rpath.extend(
                    dynstr_table
                        .get(entry.d_val() as usize)
                        .unwrap()
                        .split(":")
                        .map(|s| String::from(s)),
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
                if !sym.is_undefined() || sym.st_name == 0 || sym.st_bind() == abi::STB_WEAK {
                    return vec![];
                }
                if let Some(ver) = ver_table
                    .as_ref()
                    .map(|t| t.get_requirement(index).ok().flatten())
                    .flatten()
                {
                    return vec![format!("{name}@{}", ver.name)];
                }
                return vec![name.clone()];
            })
            .collect();

        return Ok(Self {
            name: String::from(name.as_ref()),
            rpath,
            needed,
            undefined,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::BinaryInfo;

    #[test]
    fn test_parse() {
        let mut content = Cursor::new(include_bytes!("fixtures/sample.bin"));
        let info =
            BinaryInfo::parse(&mut content, "sample.bin").expect("should not have any error");
        assert_eq!(info.needed[0], "libc.so.6");
    }
}
