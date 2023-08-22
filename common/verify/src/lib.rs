use bin_lib::LibraryInfo;

#[cfg(feature = "bin")]
pub mod bin;
#[cfg(feature = "ipk")]
pub mod ipk;

pub trait Verify<R> {
    fn verify<F>(&self, find_library: &F) -> R
    where
        F: Fn(&str) -> Option<LibraryInfo>;
}

pub trait VerifyResult {
    fn is_good(&self) -> bool;
}
