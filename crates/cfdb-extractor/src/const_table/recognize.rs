use syn::{
    Expr, ExprArray, ExprLit, ExprReference, Lit, Type, TypeArray, TypeReference, TypeSlice,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElementType {
    Str,
    U32,
    I32,
    U64,
    I64,
}

impl ElementType {
    pub(crate) fn as_wire_str(&self) -> &'static str {
        match self {
            ElementType::Str => "str",
            ElementType::U32 => "u32",
            ElementType::I32 => "i32",
            ElementType::U64 => "u64",
            ElementType::I64 => "i64",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EntryValue {
    Str(String),
    Num(i128),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecognizedConstTable {
    pub(crate) qname: String,
    pub(crate) name: String,
    pub(crate) crate_name: String,
    pub(crate) module_qpath: String,
    pub(crate) element_type: ElementType,
    pub(crate) entries: Vec<EntryValue>,
    pub(crate) is_test: bool,
}

pub(crate) fn recognize_const_table(
    node: &syn::ItemConst,
    crate_name: &str,
    module_qpath: &str,
    is_test: bool,
) -> Option<RecognizedConstTable> {
    let element_type = element_type_of(&node.ty)?;
    let entries = entries_from_expr(&node.expr, element_type)?;
    let name = node.ident.to_string();
    let qname = build_qname(crate_name, module_qpath, &name);
    Some(RecognizedConstTable {
        qname,
        name,
        crate_name: crate_name.to_string(),
        module_qpath: module_qpath.to_string(),
        element_type,
        entries,
        is_test,
    })
}

fn element_type_of(ty: &Type) -> Option<ElementType> {
    let inner = match ty {
        Type::Reference(TypeReference { elem, .. }) => match elem.as_ref() {
            Type::Slice(TypeSlice { elem: inner, .. })
            | Type::Array(TypeArray { elem: inner, .. }) => inner.as_ref(),
            _ => return None,
        },
        Type::Array(TypeArray { elem: inner, .. }) => inner.as_ref(),
        _ => return None,
    };
    classify_element(inner)
}

fn classify_element(ty: &Type) -> Option<ElementType> {
    if let Type::Reference(TypeReference { elem, .. }) = ty {
        if path_is_ident(elem.as_ref(), "str") {
            return Some(ElementType::Str);
        }
        return None;
    }
    if path_is_ident(ty, "u32") {
        return Some(ElementType::U32);
    }
    if path_is_ident(ty, "i32") {
        return Some(ElementType::I32);
    }
    if path_is_ident(ty, "u64") {
        return Some(ElementType::U64);
    }
    if path_is_ident(ty, "i64") {
        return Some(ElementType::I64);
    }
    None
}

fn path_is_ident(ty: &Type, ident: &str) -> bool {
    if let Type::Path(p) = ty {
        if p.qself.is_none() && p.path.segments.len() == 1 {
            return p.path.segments[0].ident == ident;
        }
    }
    false
}

fn entries_from_expr(expr: &Expr, expected: ElementType) -> Option<Vec<EntryValue>> {
    let array = match expr {
        Expr::Reference(ExprReference { expr: inner, .. }) => match inner.as_ref() {
            Expr::Array(a) => a,
            _ => return None,
        },
        Expr::Array(a) => a,
        _ => return None,
    };
    parse_literal_entries(array, expected)
}

fn parse_literal_entries(array: &ExprArray, expected: ElementType) -> Option<Vec<EntryValue>> {
    let mut out = Vec::with_capacity(array.elems.len());
    for elem in &array.elems {
        let lit = match elem {
            Expr::Lit(ExprLit { lit, .. }) => lit,
            _ => return None,
        };
        out.push(parse_literal(lit, expected)?);
    }
    Some(out)
}

fn parse_literal(lit: &Lit, expected: ElementType) -> Option<EntryValue> {
    match (expected, lit) {
        (ElementType::Str, Lit::Str(s)) => Some(EntryValue::Str(s.value())),
        (ElementType::U32, Lit::Int(n))
        | (ElementType::I32, Lit::Int(n))
        | (ElementType::U64, Lit::Int(n))
        | (ElementType::I64, Lit::Int(n)) => n.base10_parse::<i128>().ok().map(EntryValue::Num),
        _ => None,
    }
}

fn build_qname(crate_name: &str, module_qpath: &str, name: &str) -> String {
    if module_qpath.is_empty() {
        format!("{crate_name}::{name}")
    } else {
        format!("{module_qpath}::{name}")
    }
}
