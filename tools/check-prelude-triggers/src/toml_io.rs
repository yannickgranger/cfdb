//! TOML-file loading helpers.
//!
//! Pattern shape cloned from `cfdb-concepts/src/lib.rs:165-220` (REUSE-of-shape
//! per prescription D3; NO `cfdb-concepts` dependency per Forbidden move #8).

use std::fs;
use std::path::{Path, PathBuf};

/// Errors surfaced while reading a TOML configuration file.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("toml parse error in {path}: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
}

/// Read a TOML file from disk and parse it into a [`toml::Value`].
///
/// Binary uses dynamic parsing (rather than concrete `#[derive(Deserialize)]`
/// structs) so unknown keys are tolerated — future S0 schema extensions do not
/// break existing trigger handlers. Each handler inspects only the keys it
/// needs.
pub fn read_toml(path: &Path) -> Result<toml::Value, LoadError> {
    let text = fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str::<toml::Value>(&text).map_err(|source| LoadError::Toml {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

/// Read a newline-separated paths file. Blank lines and leading/trailing
/// whitespace are stripped. Returns the list as owned `String`s so downstream
/// handlers can match prefixes without holding a reference to the file text.
pub fn read_changed_paths(path: &Path) -> Result<Vec<String>, LoadError> {
    let text = fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{read_changed_paths, read_toml, LoadError};
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn read_toml_parses_valid_file() {
        let mut f = NamedTempFile::new().expect("tmp");
        writeln!(f, "key = \"value\"").unwrap();
        let val = read_toml(f.path()).expect("parse");
        assert_eq!(val.get("key").and_then(toml::Value::as_str), Some("value"));
    }

    #[test]
    fn read_toml_rejects_malformed_file() {
        let mut f = NamedTempFile::new().expect("tmp");
        writeln!(f, "key =").unwrap();
        let err = read_toml(f.path()).expect_err("malformed");
        matches!(err, LoadError::Toml { .. });
    }

    #[test]
    fn read_toml_surfaces_io_error_for_missing_file() {
        let err = read_toml(std::path::Path::new("/definitely/does/not/exist.toml"))
            .expect_err("missing");
        matches!(err, LoadError::Io { .. });
    }

    #[test]
    fn read_changed_paths_strips_blanks_and_whitespace() {
        let mut f = NamedTempFile::new().expect("tmp");
        write!(f, "crates/a/src/one.rs\n\n  crates/b/src/two.rs  \n\n\n").unwrap();
        let paths = read_changed_paths(f.path()).expect("parse");
        assert_eq!(
            paths,
            vec![
                "crates/a/src/one.rs".to_string(),
                "crates/b/src/two.rs".to_string()
            ]
        );
    }
}
