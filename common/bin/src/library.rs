use std::cmp::Ordering;

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
        self.name == name || self.names.iter().find(|n| *n == name).is_some()
    }

    pub fn has_symbol(&self, symbol: &str) -> bool {
        self.symbols
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
            .is_ok()
    }

    pub fn parse<S, N>(source: S, with_undefined: bool, name: N) -> Result<Self, ParseError>
    where
        S: std::io::Read + std::io::Seek,
        N: AsRef<str>,
    {
        let mut needed = Vec::<String>::new();
        let mut rpath = Vec::<String>::new();
        let mut elf = ElfStream::<AnyEndian, S>::open_stream(source)?;
        let mut name = String::from(name.as_ref());

        let dynamic_entries: Vec<Dyn> = elf
            .dynamic()?
            .map(|tbl| tbl.iter().collect())
            .unwrap_or_default();

        // Guard against an ELF without `.dynstr`/`.dynsym` (not a real shared
        // object) instead of panicking.
        if let Some(dynstr_header) = elf.section_header_by_name(".dynstr")?.copied() {
            let dynstr_table = elf.section_data_as_strtab(&dynstr_header)?;
            for entry in dynamic_entries {
                match entry.d_tag {
                    abi::DT_NEEDED => {
                        if let Ok(s) = dynstr_table.get(entry.d_val() as usize) {
                            needed.push(String::from(s));
                        }
                    }
                    abi::DT_SONAME => {
                        if let Ok(s) = dynstr_table.get(entry.d_val() as usize) {
                            name = String::from(s);
                        }
                    }
                    abi::DT_RPATH | abi::DT_RUNPATH => {
                        if let Ok(s) = dynstr_table.get(entry.d_val() as usize) {
                            rpath.extend(s.split(":").map(|s| String::from(s)));
                        }
                    }
                    _ => {}
                }
            }
        }

        let all_syms: Vec<(Symbol, String)> = match elf.dynamic_symbol_table()? {
            Some((sym_table, str)) => sym_table
                .iter()
                .map(move |sym| {
                    (
                        sym.clone(),
                        String::from(str.get(sym.st_name as usize).unwrap_or("")),
                    )
                })
                .collect(),
            None => Vec::new(),
        };
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

        Ok(Self {
            name,
            package: Default::default(),
            needed,
            symbols,
            undefined,
            rpath,
            names: Default::default(),
            priority: Default::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::LibraryInfo;

    #[test]
    fn test_parse_runpath() {
        // Fixture is a shared object built with -Wl,-rpath,'$ORIGIN/pulseaudio'
        // -Wl,--enable-new-dtags, i.e. a DT_RUNPATH entry.
        let mut content = Cursor::new(include_bytes!("fixtures/lib_runpath.so"));
        let info = LibraryInfo::parse(&mut content, true, "lib_runpath.so")
            .expect("should not have any error");
        assert_eq!(info.name, "libfixture.so.1", "name should come from DT_SONAME");
        assert!(
            info.rpath.iter().any(|p| p == "$ORIGIN/pulseaudio"),
            "rpath should capture DT_RUNPATH, got {:?}",
            info.rpath
        );
    }
}
