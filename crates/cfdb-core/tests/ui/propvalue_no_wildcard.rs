//! Exhaustive match on `cfdb_core::fact::PropValue` from outside the cfdb-core
//! crate, WITHOUT a wildcard arm. MUST fail to compile under `#[non_exhaustive]`
//! (RFC-044 §3.7 slice 044-G).

use cfdb_core::fact::PropValue;

fn classify(v: PropValue) -> &'static str {
    match v {
        PropValue::Str(_) => "str",
        PropValue::Int(_) => "int",
        PropValue::Float(_) => "float",
        PropValue::Bool(_) => "bool",
        PropValue::Null => "null",
    }
}

fn main() {
    let _ = classify(PropValue::Null);
}
