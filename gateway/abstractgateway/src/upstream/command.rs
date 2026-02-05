use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `any` from the diagram -> JSON value for protocol-agnostic transport/storage.
pub type AnyValue = Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Command {
    /// Payload / arguments of the command.
    pub value: AnyValue,

    /// Type hint (e.g., "StartMotor", "SetSpeed", "bool", "dt.CommandPayload").
    pub ty: String,
}
