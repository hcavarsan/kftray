use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
pub struct ConfigState {
    pub id: Option<i64>,
    pub config_id: i64,
    pub is_running: bool,
}
