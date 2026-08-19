use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::PropValue;
use cfdb_core::query::ParamBinding;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ParamResolveError {
    #[error("unknown param form `{form}` — expected one of context / regex / literal / list")]
    UnknownForm { form: String },

    #[error("context `{name}` not declared in .cfdb/concepts/")]
    UnknownContext { name: String },

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

impl From<cfdb_concepts::LoadError> for ParamResolveError {
    fn from(err: cfdb_concepts::LoadError) -> Self {
        match err {
            cfdb_concepts::LoadError::Io { path, source } => Self::Io { path, source },
            cfdb_concepts::LoadError::Toml { path, source } => Self::Toml { path, source },
        }
    }
}

pub(crate) fn resolve_param(
    workspace_root: &Path,
    cli_arg: &str,
) -> Result<(String, ParamBinding), ParamResolveError> {
    let (name, form, value) = split_cli_arg(cli_arg)?;
    let param = match form {
        "context" => resolve_context(workspace_root, value)?,
        "regex" | "literal" => ParamBinding::Scalar(PropValue::Str(value.to_string())),
        "list" => ParamBinding::List(
            value
                .split(',')
                .map(|s| PropValue::Str(s.to_string()))
                .collect(),
        ),
        other => {
            return Err(ParamResolveError::UnknownForm {
                form: other.to_string(),
            });
        }
    };
    Ok((name.to_string(), param))
}

pub(crate) fn resolve_params(
    workspace_root: &Path,
    cli_args: &[String],
) -> Result<BTreeMap<String, ParamBinding>, ParamResolveError> {
    cli_args
        .iter()
        .map(|arg| resolve_param(workspace_root, arg))
        .collect()
}

fn split_cli_arg(cli_arg: &str) -> Result<(&str, &str, &str), ParamResolveError> {
    let mut parts = cli_arg.splitn(3, ':');
    let name = parts.next().unwrap_or("");
    let form = parts.next().unwrap_or("");
    let value = parts.next().unwrap_or("");
    if name.is_empty() || form.is_empty() {
        return Err(ParamResolveError::UnknownForm {
            form: cli_arg.to_string(),
        });
    }
    Ok((name, form, value))
}

fn resolve_context(workspace_root: &Path, wanted: &str) -> Result<ParamBinding, ParamResolveError> {
    let overrides = cfdb_concepts::load_concept_overrides(workspace_root)?;
    if !overrides.declared_contexts().contains_key(wanted) {
        return Err(ParamResolveError::UnknownContext {
            name: wanted.to_string(),
        });
    }
    let mut crates: Vec<String> = overrides
        .crate_assignments()
        .iter()
        .filter(|(_crate_name, meta)| meta.name == wanted)
        .map(|(crate_name, _)| crate_name.clone())
        .collect();
    crates.sort();
    Ok(ParamBinding::List(
        crates.into_iter().map(PropValue::Str).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn propstr(s: &str) -> PropValue {
        PropValue::Str(s.to_string())
    }

    #[test]
    fn regex_form_resolves_to_scalar_str() {
        let tmp = tempdir().unwrap();
        let (name, param) = resolve_param(tmp.path(), "p:regex:^foo.*$").unwrap();
        assert_eq!(name, "p");
        assert_eq!(param, ParamBinding::Scalar(propstr("^foo.*$")));
    }

    #[test]
    fn literal_form_resolves_to_scalar_str() {
        let tmp = tempdir().unwrap();
        let (name, param) = resolve_param(tmp.path(), "q:literal:hello").unwrap();
        assert_eq!(name, "q");
        assert_eq!(param, ParamBinding::Scalar(propstr("hello")));
    }

    #[test]
    fn list_form_preserves_input_order() {
        let tmp = tempdir().unwrap();
        let (name, param) = resolve_param(tmp.path(), "xs:list:alpha,beta,gamma").unwrap();
        assert_eq!(name, "xs");
        assert_eq!(
            param,
            ParamBinding::List(vec![propstr("alpha"), propstr("beta"), propstr("gamma")])
        );
    }

    #[test]
    fn context_form_sorts_crate_names_ascending() {
        let tmp = tempdir().unwrap();
        let concepts = tmp.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts).unwrap();
        fs::write(
            concepts.join("trading.toml"),
            r#"
name = "trading"
crates = ["zeta", "alpha", "mu"]
"#,
        )
        .unwrap();

        let (name, param) = resolve_param(tmp.path(), "ctx:context:trading").unwrap();
        assert_eq!(name, "ctx");
        assert_eq!(
            param,
            ParamBinding::List(vec![propstr("alpha"), propstr("mu"), propstr("zeta")])
        );
    }

    #[test]
    fn context_form_is_deterministic_across_two_calls() {
        let tmp = tempdir().unwrap();
        let concepts = tmp.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts).unwrap();
        fs::write(
            concepts.join("finance.toml"),
            r#"
name = "finance"
crates = ["gamma", "alpha", "beta"]
"#,
        )
        .unwrap();

        let first = resolve_param(tmp.path(), "p:context:finance").unwrap();
        let second = resolve_param(tmp.path(), "p:context:finance").unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn unknown_form_returns_structured_error() {
        let tmp = tempdir().unwrap();
        let err = resolve_param(tmp.path(), "p:exotic:whatever").unwrap_err();
        assert!(
            matches!(&err, ParamResolveError::UnknownForm { form } if form == "exotic"),
            "expected UnknownForm(exotic), got {err:?}"
        );
    }

    #[test]
    fn unknown_context_returns_structured_error() {
        let tmp = tempdir().unwrap();
        let concepts = tmp.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts).unwrap();
        fs::write(
            concepts.join("only.toml"),
            r#"
name = "only"
crates = ["a"]
"#,
        )
        .unwrap();

        let err = resolve_param(tmp.path(), "p:context:nonexistent_ctx").unwrap_err();
        assert!(
            matches!(&err, ParamResolveError::UnknownContext { name } if name == "nonexistent_ctx"),
            "expected UnknownContext(nonexistent_ctx), got {err:?}"
        );
    }

    #[test]
    fn context_with_missing_concepts_dir_returns_unknown_context() {
        let tmp = tempdir().unwrap();
        let err = resolve_param(tmp.path(), "p:context:anything").unwrap_err();
        assert!(
            matches!(&err, ParamResolveError::UnknownContext { name } if name == "anything"),
            "expected UnknownContext(anything), got {err:?}"
        );
    }

    #[test]
    fn malformed_toml_returns_toml_variant() {
        let tmp = tempdir().unwrap();
        let concepts = tmp.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts).unwrap();
        fs::write(concepts.join("broken.toml"), "this is !!! not valid toml").unwrap();

        let err = resolve_param(tmp.path(), "p:context:broken").unwrap_err();
        assert!(
            matches!(&err, ParamResolveError::Toml { .. }),
            "expected Toml(..), got {err:?}"
        );
    }

    #[test]
    fn missing_name_or_form_returns_unknown_form() {
        let tmp = tempdir().unwrap();
        let err = resolve_param(tmp.path(), ":literal:x").unwrap_err();
        assert!(matches!(&err, ParamResolveError::UnknownForm { .. }));
        let err = resolve_param(tmp.path(), "noseparator").unwrap_err();
        assert!(matches!(&err, ParamResolveError::UnknownForm { .. }));
    }

    #[test]
    fn regex_with_colon_in_pattern_is_preserved() {
        let tmp = tempdir().unwrap();
        let (_name, param) = resolve_param(tmp.path(), "p:regex:a:b:c").unwrap();
        assert_eq!(param, ParamBinding::Scalar(propstr("a:b:c")));
    }

    #[test]
    fn resolve_params_collects_multiple_args_into_btreemap() {
        let tmp = tempdir().unwrap();
        let args = vec![
            "p1:literal:one".to_string(),
            "p2:regex:^two$".to_string(),
            "p3:list:a,b".to_string(),
        ];
        let out = resolve_params(tmp.path(), &args).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out["p1"], ParamBinding::Scalar(propstr("one")));
        assert_eq!(out["p2"], ParamBinding::Scalar(propstr("^two$")));
        assert_eq!(
            out["p3"],
            ParamBinding::List(vec![propstr("a"), propstr("b")])
        );
    }

    #[test]
    fn resolve_params_short_circuits_on_first_error() {
        let tmp = tempdir().unwrap();
        let args = vec![
            "p1:literal:ok".to_string(),
            "p2:exotic:bad".to_string(),
            "p3:literal:never_reached".to_string(),
        ];
        let err = resolve_params(tmp.path(), &args).unwrap_err();
        assert!(matches!(err, ParamResolveError::UnknownForm { .. }));
    }

    #[test]
    fn self_dogfood_context_cfdb_resolves_to_expected_crates() {
        let workspace_root = workspace_root_from_manifest();
        let (name, param) =
            resolve_param(&workspace_root, "ctx:context:cfdb").expect("context:cfdb resolves");
        assert_eq!(name, "ctx");
        let items = match param {
            ParamBinding::List(xs) => xs,
            other => panic!("expected ParamBinding::List, got {other:?}"),
        };
        assert!(!items.is_empty(), "cfdb context has >=1 crate");

        let strs: Vec<String> = items
            .iter()
            .map(|v| match v {
                PropValue::Str(s) => s.clone(),
                other => panic!("expected PropValue::Str, got {other:?}"),
            })
            .collect();
        let mut sorted = strs.clone();
        sorted.sort();
        assert_eq!(strs, sorted, "context: form must sort ascending");

        let seed = ["cfdb-cli", "cfdb-concepts", "cfdb-core"];
        for expected in seed {
            assert!(
                strs.iter().any(|s| s == expected),
                "cfdb context must contain {expected}; got {strs:?}"
            );
        }
    }

    fn workspace_root_from_manifest() -> PathBuf {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        Path::new(manifest_dir)
            .parent()
            .expect("crates/ parent")
            .parent()
            .expect("workspace root")
            .to_path_buf()
    }
}
