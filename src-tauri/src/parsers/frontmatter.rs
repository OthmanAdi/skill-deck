// @agent-context: Universal YAML frontmatter parser.
// This is the BASE parser that covers ~90% of all agent formats.
// It extracts the YAML block between --- delimiters and the markdown body.
// Specific parsers (skill_md, mdc) call this first, then add format-specific logic.

use anyhow::{Context, Result};
use serde_yaml::Value as YamlValue;

/// Raw result of parsing a file with optional YAML frontmatter.
/// This is the intermediate representation before agent-specific normalization.
#[derive(Debug, Clone)]
pub struct ParsedFrontmatter {
    /// Parsed YAML frontmatter as a dynamic map (None if no frontmatter)
    pub frontmatter: Option<YamlValue>,
    /// The markdown body after the frontmatter block
    pub body: String,
    /// The raw frontmatter string (for debugging / display)
    #[allow(dead_code)]
    pub raw_frontmatter: Option<String>,
}

/// Extracts YAML frontmatter and body from a markdown file.
///
/// Handles three cases:
/// - File starts with `---\n` followed by `---\n` or `---\nEOF` -> has frontmatter
/// - File has no `---` at the start -> no frontmatter, entire file is body
/// - File is empty -> no frontmatter, empty body
pub fn parse_frontmatter(content: &str) -> Result<ParsedFrontmatter> {
    let trimmed = content.trim_start();

    // No frontmatter: file doesn't start with ---
    if !trimmed.starts_with("---") {
        return Ok(ParsedFrontmatter {
            frontmatter: None,
            body: content.to_string(),
            raw_frontmatter: None,
        });
    }

    // Find the closing --- delimiter (skip the opening one)
    let after_opening = &trimmed[3..];
    let after_opening = after_opening.trim_start_matches(['\r', '\n']);

    let closing_pos = after_opening.find("\n---");

    match closing_pos {
        Some(pos) => {
            let yaml_str = &after_opening[..pos];
            let body_start = pos + 4; // skip \n---
            let body = if body_start < after_opening.len() {
                after_opening[body_start..]
                    .trim_start_matches(['\r', '\n'])
                    .to_string()
            } else {
                String::new()
            };

            let frontmatter: YamlValue =
                serde_yaml::from_str(yaml_str).context("Failed to parse YAML frontmatter")?;

            Ok(ParsedFrontmatter {
                frontmatter: Some(frontmatter),
                body,
                raw_frontmatter: Some(yaml_str.to_string()),
            })
        }
        // No closing delimiter — treat entire content as body
        None => Ok(ParsedFrontmatter {
            frontmatter: None,
            body: content.to_string(),
            raw_frontmatter: None,
        }),
    }
}

/// Helper: extract a string field from a YAML mapping.
pub fn yaml_str(value: &YamlValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Helper: extract a bool field from a YAML mapping.
pub fn yaml_bool(value: &YamlValue, key: &str) -> Option<bool> {
    value.get(key).and_then(|v| v.as_bool())
}

/// Helper: extract a string array from a YAML mapping.
/// Handles both `globs: "*.rs"` (single string) and `globs: ["*.rs", "*.toml"]` (array).
pub fn yaml_string_array(value: &YamlValue, key: &str) -> Option<Vec<String>> {
    match value.get(key)? {
        YamlValue::String(s) => Some(vec![s.clone()]),
        YamlValue::Sequence(seq) => {
            let strings: Vec<String> = seq
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if strings.is_empty() {
                None
            } else {
                Some(strings)
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_with_frontmatter() {
        let content = "---\nname: test-skill\ndescription: A test\n---\n# Body\nHello world";
        let result = parse_frontmatter(content).unwrap();
        assert!(result.frontmatter.is_some());
        assert!(result.body.contains("# Body"));
        let fm = result.frontmatter.unwrap();
        assert_eq!(yaml_str(&fm, "name").unwrap(), "test-skill");
    }

    #[test]
    fn test_parse_without_frontmatter() {
        let content = "# Just a markdown file\nNo frontmatter here.";
        let result = parse_frontmatter(content).unwrap();
        assert!(result.frontmatter.is_none());
        assert!(result.body.contains("# Just a markdown file"));
    }

    #[test]
    fn test_parse_empty() {
        let result = parse_frontmatter("").unwrap();
        assert!(result.frontmatter.is_none());
        assert!(result.body.is_empty());
    }

    #[test]
    fn test_yaml_string_array_single() {
        let yaml: YamlValue = serde_yaml::from_str("globs: \"*.rs\"").unwrap();
        let arr = yaml_string_array(&yaml, "globs").unwrap();
        assert_eq!(arr, vec!["*.rs"]);
    }

    #[test]
    fn test_yaml_string_array_multiple() {
        let yaml: YamlValue = serde_yaml::from_str("globs:\n  - \"*.rs\"\n  - \"*.toml\"").unwrap();
        let arr = yaml_string_array(&yaml, "globs").unwrap();
        assert_eq!(arr, vec!["*.rs", "*.toml"]);
    }
}
