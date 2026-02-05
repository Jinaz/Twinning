use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `any` from the diagram -> JSON value for protocol-agnostic transport/storage.
pub type AnyValue = Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Property {
    /// Current value of the property.
    pub value: AnyValue,

    /// Type hint (e.g., "float", "bool", "string", "dt.Measurement").
    /// Kept as string to stay transport-agnostic and allow custom domain types.
    pub ty: String,
}
