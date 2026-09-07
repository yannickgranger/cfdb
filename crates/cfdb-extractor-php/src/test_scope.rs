use std::path::Path;

use cfdb_lang::LanguageError;

use crate::PRODUCER_NAME;

#[derive(Debug, Default)]
pub(crate) struct ComposerScope {
    production: Vec<String>,
    dev: Vec<String>,
}

impl ComposerScope {
    pub(crate) fn from_composer(workspace_root: &Path) -> Result<Self, LanguageError> {
        let manifest_path = workspace_root.join("composer.json");
        let manifest = match std::fs::read_to_string(&manifest_path) {
            Ok(manifest) => manifest,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(LanguageError::NotDetected {
                    producer: PRODUCER_NAME,
                    reason: format!(
                        "composer.json is absent from {}; detect requires it, and without it the \
                         project declares neither where its code lives nor which of it is test scope",
                        workspace_root.display()
                    ),
                })
            }
            Err(e) => return Err(LanguageError::Io(e)),
        };
        let manifest: serde_json::Value =
            serde_json::from_str(&manifest).map_err(|e| LanguageError::Parse {
                producer: PRODUCER_NAME,
                message: format!("composer.json is not readable JSON: {e}"),
            })?;
        Ok(Self {
            production: collect_section(&manifest, "autoload"),
            dev: collect_section(&manifest, "autoload-dev"),
        })
    }

    pub(crate) fn declared_roots(&self) -> Vec<&str> {
        let mut roots: Vec<&str> = self
            .production
            .iter()
            .chain(self.dev.iter())
            .map(String::as_str)
            .collect();
        roots.sort();
        roots.dedup();
        roots
    }

    pub(crate) fn is_test(&self, file: &str) -> bool {
        self.dev.iter().any(|root| under(file, root))
    }
}

fn under(file: &str, root: &str) -> bool {
    file == root || file.starts_with(&format!("{root}/"))
}

fn collect_section(manifest: &serde_json::Value, section: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(section) = manifest.get(section).and_then(serde_json::Value::as_object) else {
        return out;
    };
    for (key, value) in section {
        match key.as_str() {
            "psr-4" | "psr-0" => collect_map(value, &mut out),
            "classmap" | "files" | "exclude-from-classmap" => {
                collect_scalar_or_array(value, &mut out)
            }
            _ => {}
        }
    }
    out.sort();
    out.dedup();
    out
}

fn collect_map(value: &serde_json::Value, out: &mut Vec<String>) {
    let Some(map) = value.as_object() else {
        return;
    };
    for path in map.values() {
        collect_scalar_or_array(path, out);
    }
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

    fn scope(production: &[&str], dev: &[&str]) -> ComposerScope {
        ComposerScope {
            production: production.iter().map(|r| (*r).to_string()).collect(),
            dev: dev.iter().map(|r| (*r).to_string()).collect(),
        }
    }

    #[test]
    fn a_root_covers_its_subtree_and_not_a_sibling_sharing_its_prefix() {
        let scope = scope(&[], &["tests"]);
        assert!(scope.is_test("tests/Unit/ThingTest.php"));
        assert!(scope.is_test("tests"));
        assert!(!scope.is_test("tests-fixtures/Thing.php"));
        assert!(!scope.is_test("src/Thing.php"));
    }

    #[test]
    fn a_production_root_is_walked_but_is_never_test_scope() {
        let scope = scope(&["src"], &["tests"]);
        assert_eq!(scope.declared_roots(), vec!["src", "tests"]);
        assert!(!scope.is_test("src/Thing.php"));
    }

    #[test]
    fn a_root_declared_in_both_sections_is_walked_once() {
        let scope = scope(&["shared"], &["shared"]);
        assert_eq!(scope.declared_roots(), vec!["shared"]);
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
    fn a_workspace_declaring_nothing_walks_nothing_and_has_no_test_scope() {
        let scope = scope(&[], &[]);
        assert!(scope.declared_roots().is_empty());
        assert!(!scope.is_test("tests/ServiceTest.php"));
    }
}
