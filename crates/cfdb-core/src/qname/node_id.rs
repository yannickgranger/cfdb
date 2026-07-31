//! Canonical per-label node-id formulas for `cfdb`'s fact graph.
//!
//! Each function produces the stable node-id for one label kind. Both
//! the syn-based `cfdb-extractor` and the HIR-based
//! `cfdb-hir-extractor` route through these helpers so cross-extractor
//! edges always target the same node id (RFC-037 §3.8 B8 canonical id
//! helpers).
//!
//! All functions are pure: values in → values out, zero I/O, zero
//! allocations beyond the return `String`.

/// Wrap a qname into the canonical `:Item` node-id form
/// (`item:<qname>`). This prefix is the graph-level convention used
/// by every edge whose source or target is an Item.
///
/// RFC-054 §3.1: this is the LIB-target formula. Bin-target items get a
/// `#bin:{name}` identity suffix via [`item_node_id_for_target`] —
/// producers that know their target route through that instead.
#[must_use]
pub fn item_node_id(qname: &str) -> String {
    format!("item:{qname}")
}

/// Which cargo build target an item was walked from (RFC-054 §3.1).
///
/// A plain domain type — deliberately NOT a re-export of
/// `cargo_metadata`/`ra_ap` types (cfdb-core stays dependency-free; the
/// producers convert at their boundary). Distinct targets of one package
/// occupy distinct identity namespaces: a lib and a same-named bin are two
/// crates in rustc's model with no textual path distinction, so identity
/// needs this discriminator — no spelling of the path alone is sufficient.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetDiscriminator {
    /// The package's `[lib]` target — identity is the bare qname
    /// (byte-stable vs pre-RFC-054 ids, keeping the overwhelming majority
    /// of every keyspace, rule, and the cross-dogfood contract untouched).
    Lib,
    /// A `[[bin]]` target; `name` is the cargo target name verbatim
    /// (dashes kept).
    Bin {
        /// Cargo target name as `cargo metadata` reports it.
        name: String,
    },
}

impl TargetDiscriminator {
    /// The identity string for `qname` under this target: the bare qname
    /// for [`Self::Lib`], `{qname}#bin:{name}` for [`Self::Bin`]. `#`
    /// after the path is the established non-path-metadata discriminator
    /// (`param:…#0`, `variant:…#1`, `arg:…#0` — same module). Children
    /// (`param:`/`field:`/`variant:`/`callsite:`/`arg:`) derive from this
    /// identity, not from the bare qname, so they separate for free.
    #[must_use]
    pub fn identity(&self, qname: &str) -> String {
        // RED stub (#557): bin suffix lands with green.
        let _ = self;
        qname.to_string()
    }

    /// Wire value for the `:Item.target` prop: `"lib"` or `"bin:{name}"`.
    /// A plain wire string, not an enum in the schema — same convention as
    /// the `kind` and `resolver` props (RFC-054 §3.2).
    #[must_use]
    pub fn as_wire_str(&self) -> String {
        match self {
            TargetDiscriminator::Lib => "lib".to_string(),
            TargetDiscriminator::Bin { name } => format!("bin:{name}"),
        }
    }
}

/// Canonical `:Item` node id under a target discriminator (RFC-054 §3.1):
/// `item:{qname}` for lib targets (byte-identical to [`item_node_id`]),
/// `item:{qname}#bin:{name}` for bin targets. The suffix position (not a
/// prefix rewrite) keeps `item:` ids prefix-searchable.
#[must_use]
pub fn item_node_id_for_target(qname: &str, target: &TargetDiscriminator) -> String {
    format!("item:{}", target.identity(qname))
}

/// Canonical `:CallSite` node id (RFC-054 §3.1 — centralizing a shape that
/// was previously duplicated as ad-hoc `format!` calls across producers,
/// which is exactly the #517 split-brain class). `caller_identity` is the
/// discriminated identity from [`TargetDiscriminator::identity`], NOT the
/// bare display qname — so syntactically identical calls in sibling bins
/// get distinct ids (#542's measured 20→1 `:CallSite` collapse).
#[must_use]
pub fn callsite_node_id(caller_identity: &str, callee_path: &str, local_idx: usize) -> String {
    format!("callsite:{caller_identity}:{callee_path}:{local_idx}")
}

