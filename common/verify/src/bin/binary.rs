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
        let mut visited_libs: HashSet<String> = Default::default();

        for needed in &self.needed {
            let Some(lib) = find_library(needed) else {
                result.missing_lib.push(needed.clone());
                continue;
            };
            recursive_resolve_symbols(
                &lib,
                &mut result.undefined_sym,
                &mut visited_libs,
                &find_library,
            );
        }
        return result;
    }
}

pub(crate) fn recursive_resolve_symbols<F>(
    lib: &LibraryInfo,
    undefined: &mut Vec<String>,
    visited: &mut HashSet<String>,
    lib_resolver: &F,
) where
    F: Fn(&str) -> Option<LibraryInfo>,
{
    undefined.retain(|symbol| !lib.has_symbol(symbol));
    for needed in &lib.needed {
        if visited.contains(needed) {
            continue;
        }
        visited.insert(needed.clone());
        let Some(needed) = lib_resolver(needed) else {
            continue;
        };
        recursive_resolve_symbols(&needed, undefined, visited, lib_resolver);
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
