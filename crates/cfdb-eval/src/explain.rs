#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExplainHit {
    Indexed,
    Fallback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplainRow {
    pub pattern: String,
    pub hit: ExplainHit,
}

impl ExplainRow {
    pub fn format_line(&self) -> String {
        let arrow = match self.hit {
            ExplainHit::Indexed => "indexed",
            ExplainHit::Fallback => "fallback",
        };
        format!("explain: {} → {arrow}", self.pattern)
    }
}
