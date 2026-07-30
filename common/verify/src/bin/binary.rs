use std::collections::HashSet;

use bin_lib::{BinaryInfo, LibraryInfo};

use crate::bin::BinVerifyResult;
use crate::{Verify, VerifyResult};

impl Verify<BinVerifyResult> for BinaryInfo {
    fn verify<F>(&self, find_library: &F) -> BinVerifyResult
    where
        F: Fn(&str) -> Option<LibraryInfo>,
    {
        let mut result = BinVerifyResult::new(self.name.clone());
        result.undefined_sym.extend(self.undefined.clone());
        result
            .undefined_sym_lazy
            .extend(self.undefined_lazy.clone());
        let mut visited_libs: HashSet<String> = HashSet::new();

        for needed in &self.needed {
            let Some(lib) = find_library(needed) else {
                result.missing_lib.push(needed.clone());
                continue;
            };
            recursive_resolve_symbols(
                &lib,
                &mut result.undefined_sym,
                &mut result.undefined_sym_lazy,
                &mut visited_libs,
                &find_library,
            );
        }
        return result;
    }
}

/// Strike every symbol `lib` (or anything it pulls in) defines off both lists.
/// `lazy` holds the imports that only warn; it walks the same tree, so resolve
/// both in one pass.
pub(crate) fn recursive_resolve_symbols<F>(
    lib: &LibraryInfo,
    undefined: &mut Vec<String>,
    lazy: &mut Vec<String>,
    visited: &mut HashSet<String>,
    lib_resolver: &F,
) where
    F: Fn(&str) -> Option<LibraryInfo>,
{
    undefined.retain(|symbol| !lib.has_symbol(symbol));
    lazy.retain(|symbol| !lib.has_symbol(symbol));
    for needed in &lib.needed {
        if visited.contains(needed) {
            continue;
        }
        visited.insert(needed.clone());
        let Some(needed) = lib_resolver(needed) else {
            continue;
        };
        recursive_resolve_symbols(&needed, undefined, lazy, visited, lib_resolver);
    }
}

impl BinVerifyResult {
    pub fn new(name: String) -> Self {
        return Self {
            name,
            missing_lib: Vec::new(),
            undefined_sym: Vec::new(),
            undefined_sym_lazy: Vec::new(),
        };
    }
}

impl VerifyResult for BinVerifyResult {
    fn is_good(&self) -> bool {
        return self.missing_lib.is_empty() && self.undefined_sym.is_empty();
    }
}
