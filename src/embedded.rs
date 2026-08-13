#[cfg(feature = "embed-data")]
static TRANSLATION: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/translation.asar"));

pub fn translation() -> Option<&'static [u8]> {
    #[cfg(feature = "embed-data")]
    {
        Some(TRANSLATION)
    }
    #[cfg(not(feature = "embed-data"))]
    {
        None
    }
}
