// @agent-context: Item-count parser. The agent honors user-stated counts as
// hard contracts ("give me 7 skills" → return exactly 7). System prompt
// states this rule too — this regex is the belt + suspenders default.

use regex::Regex;
use std::sync::OnceLock;

static RE: OnceLock<Regex> = OnceLock::new();

fn pattern() -> &'static Regex {
    RE.get_or_init(|| {
        // Allow up to a few words between the number and the noun so natural
        // phrasing like "three rust skills" or "5 good testing examples" still
        // resolves, while the {0,3} bound stops a far-away number from binding
        // to an unrelated noun.
        // expect() is infallible: compile-time literal pattern.
        Regex::new(
            r"(?i)\b(?P<n>\d+|one|two|three|four|five|six|seven|eight|nine|ten|dozen|twelve|fifteen|twenty)\s+(?:[a-z]+\s+){0,3}(skill|example|prompt|result|item|workflow|option|candidate|match)s?\b",
        )
        .expect("intent regex compiles")
    })
}

/// Extract an explicit item count from the user's prompt. Returns Some if a
/// number-near-noun phrase is found.
pub fn extract_item_count(prompt: &str) -> Option<usize> {
    let re = pattern();
    let caps = re.captures(prompt)?;
    let raw = caps.name("n")?.as_str().to_lowercase();
    word_to_number(&raw).or_else(|| raw.parse::<usize>().ok())
}

fn word_to_number(s: &str) -> Option<usize> {
    Some(match s {
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "dozen" | "twelve" => 12,
        "fifteen" => 15,
        "twenty" => 20,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_digits() {
        assert_eq!(extract_item_count("give me 7 skills"), Some(7));
        assert_eq!(extract_item_count("show 12 examples"), Some(12));
    }

    #[test]
    fn extracts_words() {
        assert_eq!(extract_item_count("three rust skills please"), Some(3));
        assert_eq!(extract_item_count("a dozen prompts"), Some(12));
    }

    #[test]
    fn ignores_when_absent() {
        assert_eq!(extract_item_count("what skills do I have for rust"), None);
    }
}
