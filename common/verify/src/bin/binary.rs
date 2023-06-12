use std::collections::HashSet;

use bin_lib::{BinaryInfo, LibraryInfo};
use fw_lib::Firmware;

use crate::bin::BinVerifyResult;
use crate::{VerifyResult, VerifyWithFirmware};

impl VerifyWithFirmware<BinVerifyResult> for BinaryInfo {
    fn verify(&self, firmware: &Firmware) -> BinVerifyResult {
        let mut result = BinVerifyResult::new(self.name.clone());
        result.undefined_sym.extend(self.undefined.clone());
        let mut visited_libs: HashSet<String> = Default::default();

        let find_library = |name: &str| -> Option<LibraryInfo> {
            return firmware
                .find_library(name)
                .or_else(|| self.find_library(name));
        };

        fn resolve_symbols<F>(
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
                if let Some(needed) = lib_resolver(needed) {
                    resolve_symbols(&needed, undefined, visited, lib_resolver);
                }
            }
        }

        for needed in &self.needed {
            if let Some(lib) = find_library(needed) {
                resolve_symbols(
                    &lib,
                    &mut result.undefined_sym,
                    &mut visited_libs,
                    &find_library,
                );
            } else {
                result.missing_lib.push(needed.clone());
            }
        }
        return result;
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
