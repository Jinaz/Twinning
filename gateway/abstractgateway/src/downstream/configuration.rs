use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub type Id = String;
pub type AnyValue = Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Configuration {
    pub ipaddress: String,
    pub username: String,
    pub password: String,
    ///pub settings: Map<ID, any>
    pub settings: HashMap<Id, AnyValue>,
}
