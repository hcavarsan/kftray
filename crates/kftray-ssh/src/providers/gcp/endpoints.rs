use super::consts::{
    COMPUTE,
    IAP_WS,
    OSLOGIN,
    TOKENINFO,
};

/// Base URLs of every Google service the provider talks to.
#[derive(Clone, Debug)]
#[cfg_attr(
    any(feature = "testing", feature = "e2e"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct GcpEndpoints {
    pub compute: String,
    pub oslogin: String,
    pub tokeninfo: String,
    pub iap_ws: String,
}

impl Default for GcpEndpoints {
    fn default() -> Self {
        Self {
            compute: COMPUTE.into(),
            oslogin: OSLOGIN.into(),
            tokeninfo: TOKENINFO.into(),
            iap_ws: IAP_WS.into(),
        }
    }
}
