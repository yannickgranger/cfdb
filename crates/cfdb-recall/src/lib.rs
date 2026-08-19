use std::collections::BTreeSet;

pub mod adapters;
pub mod thresholds;

pub use thresholds::{threshold_for_crate, RECALL_THRESHOLD_PER_CRATE, RECALL_THRESHOLD_TOTAL};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PublicItem {
    pub qname: String,
}

impl PublicItem {
    pub fn new(qname: impl Into<String>) -> Self {
        Self {
            qname: qname.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuditList {
    items: BTreeSet<PublicItem>,
}

impl AuditList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_items<I: IntoIterator<Item = PublicItem>>(iter: I) -> Self {
        Self {
            items: iter.into_iter().collect(),
        }
    }

    pub fn contains(&self, item: &PublicItem) -> bool {
        self.items.contains(item)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &BTreeSet<PublicItem> {
        &self.items
    }
}

pub const DEFAULT_THRESHOLD: f64 = 0.95;

#[derive(Debug, Clone, PartialEq)]
pub struct RecallReport {
    pub crate_name: String,
    pub threshold: f64,
    pub total_public: usize,
    pub adjusted_denominator: usize,
    pub matched: usize,
    pub missing: Vec<PublicItem>,
    pub audited: Vec<PublicItem>,
}

impl RecallReport {
    pub fn recall(&self) -> Option<f64> {
        if self.adjusted_denominator == 0 {
            None
        } else {
            Some(self.matched as f64 / self.adjusted_denominator as f64)
        }
    }

    pub fn passes(&self) -> bool {
        match self.recall() {
            None => true,
            Some(r) => r >= self.threshold,
        }
    }
}

pub fn compute_recall(
    crate_name: impl Into<String>,
    public: &BTreeSet<PublicItem>,
    extracted: &BTreeSet<PublicItem>,
    audit: &AuditList,
    threshold: f64,
) -> RecallReport {
    let crate_name = crate_name.into();

    let adjusted: BTreeSet<&PublicItem> = public.iter().filter(|it| !audit.contains(it)).collect();
    let adjusted_denominator = adjusted.len();

    let matched = adjusted
        .iter()
        .filter(|it| extracted.contains(**it))
        .count();

    let mut missing: Vec<PublicItem> = adjusted
        .iter()
        .filter(|it| !extracted.contains(**it))
        .map(|it| (*it).clone())
        .collect();
    missing.sort();

    let mut audited: Vec<PublicItem> = public
        .iter()
        .filter(|it| audit.contains(it))
        .cloned()
        .collect();
    audited.sort();

    RecallReport {
        crate_name,
        threshold,
        total_public: public.len(),
        adjusted_denominator,
        matched,
        missing,
        audited,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(qname: &str) -> PublicItem {
        PublicItem::new(qname)
    }

    fn set<I: IntoIterator<Item = PublicItem>>(items: I) -> BTreeSet<PublicItem> {
        items.into_iter().collect()
    }

    #[test]
    fn recall_is_perfect_when_extracted_matches_public() {
        let public = set([item("c::foo"), item("c::bar")]);
        let extracted = public.clone();
        let audit = AuditList::new();
        let report = compute_recall("c", &public, &extracted, &audit, 0.95);
        assert_eq!(report.matched, 2);
        assert_eq!(report.adjusted_denominator, 2);
        assert_eq!(report.recall(), Some(1.0));
        assert!(report.passes());
        assert!(report.missing.is_empty());
    }

    #[test]
    fn recall_is_zero_when_extracted_is_disjoint() {
        let public = set([item("c::foo"), item("c::bar")]);
        let extracted = set([item("c::baz")]);
        let audit = AuditList::new();
        let report = compute_recall("c", &public, &extracted, &audit, 0.95);
        assert_eq!(report.matched, 0);
        assert_eq!(report.adjusted_denominator, 2);
        assert_eq!(report.recall(), Some(0.0));
        assert!(!report.passes());
    }

    #[test]
    fn recall_is_set_based_not_count_based() {
        let public = set([item("c::a::foo"), item("c::b::foo")]);
        let extracted = set([item("c::a::foo")]);
        let audit = AuditList::new();
        let report = compute_recall("c", &public, &extracted, &audit, 0.95);
        assert_eq!(report.adjusted_denominator, 2);
        assert_eq!(report.matched, 1);
        assert_eq!(report.recall(), Some(0.5));
    }

    #[test]
    fn passes_at_exactly_95_percent() {
        let public: BTreeSet<PublicItem> = (0..20).map(|i| item(&format!("c::f{i}"))).collect();
        let extracted: BTreeSet<PublicItem> = (0..19).map(|i| item(&format!("c::f{i}"))).collect();
        let report = compute_recall("c", &public, &extracted, &AuditList::new(), 0.95);
        assert_eq!(report.recall(), Some(0.95));
        assert!(report.passes());
    }

    #[test]
    fn fails_just_below_95_percent() {
        let public: BTreeSet<PublicItem> = (0..20).map(|i| item(&format!("c::f{i}"))).collect();
        let extracted: BTreeSet<PublicItem> = (0..18).map(|i| item(&format!("c::f{i}"))).collect();
        let report = compute_recall("c", &public, &extracted, &AuditList::new(), 0.95);
        assert_eq!(report.recall(), Some(0.90));
        assert!(!report.passes());
        assert_eq!(report.missing.len(), 2);
    }

    #[test]
    fn audit_list_removes_items_from_denominator() {
        let public = set([item("c::define_id_generated"), item("c::real_fn")]);
        let extracted = set([item("c::real_fn")]);
        let audit = AuditList::from_items([item("c::define_id_generated")]);
        let report = compute_recall("c", &public, &extracted, &audit, 0.95);
        assert_eq!(report.total_public, 2);
        assert_eq!(report.adjusted_denominator, 1);
        assert_eq!(report.matched, 1);
        assert_eq!(report.recall(), Some(1.0));
        assert!(report.passes());
        assert_eq!(report.audited.len(), 1);
    }

    #[test]
    fn audit_list_also_removes_items_from_numerator() {
        let public = set([item("c::audited"), item("c::real")]);
        let extracted = set([item("c::audited"), item("c::real")]);
        let audit = AuditList::from_items([item("c::audited")]);
        let report = compute_recall("c", &public, &extracted, &audit, 0.95);
        assert_eq!(report.adjusted_denominator, 1);
        assert_eq!(report.matched, 1);
        assert_eq!(report.recall(), Some(1.0));
    }

    #[test]
    fn audit_list_never_references_an_item_not_in_public_set() {
        let public = set([item("c::foo")]);
        let extracted = set([item("c::foo")]);
        let audit = AuditList::from_items([item("c::stale_entry")]);
        let report = compute_recall("c", &public, &extracted, &audit, 0.95);
        assert_eq!(report.adjusted_denominator, 1);
        assert_eq!(report.matched, 1);
        assert_eq!(report.recall(), Some(1.0));
        assert!(report.audited.is_empty());
    }

    #[test]
    fn empty_public_set_passes_vacuously() {
        let public = BTreeSet::new();
        let extracted = set([item("c::internal")]);
        let report = compute_recall("c", &public, &extracted, &AuditList::new(), 0.95);
        assert_eq!(report.adjusted_denominator, 0);
        assert_eq!(report.recall(), None);
        assert!(report.passes());
    }

    #[test]
    fn empty_denominator_after_audit_passes_vacuously() {
        let public = set([item("c::generated1"), item("c::generated2")]);
        let extracted = BTreeSet::new();
        let audit = AuditList::from_items([item("c::generated1"), item("c::generated2")]);
        let report = compute_recall("c", &public, &extracted, &audit, 0.95);
        assert_eq!(report.total_public, 2);
        assert_eq!(report.adjusted_denominator, 0);
        assert_eq!(report.recall(), None);
        assert!(report.passes());
    }

    #[test]
    fn missing_items_are_sorted_for_stable_reporting() {
        let public = set([item("c::z_last"), item("c::a_first"), item("c::m_middle")]);
        let extracted = BTreeSet::new();
        let report = compute_recall("c", &public, &extracted, &AuditList::new(), 0.95);
        assert_eq!(
            report
                .missing
                .iter()
                .map(|it| it.qname.as_str())
                .collect::<Vec<_>>(),
            vec!["c::a_first", "c::m_middle", "c::z_last"]
        );
    }

    #[test]
    fn extractor_superset_does_not_affect_recall() {
        let public = set([item("c::pub_fn")]);
        let extracted = set([
            item("c::pub_fn"),
            item("c::private_helper_1"),
            item("c::private_helper_2"),
        ]);
        let report = compute_recall("c", &public, &extracted, &AuditList::new(), 0.95);
        assert_eq!(report.adjusted_denominator, 1);
        assert_eq!(report.matched, 1);
        assert_eq!(report.recall(), Some(1.0));
    }
}
