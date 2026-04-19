//! Parameter binding builders (`$name` bindings).

use cfdb_core::{Param, PropValue};

use super::QueryBuilder;

impl QueryBuilder {
    /// Bind `$name` to a scalar `PropValue`.
    pub fn param(mut self, name: impl Into<String>, value: PropValue) -> Self {
        self.params.insert(name.into(), Param::Scalar(value));
        self
    }

    /// Bind `$name` to a list of `PropValue`s.
    pub fn param_list(mut self, name: impl Into<String>, values: Vec<PropValue>) -> Self {
        self.params.insert(name.into(), Param::List(values));
        self
    }
}
