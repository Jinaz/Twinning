use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub type Id = String;
pub type AnyValue = Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Method {
    /// variables: Map<ID, any>
    pub variables: HashMap<Id, AnyValue>,
}

/// The diagram says: execute method with parameter `name: ID`.
/// This is the minimal signature; concrete gateways decide what "execute" does.
pub trait ExecuteMethod<GatewayError> {
    fn execute_method(&self, name: &Id) -> Result<(), GatewayError>;
}