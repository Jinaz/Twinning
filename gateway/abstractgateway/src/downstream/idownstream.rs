use std::collections::HashMap;

use super::configuration::{Configuration, Id};
use super::method::Method;
use super::property::Property;

/// IDownstream contains configuration + methods + properties
/// (matching your diagram interpretation).
#[derive(Debug, Clone, Default)]
pub struct IDownstream {
    /// Single configuration instance (if you prefer multi-config, switch to HashMap<Id, Configuration>)
    pub configuration: Option<Configuration>,

    /// methods: Map<ID, Method>
    pub methods: HashMap<Id, Method>,

    /// properties: Map<ID, Property>
    pub properties: HashMap<Id, Property>,
}

impl IDownstream {
    pub fn new() -> Self {
        Self::default()
    }

    // -------- configuration --------
    pub fn set_configuration(&mut self, conf: Configuration) {
        self.configuration = Some(conf);
    }

    pub fn configuration(&self) -> Option<&Configuration> {
        self.configuration.as_ref()
    }

    // -------- properties --------
    pub fn upsert_property(&mut self, id: Id, prop: Property) {
        self.properties.insert(id, prop);
    }

    pub fn property(&self, id: &Id) -> Option<&Property> {
        self.properties.get(id)
    }

    pub fn remove_property(&mut self, id: &Id) -> Option<Property> {
        self.properties.remove(id)
    }

    // -------- methods --------
    pub fn upsert_method(&mut self, id: Id, method: Method) {
        self.methods.insert(id, method);
    }

    pub fn method(&self, id: &Id) -> Option<&Method> {
        self.methods.get(id)
    }

    pub fn remove_method(&mut self, id: &Id) -> Option<Method> {
        self.methods.remove(id)
    }
}
