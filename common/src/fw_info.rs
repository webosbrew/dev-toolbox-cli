use std::fmt::{Display, Formatter};

use crate::FirmwareInfo;

impl FirmwareInfo {
    pub fn list() -> Vec<FirmwareInfo> {
        return Vec::new();
    }
}

impl Display for FirmwareInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("Firmware {}, webOS {}, OTA ID: {}", self.version, self.release,
                                 self.ota_id))
    }
}