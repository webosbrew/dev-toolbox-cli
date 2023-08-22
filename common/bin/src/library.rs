use std::cmp::Ordering;
use std::ops::Deref;

use elf::dynamic::Dyn;
use elf::endian::AnyEndian;
use elf::symbol::Symbol;
use elf::{abi, ElfStream, ParseError};

use crate::LibraryInfo;

const IGNORED_SYMBOLS: &[&str] = &[
    "__bss_end__",
    "_bss_end__",
    "__bss_start",
    "__bss_start__",
    "__end__",
    "_end",
    "_fini",
    "_init",
    "_edata",
];

impl LibraryInfo {
    pub fn has_name(&self, name: &str) -> bool {
        return self.name == name || self.names.iter().find(|n| n.deref() == name).is_some();
    }

    pub fn has_symbol(&self, symbol: &str) -> bool {
        return self
            .symbols
            .binary_search_by(|def| {
                let ordering = symbol.cmp(def);
                if ordering != Ordering::Equal && def.contains("@") && !symbol.contains("@") {
                    let sym_len = symbol.len();
                    if def.len() >= sym_len {
                        return symbol.cmp(&def[..sym_len]).reverse();
                    }
                }
                return ordering.reverse();
            })
            .is_ok();
    }

    pub fn parse<S, N>(source: S, with_undefined: bool, name: N) -> Result<Self, ParseError>
    where
        S: std::io::Read + std::io::Seek,
        N: AsRef<str>,
    {
        let mut needed = Vec::<String>::new();
        let mut elf = ElfStream::<AnyEndian, S>::open_stream(source)?;
        let mut name = String::from(name.as_ref());

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
                abi::DT_SONAME => {
                    name = String::from(dynstr_table.get(entry.d_val() as usize).unwrap());
                }
                _ => {}
            }
        }

        let (sym_table, str) = elf.dynamic_symbol_table()?.unwrap();
        let all_syms: Vec<(Symbol, String)> = sym_table
            .iter()
            .map(move |sym| {
                (
                    sym.clone(),
                    String::from(str.get(sym.st_name as usize).unwrap_or("")),
                )
            })
            .collect();
        let ver_table = elf.symbol_version_table()?;
        let mut symbols: Vec<String> = all_syms
            .iter()
            .enumerate()
            .flat_map(|(index, (sym, name))| {
                if sym.is_undefined() || sym.st_name == 0 || IGNORED_SYMBOLS.contains(&&**name) {
                    return vec![];
                }
                if let Some(ver_table) = &ver_table {
                    if let Some(ver) = ver_table.get_definition(index).ok().flatten() {
                        return ver
                            .names
                            .filter_map(|ver_name| {
                                if let Ok(ver_name) = ver_name {
                                    return Some(format!("{name}@{ver_name}"));
                                }
                                return None;
                            })
                            .collect();
                    }
                }
                return vec![name.clone()];
            })
            .collect();

        let undefined: Vec<String> = if with_undefined {
            let mut list: Vec<String> = all_syms
                .iter()
                .enumerate()
                .flat_map(|(index, (sym, name))| {
                    if !sym.is_undefined()
                        || sym.st_name == 0
                        || sym.st_bind() == abi::STB_WEAK
                        || IGNORED_SYMBOLS.contains(&&**name)
                    {
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
            list.sort_unstable();
            list
        } else {
            Vec::new()
        };

        needed.sort_unstable();
        symbols.sort_unstable();

        return Ok(Self {
            name,
            needed,
            symbols,
            undefined,
            names: Default::default(),
            priority: Default::default(),
        });
    }
}
