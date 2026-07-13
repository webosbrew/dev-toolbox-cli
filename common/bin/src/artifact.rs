//! Lightweight ELF classification for *bundled* artifacts.
//!
//! Unlike [`crate::BinaryInfo`]/[`crate::LibraryInfo`] (which parse the dynamic
//! symbol/needed tables for link verification), this only reads the ELF header
//! and program headers to answer two questions cheaply: is this file an
//! executable or a shared library, and for which architecture? It is used to
//! surface the native binaries a JS service ships alongside its scripts (its own
//! `node`, `ffmpeg`, `.so`s) as supplementary report info.

use std::io::{Read, Seek};

use elf::endian::AnyEndian;
use elf::file::Class;
use elf::{abi, ElfStream};
use serde::{Deserialize, Serialize};

/// Whether a bundled ELF is a program or a shared library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactKind {
    Executable,
    SharedLibrary,
}

impl ArtifactKind {
    pub fn label(self) -> &'static str {
        match self {
            ArtifactKind::Executable => "executable",
            ArtifactKind::SharedLibrary => "shared library",
        }
    }
}

/// A native ELF file bundled inside a component, described just enough for a
/// report line: its path within the component, kind, and architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundledArtifact {
    /// Slash-separated path relative to the component directory.
    pub path: String,
    pub kind: ArtifactKind,
    /// Human-readable architecture (e.g. "ARM (32-bit)"), or `None` if unknown.
    pub arch: Option<String>,
}

impl BundledArtifact {
    /// Classify an ELF stream. Returns `None` for anything that isn't an ELF
    /// executable or shared library (data files, scripts, unreadable files).
    pub fn identify<S, P>(source: S, path: P) -> Option<Self>
    where
        S: Read + Seek,
        P: Into<String>,
    {
        let elf = ElfStream::<AnyEndian, S>::open_stream(source).ok()?;
        let e_type = elf.ehdr.e_type;
        let e_machine = elf.ehdr.e_machine;
        let class = elf.ehdr.class;
        // A `PT_INTERP` program header marks a file that requests an interpreter
        // — i.e. a program to run, not a library to load. This is what tells a
        // PIE executable (also `ET_DYN`) apart from a real shared object.
        let has_interp = elf.segments().iter().any(|ph| ph.p_type == abi::PT_INTERP);
        let kind = match e_type {
            abi::ET_EXEC => ArtifactKind::Executable,
            abi::ET_DYN if has_interp => ArtifactKind::Executable,
            abi::ET_DYN => ArtifactKind::SharedLibrary,
            _ => return None,
        };
        Some(BundledArtifact {
            path: path.into(),
            kind,
            arch: arch_label(e_machine, class),
        })
    }
}

/// Map an ELF machine + class to a readable architecture label.
fn arch_label(machine: u16, class: Class) -> Option<String> {
    let name = match machine {
        abi::EM_ARM => "ARM (32-bit)",
        abi::EM_AARCH64 => "AArch64 (64-bit)",
        abi::EM_386 => "x86",
        abi::EM_X86_64 => "x86-64",
        _ => {
            return Some(match class {
                Class::ELF32 => "unknown (32-bit)".to_string(),
                Class::ELF64 => "unknown (64-bit)".to_string(),
            })
        }
    };
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn identifies_sample_fixture() {
        let mut content = Cursor::new(include_bytes!("fixtures/sample.bin"));
        let a = BundledArtifact::identify(&mut content, "sample.bin").expect("is an ELF");
        // The fixture links libc.so.6, so it's an ELF of some kind with an arch.
        assert!(a.arch.is_some());
    }

    #[test]
    fn rejects_non_elf() {
        let mut content = Cursor::new(b"#!/bin/sh\necho hi\n");
        assert!(BundledArtifact::identify(&mut content, "script.sh").is_none());
    }
}
