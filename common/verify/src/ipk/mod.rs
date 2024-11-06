use bin_lib::LibraryInfo;
use ipk_lib::Package;

use crate::{bin::BinVerifyResult, Verify, VerifyResult};

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

impl Verify<PackageVerifyResult> for Package {
    fn verify<F>(&self, find_library: &F) -> PackageVerifyResult
    where
        F: Fn(&str) -> Option<LibraryInfo>,
    {
        return PackageVerifyResult {
            app: self.app.verify(find_library),
            services: self
                .services
                .iter()
                .map(|svc| svc.verify(find_library))
                .collect(),
        };
    }
}

impl VerifyResult for PackageVerifyResult {
    fn is_good(&self) -> bool {
        return self.app.is_good() && self.services.iter().all(|s| s.is_good());
    }
}
