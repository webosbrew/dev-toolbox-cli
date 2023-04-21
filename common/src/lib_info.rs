use elf::{abi, ElfStream, ParseError};
use elf::dynamic::Dyn;
use elf::endian::AnyEndian;
use elf::symbol::Symbol;

use crate::LibraryInfo;

impl LibraryInfo {
    pub fn parse<S>(source: S) -> Result<Self, ParseError>
        where S: std::io::Read + std::io::Seek {
        let mut needed = Vec::<String>::new();
        let mut elf = ElfStream::<AnyEndian, S>::open_stream(source)?;


        let dynamic_entries: Vec<Dyn> = elf.dynamic()?.map(|tbl| tbl.iter().collect())
            .unwrap_or_default();

        let dynstr_header = *elf.section_header_by_name(".dynstr")?.unwrap();
        let dynstr_table = elf.section_data_as_strtab(&dynstr_header).unwrap();
        for entry in dynamic_entries {
            match entry.d_tag {
                abi::DT_NEEDED => {
                    needed.push(String::from(dynstr_table.get(entry.d_val() as usize).unwrap()));
                }
                _ => {}
            }
        }

        let (sym_table, str) = elf.dynamic_symbol_table()?.unwrap();
        let symbols: Vec<(Symbol, String)> = sym_table.iter().map(move |sym|
            (sym.clone(), String::from(str.get(sym.st_name as usize).unwrap_or("")))).collect();
        let ver_table = elf.symbol_version_table()?;
        let mut defined: Vec<String> = symbols.iter().enumerate().flat_map(|(index, (sym, name))| {
            if sym.is_undefined() || sym.st_name == 0 {
                return vec![];
            }
            if let Some(ver_table) = &ver_table {
                if let Some(ver) = ver_table.get_definition(index).ok().flatten() {
                    return ver.names.filter_map(|ver_name| {
                        if let Ok(ver_name) = ver_name {
                            return Some(format!("{name}@{ver_name}"));
                        }
                        return None;
                    }).collect();
                }
            }
            return vec![name.clone()];
        }).collect();

        needed.sort_unstable();
        defined.sort_unstable();

        return Ok(Self {
            needed,
            defined,
        });
    }
}