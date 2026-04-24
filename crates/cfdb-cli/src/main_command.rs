//! The `cfdb` CLI subcommand enum. Split out of `main.rs` as part of the
//! #128 god-file split. Moved-only — no variant renames, no signature
//! changes. See `main.rs` for top-level entry, `main_parse.rs` for the
//! two `value_parser` bindings referenced here, and `main_dispatch.rs`
//! for the group-dispatch helpers this enum feeds.
//!
//! The `Command` enum body lives in the `args` submodule (extracted as
//! part of #248); this file re-exports it so all existing callers keep
//! the same `use crate::main_command::Command` import path.

mod args;

pub(crate) use args::{Command, ExtractArgs};
