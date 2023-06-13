use fw_lib::Firmware;
use ipk_lib::Package;

use crate::{bin::BinVerifyResult, VerifyResult, VerifyWithFirmware};

pub mod component;

#[derive(Debug)]
pub struct PackageVerifyResult {
    pub app: ComponentVerifyResult,
    pub services: Vec<ComponentVerifyResult>,
}

#[derive(Debug)]
pub struct ComponentVerifyResult {
    pub id: String,
    pub exe: ComponentBinVerifyResult,
    pub libs: Vec<(bool, ComponentBinVerifyResult)>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ComponentBinVerifyResult {
    Skipped { name: String },
    Ok { name: String },
    Failed(BinVerifyResult),
}

impl VerifyWithFirmware<PackageVerifyResult> for Package {
    fn verify(&self, firmware: &Firmware) -> PackageVerifyResult {
        return PackageVerifyResult {
            app: self.app.verify(firmware),
            services: self
                .services
                .iter()
                .map(|svc| svc.verify(firmware))
                .collect(),
        };
    }
}

impl VerifyResult for PackageVerifyResult {
    fn is_good(&self) -> bool {
        return self.app.is_good() && self.services.iter().all(|s| s.is_good());
    }
}
