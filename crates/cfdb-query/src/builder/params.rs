use cfdb_core::{ParamBinding, PropValue};

use super::QueryBuilder;

impl QueryBuilder {
    pub fn param(mut self, name: impl Into<String>, value: PropValue) -> Self {
        self.params.insert(name.into(), ParamBinding::Scalar(value));
        self
    }

    pub fn param_list(mut self, name: impl Into<String>, values: Vec<PropValue>) -> Self {
        self.params.insert(name.into(), ParamBinding::List(values));
        self
    }
}
