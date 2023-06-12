use fw_lib::Firmware;
use ipk_lib::Package;

use crate::{bin::BinVerifyResult, VerifyWithFirmware};

mod component;

#[derive(Debug)]
pub struct PackageVerifyResult {
    pub app: ComponentVerifyResult,
    pub services: Vec<ComponentVerifyResult>,
}

#[derive(Debug)]
pub struct ComponentVerifyResult {
    pub id: String,
    pub exe: Option<BinVerifyResult>,
    pub libs: Vec<(bool, BinVerifyResult)>,
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
