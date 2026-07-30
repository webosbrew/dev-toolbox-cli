use std::collections::HashSet;

use elf::abi;
use elf::dynamic::Dyn;
use elf::endian::EndianParse;
use elf::section::SectionHeader;
use elf::{ElfStream, ParseError};

/// The dynamic symbol indices the loader binds lazily, on the first call.
///
/// A function called through the PLT gets a relocation in the PLT relocation
/// section (`DT_JMPREL`, usually `.rel.plt`). The loader leaves those alone at
/// load time and resolves each one when the call first happens, so a missing
/// symbol only aborts the process if the code path runs. Every other relocation
/// is resolved while the object loads, and a missing symbol there stops the
/// program from starting.
///
/// Two things cancel this. `DT_BIND_NOW`, `DF_BIND_NOW` or `DF_1_NOW` tell the
/// loader to resolve everything up front, and a symbol that also has a non-PLT
/// relocation (its address is taken somewhere) is resolved up front too.
///
/// The result holds symbol table indices, so the caller can classify a symbol
/// without matching names.
pub(crate) fn lazy_bound_symbols<E, S>(
    elf: &mut ElfStream<E, S>,
    dynamic: &[Dyn],
) -> Result<HashSet<usize>, ParseError>
where
    E: EndianParse,
    S: std::io::Read + std::io::Seek,
{
    if binds_now(dynamic) {
        return Ok(HashSet::new());
    }
    let Some(jmprel) = dynamic
        .iter()
        .find(|entry| entry.d_tag == abi::DT_JMPREL)
        .map(Dyn::d_ptr)
    else {
        return Ok(HashSet::new());
    };
    // Copy the headers out: reading a section's data borrows the stream.
    let headers: Vec<SectionHeader> = elf
        .section_headers()
        .iter()
        .filter(|shdr| shdr.sh_type == abi::SHT_REL || shdr.sh_type == abi::SHT_RELA)
        .copied()
        .collect();

    let mut lazy: HashSet<usize> = HashSet::new();
    let mut eager: HashSet<usize> = HashSet::new();
    for shdr in headers {
        let target = if shdr.sh_addr == jmprel {
            &mut lazy
        } else {
            &mut eager
        };
        if shdr.sh_type == abi::SHT_REL {
            for rel in elf.section_data_as_rels(&shdr)? {
                target.insert(rel.r_sym as usize);
            }
        } else {
            for rel in elf.section_data_as_relas(&shdr)? {
                target.insert(rel.r_sym as usize);
            }
        }
    }
    lazy.retain(|index| !eager.contains(index));
    return Ok(lazy);
}

/// `DF_BIND_NOW` and `DF_1_NOW`, typed to match `Dyn::d_val`.
const DF_BIND_NOW: u64 = 0x8;
const DF_1_NOW: u64 = 0x1;

/// Whether the object asks the loader to resolve every symbol at load time.
fn binds_now(dynamic: &[Dyn]) -> bool {
    return dynamic.iter().any(|entry| match entry.d_tag {
        abi::DT_BIND_NOW => true,
        abi::DT_FLAGS => entry.d_val() & DF_BIND_NOW != 0,
        abi::DT_FLAGS_1 => entry.d_val() & DF_1_NOW != 0,
        _ => false,
    });
}
