//! The `cfdb` CLI subcommand enum. The `Command` enum body lives in the
//! `args` submodule; this file re-exports it.

mod args;

pub(crate) use args::{Command, ExtractArgs};
