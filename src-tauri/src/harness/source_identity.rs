// @agent-context: Canonical evidence identity and stable final-source dedup.

use std::collections::HashSet;

/// Canonical evidence identity used to collapse repeated final sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIdentity {
    pub id: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
}

impl SourceIdentity {
    pub fn new(id: Option<&str>, url: Option<&str>, source: Option<&str>) -> Self {
        Self {
            id: id.and_then(canonical_id),
            url: url.and_then(canonical_url),
            source: source.and_then(canonical_source),
        }
    }

    /// URL wins because it identifies the evidence across registries. Without
    /// a URL, IDs are scoped by source to avoid cross-provider collisions.
    pub fn dedup_key(&self) -> Option<String> {
        if let Some(url) = self.url.as_deref() {
            return Some(format!("url:{url}"));
        }
        match (self.source.as_deref(), self.id.as_deref()) {
            (Some(source), Some(id)) => Some(format!("source-id:{source}:{id}")),
            (None, Some(id)) => Some(format!("id:{id}")),
            _ => None,
        }
    }
}

/// Canonicalize a source item ID for case/whitespace-insensitive comparison.
pub fn canonical_id(value: &str) -> Option<String> {
    normalize_nonempty(value, ' ')
}

/// Canonicalize a provider/source label so punctuation variants deduplicate.
pub fn canonical_source(value: &str) -> Option<String> {
    let normalized = value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    (!normalized.is_empty()).then_some(normalized)
}

/// Canonicalize an absolute URL for evidence deduplication.
///
/// Scheme and host casing/default ports are normalized by the URL parser.
/// Fragments are dropped, query pairs are sorted, and trailing path slashes
/// are removed without changing path casing.
pub fn canonical_url(value: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(value.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    url.set_fragment(None);

    let mut query_pairs = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    query_pairs.sort_unstable();
    url.set_query(None);
    if !query_pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(query_pairs);
    }

    let trimmed_path = url.path().trim_end_matches('/').to_string();
    url.set_path(if trimmed_path.is_empty() {
        "/"
    } else {
        &trimmed_path
    });

    let mut normalized = url.to_string();
    if url.path() == "/" && url.query().is_none() {
        normalized = normalized.trim_end_matches('/').to_string();
    }
    Some(normalized)
}

/// Preserve first-seen order while dropping repeated evidence identities.
///
/// Items without a usable ID or URL are retained because a source label alone
/// is not a unique item identity.
pub fn stable_dedup_by_identity<T, F>(items: Vec<T>, mut identity: F) -> Vec<T>
where
    F: FnMut(&T) -> SourceIdentity,
{
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| {
            identity(item)
                .dedup_key()
                .is_none_or(|key| seen.insert(key))
        })
        .collect()
}

/// Preserve first-seen order while deduplicating provider/source labels.
pub fn stable_dedup_sources<'a, I>(sources: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut seen = HashSet::new();
    sources
        .into_iter()
        .filter_map(canonical_source)
        .filter(|source| seen.insert(source.clone()))
        .collect()
}

fn normalize_nonempty(value: &str, separator: char) -> Option<String> {
    let normalized = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(&separator.to_string())
        .to_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct Evidence {
        label: &'static str,
        id: Option<&'static str>,
        url: Option<&'static str>,
        source: Option<&'static str>,
    }

    #[test]
    fn canonical_helpers_are_stable() {
        assert_eq!(
            canonical_id("  ARTIFACT:ABC  "),
            Some("artifact:abc".into())
        );
        assert_eq!(canonical_source(" Skills.SH "), Some("skills-sh".into()));
        assert_eq!(
            canonical_url("HTTPS://Example.COM:443/owner/repo/#readme"),
            Some("https://example.com/owner/repo".into())
        );
        assert_eq!(
            canonical_url("https://example.com/search?z=2&a=1"),
            Some("https://example.com/search?a=1&z=2".into())
        );
    }

    #[test]
    fn stable_dedup_prefers_url_then_source_scoped_id() {
        let items = vec![
            Evidence {
                label: "first-url",
                id: Some("one"),
                url: Some("https://EXAMPLE.com/item/#top"),
                source: Some("skills.sh"),
            },
            Evidence {
                label: "duplicate-url",
                id: Some("different"),
                url: Some("https://example.com/item"),
                source: Some("claw-hub"),
            },
            Evidence {
                label: "first-id",
                id: Some(" SHARED "),
                url: None,
                source: Some("Skills.SH"),
            },
            Evidence {
                label: "duplicate-id",
                id: Some("shared"),
                url: None,
                source: Some("skills-sh"),
            },
            Evidence {
                label: "same-id-other-source",
                id: Some("shared"),
                url: None,
                source: Some("claw hub"),
            },
            Evidence {
                label: "unidentified",
                id: None,
                url: None,
                source: Some("skills.sh"),
            },
        ];

        let deduplicated = stable_dedup_by_identity(items, |item| {
            SourceIdentity::new(item.id, item.url, item.source)
        });
        assert_eq!(
            deduplicated
                .iter()
                .map(|item| item.label)
                .collect::<Vec<_>>(),
            vec![
                "first-url",
                "first-id",
                "same-id-other-source",
                "unidentified"
            ]
        );
    }

    #[test]
    fn source_name_dedup_preserves_first_seen_order() {
        assert_eq!(
            stable_dedup_sources(["Skills.SH", "skills-sh", "Claw Hub"].into_iter()),
            vec!["skills-sh", "claw-hub"]
        );
    }
}
