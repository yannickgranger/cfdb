use std::path::Path;

use cfdb_lang::LanguageError;

use crate::PRODUCER_NAME;

#[derive(Debug, Default)]
pub(crate) struct TestScope {
    roots: Vec<String>,
}

impl TestScope {
    pub(crate) fn from_composer(workspace_root: &Path) -> Result<Self, LanguageError> {
        let manifest = std::fs::read_to_string(workspace_root.join("composer.json"))
            .map_err(LanguageError::Io)?;
        let manifest: serde_json::Value =
            serde_json::from_str(&manifest).map_err(|e| LanguageError::Parse {
                producer: PRODUCER_NAME,
                message: format!("composer.json is not readable JSON: {e}"),
            })?;
        let mut roots = Vec::new();
        let Some(dev) = manifest
            .get("autoload-dev")
            .and_then(serde_json::Value::as_object)
        else {
            return Ok(Self { roots });
        };
        for (section, value) in dev {
            match section.as_str() {
                "psr-4" | "psr-0" => collect_map(value, &mut roots),
                "classmap" | "files" | "exclude-from-classmap" => collect_array(value, &mut roots),
                _ => {}
            }
        }
        roots.sort();
        roots.dedup();
        Ok(Self { roots })
    }

    pub(crate) fn covers(&self, file: &str) -> bool {
        self.roots.iter().any(|root| {
            file == root.trim_end_matches('/')
                || file.starts_with(&format!("{}/", root.trim_end_matches('/')))
        })
    }
}

fn collect_map(value: &serde_json::Value, out: &mut Vec<String>) {
    let Some(map) = value.as_object() else {
        return;
    };
    for path in map.values() {
        collect_scalar_or_array(path, out);
    }
}

fn collect_array(value: &serde_json::Value, out: &mut Vec<String>) {
    collect_scalar_or_array(value, out);
}

fn collect_scalar_or_array(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => push_normalized(s, out),
        serde_json::Value::Array(items) => {
            for item in items {
                if let serde_json::Value::String(s) = item {
                    push_normalized(s, out);
                }
            }
        }
        _ => {}
    }
}

fn push_normalized(raw: &str, out: &mut Vec<String>) {
    let trimmed = raw
        .trim()
        .trim_start_matches("./")
        .trim_matches('/')
        .to_string();
    if !trimmed.is_empty() {
        out.push(trimmed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(roots: &[&str]) -> TestScope {
        TestScope {
            roots: roots.iter().map(|r| (*r).to_string()).collect(),
        }
    }

    #[test]
    fn a_root_covers_its_subtree_and_not_a_sibling_sharing_its_prefix() {
        let scope = scope(&["tests"]);
        assert!(scope.covers("tests/Unit/ThingTest.php"));
        assert!(scope.covers("tests"));
        assert!(!scope.covers("tests-fixtures/Thing.php"));
        assert!(!scope.covers("src/Thing.php"));
    }

    #[test]
    fn a_declared_root_is_normalized_before_it_is_compared() {
        let mut roots = Vec::new();
        push_normalized("./tests/", &mut roots);
        push_normalized("  ", &mut roots);
        assert_eq!(roots, vec!["tests".to_string()]);
    }

    #[test]
    fn a_psr_4_prefix_mapped_to_several_directories_yields_every_one() {
        let mut roots = Vec::new();
        collect_map(
            &serde_json::json!({ "App\\Tests\\": ["tests/", "spec/"] }),
            &mut roots,
        );
        roots.sort();
        assert_eq!(roots, vec!["spec".to_string(), "tests".to_string()]);
    }

    #[test]
    fn a_workspace_declaring_no_dev_autoload_covers_nothing() {
        assert!(!scope(&[]).covers("tests/ServiceTest.php"));
    }
}
