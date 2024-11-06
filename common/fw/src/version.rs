use semver::Version;
use serde::{de, Deserialize, Deserializer, Serializer};

pub(crate) fn version_serialize<S>(value: &Version, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

pub(crate) fn version_deserialize<'de, D>(deserializer: D) -> Result<Version, D::Error>
where
    D: Deserializer<'de>,
{
    let str = String::deserialize(deserializer)?;
    Version::parse(&str).map_err(de::Error::custom)
}
