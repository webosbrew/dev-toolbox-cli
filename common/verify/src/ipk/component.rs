use bin_lib::BinaryInfo;
use fw_lib::Firmware;
use ipk_lib::Component;

use crate::ipk::ComponentVerifyResult;
use crate::{bin::BinVerifyResult, VerifyWithFirmware};

trait ComponentImpl {
    fn verify_bin(&self, bin: &BinaryInfo, firmware: &Firmware) -> BinVerifyResult;
}

impl<T> ComponentImpl for Component<T> {
    fn verify_bin(&self, bin: &BinaryInfo, firmware: &Firmware) -> BinVerifyResult {
        let mut result = bin.verify(firmware);
        result.missing_lib.retain_mut(|lib| {
            if let Some(lib) = self.find_lib(lib) {
                result.undefined_sym.retain(|sym| !lib.has_symbol(sym));
                return false;
            }
            return true;
        });
        return result;
    }
}

impl<T> VerifyWithFirmware<ComponentVerifyResult> for Component<T> {
    fn verify(&self, firmware: &Firmware) -> ComponentVerifyResult {
        let exe = if let Some(bin) = &self.exe {
            self.verify_bin(bin, firmware)
        } else {
            return ComponentVerifyResult {
                id: self.id.clone(),
                exe: None,
                libs: Default::default(),
            };
        };
        let mut libs: Vec<(bool, BinVerifyResult)> = self
            .libs
            .iter()
            .map(|lib| {
                (
                    self.is_required(lib),
                    self.verify_bin(
                        &BinaryInfo {
                            name: lib.name.clone(),
                            rpath: Default::default(),
                            needed: lib.needed.clone(),
                            undefined: lib.undefined.clone(),
                        },
                        firmware,
                    ),
                )
            })
            .collect();
        libs.sort_by(|(required_a, lib_a), (required_b, lib_b)| {
            let required_cmp = required_a.cmp(required_b);
            if !required_cmp.is_eq() {
                return required_cmp.reverse();
            }
            return lib_a.name.cmp(&lib_b.name);
        });
        return ComponentVerifyResult {
            id: self.id.clone(),
            exe: Some(exe),
            libs,
        };
    }
}
