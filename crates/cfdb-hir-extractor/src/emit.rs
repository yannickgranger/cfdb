use cfdb_core::fact::{Edge, Node};

pub trait CallSiteEmitter {
    type Err;

    fn ingest_resolved_call_sites(
        &mut self,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
    ) -> Result<EmitStats, Self::Err>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmitStats {
    pub call_sites_emitted: usize,
    pub calls_edges_emitted: usize,
    pub invokes_at_edges_emitted: usize,
    pub entry_points_emitted: usize,
    pub exposes_edges_emitted: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_stats_default_is_all_zero() {
        let s = EmitStats::default();
        assert_eq!(s.call_sites_emitted, 0);
        assert_eq!(s.calls_edges_emitted, 0);
        assert_eq!(s.invokes_at_edges_emitted, 0);
        assert_eq!(s.entry_points_emitted, 0);
        assert_eq!(s.exposes_edges_emitted, 0);
    }

    #[test]
    fn emit_stats_equality_is_field_wise() {
        let a = EmitStats {
            call_sites_emitted: 3,
            calls_edges_emitted: 2,
            invokes_at_edges_emitted: 3,
            entry_points_emitted: 4,
            exposes_edges_emitted: 4,
        };
        let b = EmitStats {
            call_sites_emitted: 3,
            calls_edges_emitted: 2,
            invokes_at_edges_emitted: 3,
            entry_points_emitted: 4,
            exposes_edges_emitted: 4,
        };
        assert_eq!(a, b);
    }
}
