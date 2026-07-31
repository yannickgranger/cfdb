//! Item-kind vocabulary fixture (#479/#515): one item per kind whose
//! emission history had gaps — `static` (wire value predates its
//! `ItemKind` variant) and `union` (recall listed it aspirationally
//! before the extractor could see one).

pub union RawBits {
    pub as_int: u32,
    pub as_bytes: [u8; 4],
}

pub static ANSWER: u32 = 42;