/// Canonical `:Param` node-id form (#209, RFC-036 §3.1 CP1).
///
/// Parameters of the same fn are disambiguated by positional index,
/// never by name — a fn can legitimately have two params named `_`
/// (wildcard patterns) and must still get distinct node ids.
/// Extractors (syn-based today, HIR-based tomorrow) must route
/// through this function so cross-extractor `HAS_PARAM` /
/// `REGISTERS_PARAM` edges target the same node id.
#[must_use]
pub fn param_node_id(parent_qname: &str, index: usize) -> String {
    format!("param:{parent_qname}#{index}")
}

/// Canonical node id for a `:Field`. Both extractors (syn-based today,
/// HIR-based tomorrow) route through this function so cross-extractor
/// `HAS_FIELD` / `REGISTERS_PARAM` edges target the same node id.
/// Mirrors the #209 resolution of the `:Param` id split-brain for
/// `:Field`.
#[must_use]
pub fn field_node_id(parent_qname: &str, field_name: &str) -> String {
    format!("field:{parent_qname}.{field_name}")
}

/// Canonical node id for a `:Variant`. Index-based to mirror
/// `param_node_id` — positionally stable within a single extract;
/// variant reordering produces a new id (delete + recreate in diffs),
/// accepted per the same tradeoff as `param_node_id`.
#[must_use]
pub fn variant_node_id(enum_qname: &str, index: usize) -> String {
    format!("variant:{enum_qname}#{index}")
}

/// Canonical node id for an `:Argument` (RFC-043 Slice A).
///
/// Derives from `callsite_id` so argument ids inherit the resolver scope
/// of their parent `:CallSite` (RFC-043 §4 Invariant 9 — RFC-032 §3
/// resolver-discriminator contract). Deleting all nodes with id prefix
/// `arg:{callsite_id}#` removes all children of the given call site
/// (RFC-043 §4 Invariant 8 lifecycle coupling).
///
/// Both extractors (syn-based and HIR-based) route through this function
/// so the id shape is stable and testable independently of the emitter.
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
        // Degenerate but valid — the wrapper is a pure string operation,
        // not a parser. Empty qname is illegal-but-representable.
        assert_eq!(item_node_id(""), "item:");
    }

    #[test]
    fn param_node_id_disambiguates_by_index_not_name() {
        // A fn with two wildcard params must produce two distinct
        // node ids — the index is load-bearing, name alone is not.
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
        // Tuple fields use "_0", "_1", ... by convention (RFC-037 §3.3).
        assert_eq!(field_node_id("crate::Foo", "_0"), "field:crate::Foo._0");
    }

    #[test]
    fn variant_node_id_formula_is_variant_colon_parent_hash_index() {
        assert_eq!(variant_node_id("crate::E", 0), "variant:crate::E#0");
    }

    #[test]
    fn variant_node_id_disambiguates_by_index_not_name() {
        // Two variants with the same position-0 index but different names
        // would collide under a name-based scheme. Index-based avoids it.
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
        // Syn and HIR call sites have different cs_ids by design (RFC-032 §3).
        // Their argument children inherit that difference.
        let syn_cs = "callsite:crate::fn:method:0";
        let hir_cs = "callsite:crate::fn:crate2::ConcreteType::method:0";
        assert_ne!(argument_node_id(syn_cs, 0), argument_node_id(hir_cs, 0));
    }

    // --- RFC-054 §3.1 (#557) target-scoped identity formulas ---

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
        // The case target-name-only prefixes cannot fix: cfdb-recall's bin
        // is NAMED cfdb-recall, underscored equal to the lib crate name.
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
        // Mirrors argument_node_id's formula pin. The discriminated caller
        // identity is what separates syntactically identical calls in
        // sibling bins (#542's 20->1 CallSite collapse).
        let bin = TargetDiscriminator::Bin {
            name: "alpha".to_string(),
        };
        let caller_identity = bin.identity("twobins::main");
        assert_eq!(
            callsite_node_id(&caller_identity, "twobins::shared", 0),
            "callsite:twobins::main#bin:alpha:twobins::shared:0"
        );
        // Lib callers stay byte-stable vs the pre-054 ad-hoc format.
        assert_eq!(
            callsite_node_id(&TargetDiscriminator::Lib.identity("m::f"), "g", 1),
            "callsite:m::f:g:1"
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

