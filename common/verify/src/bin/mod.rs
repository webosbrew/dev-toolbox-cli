pub(crate) mod binary;

#[derive(Debug, Eq, PartialEq)]
pub struct BinVerifyResult {
    pub name: String,
    pub missing_lib: Vec<String>,
    pub undefined_sym: Vec<String>,
}
