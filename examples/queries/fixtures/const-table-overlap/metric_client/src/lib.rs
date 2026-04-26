//! Metric client fixture — `element_type="u32"` table. Pins the
//! cross-element-type filter: even on a hypothetical sha256 collision
//! with a `&str` table, the `a.element_type = b.element_type` filter
//! MUST exclude the cross-type pair. This const carries a numeric set
//! that does not overlap with anything in the fixture.

pub const PORTS: [u32; 2] = [443, 80];
