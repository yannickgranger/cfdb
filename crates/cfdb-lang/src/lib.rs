use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node};

pub trait LanguageProducer: Send {
    fn name(&self) -> &'static str;

    fn detect(&self, workspace_root: &Path) -> bool;

    fn produce(&self, workspace_root: &Path) -> Result<(Vec<Node>, Vec<Edge>), LanguageError>;
}

#[derive(Debug, thiserror::Error)]
pub enum LanguageError {
    #[error("workspace root not detected by producer `{producer}`: {reason}")]
    NotDetected {
        producer: &'static str,
        reason: String,
    },

    #[error("workspace root I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("producer-specific parse failure in `{producer}`: {message}")]
    Parse {
        producer: &'static str,
        message: String,
    },

    #[error(
        "producer `{producer}`: file `{file}` is outside the canonical \
         workspace root `{workspace_root}` — refusing to emit a \
         non-workspace-relative `file` prop"
    )]
    PathNotInWorkspace {
        producer: &'static str,
        file: PathBuf,
        workspace_root: PathBuf,
    },
}

pub fn canonical_workspace_root(workspace_root: &Path) -> Result<PathBuf, LanguageError> {
    Ok(workspace_root.canonicalize()?)
}

pub fn workspace_relative(
    file_path: &Path,
    canonical_root: &Path,
    producer: &'static str,
) -> Result<String, LanguageError> {
    Ok(file_path
        .strip_prefix(canonical_root)
        .map_err(|_| LanguageError::PathNotInWorkspace {
            producer,
            file: file_path.to_path_buf(),
            workspace_root: canonical_root.to_path_buf(),
        })?
        .to_string_lossy()
        .into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_producer_is_object_safe() {
        fn _assert_obj_safe(_: &dyn LanguageProducer) {}
    }

    #[test]
    fn language_producer_is_send() {
        fn _assert_send_box(_: Box<dyn LanguageProducer>)
        where
            Box<dyn LanguageProducer>: Send,
        {
        }
    }

    #[test]
    fn workspace_relative_strips_the_canonical_root() {
        let root = Path::new("/ws/project");
        let rel = workspace_relative(Path::new("/ws/project/src/a.php"), root, "php")
            .expect("in-workspace path must resolve");
        assert_eq!(rel, "src/a.php");
    }

    #[test]
    fn workspace_relative_refuses_paths_outside_the_root() {
        let root = Path::new("/ws/project");
        let err = workspace_relative(Path::new("/elsewhere/b.ts"), root, "typescript")
            .expect_err("out-of-workspace path must be a loud error, never a silent absolute");
        assert!(matches!(
            err,
            LanguageError::PathNotInWorkspace {
                producer: "typescript",
                ..
            }
        ));
    }

    #[test]
    fn canonical_workspace_root_refuses_missing_roots() {
        let err = canonical_workspace_root(Path::new("definitely/not/a/dir"))
            .expect_err("missing root must error");
        assert!(matches!(err, LanguageError::Io(_)));
    }
}
