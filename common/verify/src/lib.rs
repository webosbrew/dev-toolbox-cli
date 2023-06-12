use fw_lib::Firmware;

#[cfg(feature = "bin")]
pub mod bin;
#[cfg(feature = "ipk")]
pub mod ipk;

pub trait VerifyWithFirmware<R> {
    fn verify(&self, firmware: &Firmware) -> R;
}

pub trait VerifyResult {
    fn is_good(&self) -> bool;
}
