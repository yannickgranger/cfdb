use cfdb_core::{CompareOp, Expr, Predicate};

use super::QueryBuilder;

impl QueryBuilder {
    pub fn where_eq(self, left: Expr, right: Expr) -> Self {
        self.push_where(Predicate::Compare {
            left,
            op: CompareOp::Eq,
            right,
        })
    }

    pub fn where_ne(self, left: Expr, right: Expr) -> Self {
        self.push_where(Predicate::Ne { left, right })
    }

    pub fn where_lt(self, left: Expr, right: Expr) -> Self {
        self.push_where(Predicate::Compare {
            left,
            op: CompareOp::Lt,
            right,
        })
    }

    pub fn where_gt(self, left: Expr, right: Expr) -> Self {
        self.push_where(Predicate::Compare {
            left,
            op: CompareOp::Gt,
            right,
        })
    }

    pub fn where_in(self, left: Expr, right: Expr) -> Self {
        self.push_where(Predicate::In { left, right })
    }

    pub fn where_regex(self, left: Expr, pattern: Expr) -> Self {
        self.push_where(Predicate::Regex { left, pattern })
    }
}
