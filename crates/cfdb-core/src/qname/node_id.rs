#[must_use]
pub fn item_node_id(qname: &str) -> String {
    format!("item:{qname}")
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetDiscriminator {
    Lib,
    Bin { name: String },
}

impl TargetDiscriminator {
    #[must_use]
    pub fn identity<'a>(&self, qname: &'a str) -> std::borrow::Cow<'a, str> {
        match self {
            TargetDiscriminator::Lib => std::borrow::Cow::Borrowed(qname),
            TargetDiscriminator::Bin { name } => {
                std::borrow::Cow::Owned(format!("{qname}#bin:{name}"))
            }
        }
    }

    #[must_use]
    pub fn as_wire_str(&self) -> std::borrow::Cow<'static, str> {
        match self {
            TargetDiscriminator::Lib => std::borrow::Cow::Borrowed("lib"),
            TargetDiscriminator::Bin { name } => std::borrow::Cow::Owned(format!("bin:{name}")),
        }
    }

    #[must_use]
    pub fn from_wire_str(wire: &str) -> Option<Self> {
        match wire {
            "lib" => Some(TargetDiscriminator::Lib),
            _ => wire
                .strip_prefix("bin:")
                .map(|name| TargetDiscriminator::Bin {
                    name: name.to_string(),
                }),
        }
    }

    #[must_use]
    pub fn choose_claim<'a>(claims: &'a [Self], src: &Self) -> Option<&'a Self> {
        claims
            .iter()
            .find(|c| *c == src)
            .or_else(|| claims.iter().find(|c| **c == TargetDiscriminator::Lib))
    }
}

#[must_use]
pub fn matchsite_node_id(caller_identity: &str, matched_path: &str, local_idx: usize) -> String {
    format!("matchsite:{caller_identity}:{matched_path}:{local_idx}")
}

#[must_use]
pub fn item_node_id_for_target(qname: &str, target: &TargetDiscriminator) -> String {
    format!("item:{}", target.identity(qname))
}

#[must_use]
pub fn callsite_node_id(caller_identity: &str, callee_path: &str, local_idx: usize) -> String {
    format!("callsite:{caller_identity}:{callee_path}:{local_idx}")
}

#[must_use]
pub fn entrypoint_node_id(kind: &str, handler_identity: &str) -> String {
    format!("entrypoint:{kind}:{handler_identity}")
}

#[must_use]
pub fn param_node_id(parent_qname: &str, index: usize) -> String {
    format!("param:{parent_qname}#{index}")
}

#[must_use]
pub fn field_node_id(parent_qname: &str, field_name: &str) -> String {
    format!("field:{parent_qname}.{field_name}")
}

#[must_use]
pub fn variant_node_id(enum_qname: &str, index: usize) -> String {
    format!("variant:{enum_qname}#{index}")
}

