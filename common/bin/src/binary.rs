use elf::dynamic::Dyn;
use elf::endian::AnyEndian;
use elf::symbol::Symbol;
use elf::{abi, ElfStream};

use crate::reloc::lazy_bound_symbols;
use crate::BinaryInfo;

impl BinaryInfo {
    pub fn parse<S, N>(source: S, name: N, with_rpath: bool) -> Result<Self, elf::ParseError>
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

        // A statically-linked / non-dynamic ELF (some bundled `ffmpeg` builds,
        // for instance) has no `.dynstr`/`.dynsym`. Treat it as having no dynamic
        // dependencies or imports rather than panicking.
        if let Some(dynstr_header) = elf.section_header_by_name(".dynstr")?.copied() {
            let dynstr_table = elf.section_data_as_strtab(&dynstr_header)?;
            for entry in dynamic_entries.iter().cloned() {
                match entry.d_tag {
                    abi::DT_NEEDED => {
                        if let Ok(s) = dynstr_table.get(entry.d_val() as usize) {
                            needed.push(String::from(s));
                        }
                    }
                    abi::DT_RPATH | abi::DT_RUNPATH => {
                        if with_rpath {
                            if let Ok(s) = dynstr_table.get(entry.d_val() as usize) {
                                rpath.extend(s.split(":").map(|s| String::from(s)));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let lazy_syms = lazy_bound_symbols(&mut elf, &dynamic_entries)?;

        let symbols: Vec<(Symbol, String)> = match elf.dynamic_symbol_table()? {
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

        let mut undefined = Vec::<String>::new();
        let mut undefined_lazy = Vec::<String>::new();
        for (index, (sym, name)) in symbols.iter().enumerate() {
            if !sym.is_undefined() || sym.st_name == 0 || sym.st_bind() == abi::STB_WEAK {
                continue;
            }
            let symbol = match ver_table
                .as_ref()
                .map(|t| t.get_requirement(index).ok().flatten())
                .flatten()
            {
                Some(ver) => format!("{name}@{}", ver.name),
                None => name.clone(),
            };
            if lazy_syms.contains(&index) {
                undefined_lazy.push(symbol);
            } else {
                undefined.push(symbol);
            }
        }

        return Ok(Self {
            name: String::from(name.as_ref()),
            rpath,
            needed,
            undefined,
            undefined_lazy,
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
            BinaryInfo::parse(&mut content, "sample.bin", true).expect("should not have any error");
        assert_eq!(info.needed[0], "libc.so.6");
    }

    /// The fixture calls `puts`/`abort` through the PLT and does not force eager
    /// binding, so every import lands in the lazy list.
    #[test]
    fn plt_imports_are_lazily_bound() {
        let mut content = Cursor::new(include_bytes!("fixtures/sample.bin"));
        let info =
            BinaryInfo::parse(&mut content, "sample.bin", true).expect("should not have any error");
        assert!(
            info.undefined_lazy.iter().any(|s| s.starts_with("puts@")),
            "puts is called through the PLT, got lazy {:?} eager {:?}",
            info.undefined_lazy,
            info.undefined
        );
        assert!(
            info.undefined.is_empty(),
            "nothing needs eager binding here, got {:?}",
            info.undefined
        );
    }

    /// Taking a function's address emits a second, non-PLT relocation, and the
    /// loader resolves that one while the object loads. Such a symbol must count
    /// as eager even though it also has a PLT entry.
    ///
    /// `sample_eager.bin` is `sample.bin` with the binding of `__gmon_start__`
    /// changed from weak to global (byte 0x1f8, its `st_info` in `.dynsym`:
    /// 0x20 -> 0x10). That symbol already carries both relocations in the
    /// fixture — `R_ARM_JUMP_SLOT` in `.rel.plt` and `R_ARM_GLOB_DAT` in
    /// `.rel.dyn` — but the parser drops weak imports before classifying them,
    /// which hid the verdict.
    #[test]
    fn symbol_with_a_non_plt_relocation_is_eager() {
        let mut content = Cursor::new(include_bytes!("fixtures/sample_eager.bin"));
        let info = BinaryInfo::parse(&mut content, "sample_eager.bin", true)
            .expect("should not have any error");
        assert!(
            info.undefined.iter().any(|s| s == "__gmon_start__"),
            "the GLOB_DAT relocation makes it eager, got eager {:?} lazy {:?}",
            info.undefined,
            info.undefined_lazy
        );
        assert!(
            !info.undefined_lazy.iter().any(|s| s == "__gmon_start__"),
            "must not also be reported as lazy, got {:?}",
            info.undefined_lazy
        );
        // The PLT-only imports are unaffected.
        assert!(
            info.undefined_lazy.iter().any(|s| s.starts_with("puts@")),
            "got lazy {:?}",
            info.undefined_lazy
        );
    }
}

