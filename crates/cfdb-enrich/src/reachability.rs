use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::PropValue;
use cfdb_core::graph::GraphView;
use cfdb_core::schema::{Direction, EdgeLabel, Label};

pub(crate) const VERB: &str = "enrich_reachability";
pub(crate) const ATTR_REACHABLE: &str = "reachable_from_entry";
const ATTR_COUNT: &str = "reachable_entry_count";
const ATTR_REACHABLE_PROD: &str = "reachable_from_production_entry";
const ATTR_COUNT_PROD: &str = "reachable_production_entry_count";

#[derive(Clone, Copy)]
pub(crate) enum ReachabilityFilter {
    All,
    ProductionOnly,
}

impl ReachabilityFilter {
    fn reach_attr(self) -> &'static str {
        match self {
            Self::All => ATTR_REACHABLE,
            Self::ProductionOnly => ATTR_REACHABLE_PROD,
        }
    }

    fn count_attr(self) -> &'static str {
        match self {
            Self::All => ATTR_COUNT,
            Self::ProductionOnly => ATTR_COUNT_PROD,
        }
    }

    fn keep_entry_point(self, kind: Option<&str>) -> bool {
        match self {
            Self::All => true,
            Self::ProductionOnly => !matches!(kind, Some("test") | Some("bench")),
        }
    }
}

pub(crate) fn run(view: &mut dyn GraphView, filter: ReachabilityFilter) -> EnrichReport {
    let entry_points = view.nodes_with_label(&Label::new(Label::ENTRY_POINT));

    if entry_points.is_empty() {
        return EnrichReport {
            verb: VERB.into(),
            ran: false,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![
                "enrich_reachability: no :EntryPoint nodes in keyspace — run `cfdb extract --features hir` first to populate entry points before reachability enrichment".into(),
            ],
        };
    }

    let filtered = filter_entry_points(view, &entry_points, filter);
    let seeds = collect_seeds(view, &filtered);
    let reach_count = accumulate_reach_counts(view, &seeds);
    let bfs_attrs = write_item_attrs(view, &reach_count, filter);

    let attr_call_attrs = crate::attr_call_resolution::mark_serde_default_callees_reachable(
        view,
        filter.reach_attr(),
    );

    EnrichReport {
        verb: VERB.into(),
        ran: true,
        facts_scanned: u64::try_from(entry_points.len()).unwrap_or(u64::MAX),
        attrs_written: bfs_attrs + attr_call_attrs,
        edges_written: 0,
        warnings: Vec::new(),
    }
}

fn filter_entry_points(
    view: &dyn GraphView,
    entry_points: &[String],
    filter: ReachabilityFilter,
) -> Vec<String> {
    entry_points
        .iter()
        .filter(|id| {
            let kind = view
                .node_by_id(id)
                .and_then(|n| n.props.get("kind"))
                .and_then(PropValue::as_str);
            filter.keep_entry_point(kind)
        })
        .cloned()
        .collect()
}

fn collect_seeds(view: &dyn GraphView, entry_points: &[String]) -> BTreeSet<String> {
    entry_points
        .iter()
        .flat_map(|ep_id| exposes_targets(view, ep_id))
        .collect()
}

fn exposes_targets(view: &dyn GraphView, ep_id: &str) -> Vec<String> {
    view.neighbors(ep_id, Direction::Out)
        .into_iter()
        .filter(|(label, _)| label.as_str() == EdgeLabel::EXPOSES)
        .map(|(_, target)| target)
        .collect()
}

struct CallGraph<'v> {
    view: &'v dyn GraphView,
    ids: Vec<String>,
    handles: HashMap<String, u32>,
    successors: Vec<Option<Vec<u32>>>,
}

impl<'v> CallGraph<'v> {
    fn new(view: &'v dyn GraphView) -> Self {
        Self {
            view,
            ids: Vec::new(),
            handles: HashMap::new(),
            successors: Vec::new(),
        }
    }

    fn handle(&mut self, id: &str) -> u32 {
        if let Some(&h) = self.handles.get(id) {
            return h;
        }
        let h = u32::try_from(self.ids.len()).expect("node count fits u32");
        self.ids.push(id.to_string());
        self.handles.insert(id.to_string(), h);
        self.successors.push(None);
        h
    }

    fn id(&self, handle: u32) -> &str {
        &self.ids[handle as usize]
    }

    fn successors(&mut self, handle: u32) -> &[u32] {
        if self.successors[handle as usize].is_none() {
            let targets: Vec<u32> = self
                .view
                .neighbors(self.id(handle), Direction::Out)
                .into_iter()
                .filter(|(label, _)| is_call_graph_edge(label.as_str()))
                .map(|(_, target)| self.handle(&target))
                .collect();
            self.successors[handle as usize] = Some(targets);
        }
        self.successors[handle as usize]
            .as_deref()
            .expect("filled just above")
    }
}

fn accumulate_reach_counts(
    view: &dyn GraphView,
    seeds: &BTreeSet<String>,
) -> BTreeMap<String, i64> {
    let item_label = Label::new(Label::ITEM);
    let mut graph = CallGraph::new(view);
    let mut counts: BTreeMap<u32, i64> = BTreeMap::new();
    for seed in seeds {
        let seed_handle = graph.handle(seed);
        for reached in bfs_call_graph(&mut graph, seed_handle) {
            *counts.entry(reached).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(handle, _)| is_label(view, graph.id(*handle), &item_label))
        .map(|(handle, count)| (graph.id(handle).to_string(), count))
        .collect()
}

fn bfs_call_graph(graph: &mut CallGraph<'_>, seed: u32) -> BTreeSet<u32> {
    let mut visited: BTreeSet<u32> = BTreeSet::new();
    let mut queue: VecDeque<u32> = VecDeque::new();
    visited.insert(seed);
    queue.push_back(seed);
    while let Some(handle) = queue.pop_front() {
        for &target in graph.successors(handle) {
            if visited.insert(target) {
                queue.push_back(target);
            }
        }
    }
    visited
}

fn is_call_graph_edge(label: &str) -> bool {
    label == EdgeLabel::CALLS || label == EdgeLabel::INVOKES_AT
}

fn is_label(view: &dyn GraphView, id: &str, label: &Label) -> bool {
    view.node_by_id(id).is_some_and(|n| n.label == *label)
}

fn write_item_attrs(
    view: &mut dyn GraphView,
    reach_count: &BTreeMap<String, i64>,
    filter: ReachabilityFilter,
) -> u64 {
    let item_ids = view.nodes_with_label(&Label::new(Label::ITEM));
    let reach_attr = filter.reach_attr();
    let count_attr = filter.count_attr();
    let mut count: u64 = 0;
    for id in item_ids {
        let reached = reach_count.get(&id).copied().unwrap_or(0);
        if view.set_attr(&id, reach_attr, PropValue::Bool(reached > 0))
            && view.set_attr(&id, count_attr, PropValue::Int(reached))
        {
            count += 2;
        }
    }
    count
}

#[cfg(test)]
mod tests;
