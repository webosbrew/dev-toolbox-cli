pub(crate) mod binary;

#[derive(Debug, Eq, PartialEq)]
pub struct BinVerifyResult {
    pub name: String,
    pub missing_lib: Vec<String>,
    pub undefined_sym: Vec<String>,
    /// Undefined symbols the loader binds lazily, on the first call. The binary
    /// still loads without them, so these only warn.
    pub undefined_sym_lazy: Vec<String>,
}

impl BinVerifyResult {
    /// Whether anything is wrong that does not stop the binary from loading.
    pub fn has_warnings(&self) -> bool {
        return !self.undefined_sym_lazy.is_empty();
    }
}
