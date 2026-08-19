use cfdb_lang::LanguageProducer;
use thiserror::Error;

#[allow(clippy::vec_init_then_push)]
pub(crate) fn available_producers() -> Vec<Box<dyn LanguageProducer>> {
    #[cfg_attr(
        not(any(
            feature = "lang-rust",
            feature = "lang-php",
            feature = "lang-typescript"
        )),
        allow(unused_mut)
    )]
    let mut v: Vec<Box<dyn LanguageProducer>> = Vec::new();
    #[cfg(feature = "lang-rust")]
    v.push(Box::new(cfdb_extractor::RustProducer));
    #[cfg(feature = "lang-php")]
    v.push(Box::new(cfdb_extractor_php::PhpProducer));
    #[cfg(feature = "lang-typescript")]
    v.push(Box::new(cfdb_extractor_ts::TypeScriptProducer));
    v
}

#[derive(Debug, Error)]
#[error(
    "no LanguageProducer detected workspace `{workspace}`; \
     compiled-in producers: {compiled_in:?}"
)]
pub struct NoProducerDetected {
    pub workspace: String,
    pub compiled_in: Vec<&'static str>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(
        feature = "lang-rust",
        not(feature = "lang-php"),
        not(feature = "lang-typescript")
    ))]
    fn default_features_register_only_rust_producer() {
        let producers = available_producers();
        let names: Vec<&'static str> = producers.iter().map(|p| p.name()).collect();
        assert_eq!(names, vec!["rust"]);
    }

    #[test]
    #[cfg(all(
        feature = "lang-rust",
        feature = "lang-php",
        feature = "lang-typescript"
    ))]
    fn all_languages_register_in_declaration_order() {
        let producers = available_producers();
        let names: Vec<&'static str> = producers.iter().map(|p| p.name()).collect();
        assert_eq!(names, vec!["rust", "php", "typescript"]);
    }

    #[test]
    #[cfg(not(any(feature = "lang-rust")))]
    fn slim_build_registry_is_empty() {
        let producers = available_producers();
        assert!(
            producers.is_empty(),
            "slim build (no lang-* features) must produce an empty registry"
        );
    }
}
