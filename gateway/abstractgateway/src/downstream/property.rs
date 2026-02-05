use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type AnyValue = Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Property {
    /// `type` in the diagram (use `ty` in Rust to avoid the reserved keyword)
    pub ty: String,
    pub value: AnyValue,
}
