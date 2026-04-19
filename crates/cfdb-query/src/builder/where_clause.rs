//! `WHERE` predicate builders.

use cfdb_core::{CompareOp, Expr, Predicate};

use super::QueryBuilder;

impl QueryBuilder {
    /// Append `WHERE left = right` (and-ed with any existing predicates).
    pub fn where_eq(self, left: Expr, right: Expr) -> Self {
        self.push_where(Predicate::Compare {
            left,
            op: CompareOp::Eq,
            right,
        })
    }

    /// Append `WHERE left <> right`.
    pub fn where_ne(self, left: Expr, right: Expr) -> Self {
        self.push_where(Predicate::Ne { left, right })
    }

    /// Append `WHERE left < right`.
    pub fn where_lt(self, left: Expr, right: Expr) -> Self {
        self.push_where(Predicate::Compare {
            left,
            op: CompareOp::Lt,
            right,
        })
    }

    /// Append `WHERE left > right`.
    pub fn where_gt(self, left: Expr, right: Expr) -> Self {
        self.push_where(Predicate::Compare {
            left,
            op: CompareOp::Gt,
            right,
        })
    }

    /// Append `WHERE left IN right`.
    pub fn where_in(self, left: Expr, right: Expr) -> Self {
        self.push_where(Predicate::In { left, right })
    }

    /// Append `WHERE left =~ pattern` (regex match).
    pub fn where_regex(self, left: Expr, pattern: Expr) -> Self {
        self.push_where(Predicate::Regex { left, pattern })
    }
}
