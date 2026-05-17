//! BDD step-definition target — exercises cucumber-rs attribute
//! detection (`#[given]`, `#[when]`, `#[then]` all classify as
//! `kind="test"` per RFC-042 §3.1, ddd-specialist Finding 3).

#[given("a step")]
fn step_given() {}

#[when("an action")]
fn step_when() {}

#[then("a result")]
fn step_then() {}