#[must_use]
pub fn argument_node_id(callsite_id: &str, position: u32) -> String {
    format!("arg:{callsite_id}#{position}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_node_id_prefixes_with_item_colon() {
        assert_eq!(
            item_node_id("cfdb_core::schema::Label"),
            "item:cfdb_core::schema::Label"
        );
    }

    #[test]
    fn item_node_id_is_string_prefix_not_structural() {
        assert_eq!(item_node_id(""), "item:");
    }

    #[test]
    fn param_node_id_disambiguates_by_index_not_name() {
        let parent = "crate::module::fn_name";
        assert_eq!(param_node_id(parent, 0), "param:crate::module::fn_name#0");
        assert_eq!(param_node_id(parent, 1), "param:crate::module::fn_name#1");
        assert_ne!(param_node_id(parent, 0), param_node_id(parent, 1));
    }

    #[test]
    fn field_node_id_formula_is_field_colon_parent_dot_name() {
        assert_eq!(field_node_id("crate::Foo", "bar"), "field:crate::Foo.bar");
    }

    #[test]
    fn field_node_id_handles_tuple_field_name_convention() {
        assert_eq!(field_node_id("crate::Foo", "_0"), "field:crate::Foo._0");
    }

    #[test]
    fn variant_node_id_formula_is_variant_colon_parent_hash_index() {
        assert_eq!(variant_node_id("crate::E", 0), "variant:crate::E#0");
    }

    #[test]
    fn variant_node_id_disambiguates_by_index_not_name() {
        let a = variant_node_id("crate::E", 0);
        let b = variant_node_id("crate::E", 1);
        assert_ne!(a, b);
    }

    #[test]
    fn argument_node_id_formula_is_arg_colon_callsite_hash_position() {
        let cs = "callsite:mod::fn:SomeType::new:0";
        assert_eq!(
            argument_node_id(cs, 0),
            "arg:callsite:mod::fn:SomeType::new:0#0"
        );
        assert_eq!(
            argument_node_id(cs, 1),
            "arg:callsite:mod::fn:SomeType::new:0#1"
        );
    }

    #[test]
    fn argument_node_id_disambiguates_by_position() {
        let cs = "callsite:m::f:g:0";
        assert_ne!(argument_node_id(cs, 0), argument_node_id(cs, 1));
    }

    #[test]
    fn argument_node_id_inherits_callsite_resolver_scope() {
        let syn_cs = "callsite:crate::fn:method:0";
        let hir_cs = "callsite:crate::fn:crate2::ConcreteType::method:0";
        assert_ne!(argument_node_id(syn_cs, 0), argument_node_id(hir_cs, 0));
    }

    #[test]
    fn lib_target_id_is_byte_identical_to_item_node_id() {
        let lib = TargetDiscriminator::Lib;
        assert_eq!(
            item_node_id_for_target("cfdb_core::schema::Label", &lib),
            item_node_id("cfdb_core::schema::Label")
        );
    }

    #[test]
    fn bin_target_id_carries_the_bin_suffix() {
        let bin = TargetDiscriminator::Bin {
            name: "cfdb-recall".to_string(),
        };
        assert_eq!(
            item_node_id_for_target("cfdb_recall::main", &bin),
            "item:cfdb_recall::main#bin:cfdb-recall"
        );
    }

    #[test]
    fn same_named_bin_and_lib_get_distinct_ids() {
        let lib = TargetDiscriminator::Lib;
        let bin = TargetDiscriminator::Bin {
            name: "cfdb-recall".to_string(),
        };
        assert_ne!(
            item_node_id_for_target("cfdb_recall::helper", &lib),
            item_node_id_for_target("cfdb_recall::helper", &bin)
        );
    }

    #[test]
    fn target_wire_strings() {
        assert_eq!(TargetDiscriminator::Lib.as_wire_str(), "lib");
        assert_eq!(
            TargetDiscriminator::Bin {
                name: "alpha".to_string()
            }
            .as_wire_str(),
            "bin:alpha"
        );
    }

    #[test]
    fn callsite_node_id_formula_embeds_the_caller_identity() {
        let bin = TargetDiscriminator::Bin {
            name: "alpha".to_string(),
        };
        let caller_identity = bin.identity("twobins::main");
        assert_eq!(
            callsite_node_id(&caller_identity, "twobins::shared", 0),
            "callsite:twobins::main#bin:alpha:twobins::shared:0"
        );
        assert_eq!(
            callsite_node_id(&TargetDiscriminator::Lib.identity("m::f"), "g", 1),
            "callsite:m::f:g:1"
        );
    }

    #[test]
    fn entrypoint_node_id_formula_embeds_the_handler_identity() {
        assert_eq!(
            entrypoint_node_id(
                "cli_command",
                &TargetDiscriminator::Lib.identity("app::Cli")
            ),
            "entrypoint:cli_command:app::Cli"
        );
        let bin = TargetDiscriminator::Bin {
            name: "alpha".to_string(),
        };
        assert_eq!(
            entrypoint_node_id("cli_command", &bin.identity("twobins::Cli")),
            "entrypoint:cli_command:twobins::Cli#bin:alpha"
        );
    }

    #[test]
    fn derived_ids_inherit_the_discriminated_parent_identity() {
        let bin = TargetDiscriminator::Bin {
            name: "alpha".to_string(),
        };
        let identity = bin.identity("twobins::make");
        assert_eq!(
            param_node_id(&identity, 0),
            "param:twobins::make#bin:alpha#0"
        );
        assert_eq!(
            field_node_id(&identity, "x"),
            "field:twobins::make#bin:alpha.x"
        );
        assert_eq!(
            variant_node_id(&identity, 1),
            "variant:twobins::make#bin:alpha#1"
        );
    }
}
