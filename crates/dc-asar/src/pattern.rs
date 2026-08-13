use std::borrow::Cow;

pub fn base_name(rel_path: &str) -> &str {
    rel_path.rsplit('/').next().unwrap_or(rel_path)
}

pub fn matches(pattern: &str, name: &str) -> bool {
    let pattern = normcase(pattern);
    let name = normcase(name);
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    fnmatch(&pattern, &name)
}

pub fn matches_any(patterns: &[String], name: &str) -> bool {
    patterns.iter().any(|pattern| matches(pattern, name))
}

#[cfg(windows)]
fn normcase(text: &str) -> Cow<'_, str> {
    Cow::Owned(text.replace('/', "\\").to_lowercase())
}

#[cfg(not(windows))]
fn normcase(text: &str) -> Cow<'_, str> {
    Cow::Borrowed(text)
}

fn fnmatch(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((&'*', rest)) => {
            let rest = skip_stars(rest);
            (0..=text.len()).any(|index| fnmatch(rest, &text[index..]))
        }
        Some((&'?', rest)) => !text.is_empty() && fnmatch(rest, &text[1..]),
        Some((&'[', rest)) => match CharClass::parse(rest) {
            Some((class, consumed)) => {
                !text.is_empty()
                    && class.contains(text[0])
                    && fnmatch(&rest[consumed..], &text[1..])
            }
            None => !text.is_empty() && text[0] == '[' && fnmatch(rest, &text[1..]),
        },
        Some((expected, rest)) => {
            !text.is_empty() && text[0] == *expected && fnmatch(rest, &text[1..])
        }
    }
}

fn skip_stars(pattern: &[char]) -> &[char] {
    let mut rest = pattern;
    while let Some((&'*', tail)) = rest.split_first() {
        rest = tail;
    }
    rest
}

enum CharClass {
    Never,
    Any,
    Set {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

impl CharClass {
    fn parse(rest: &[char]) -> Option<(CharClass, usize)> {
        let mut end = 0;
        if rest.get(end) == Some(&'!') {
            end += 1;
        }
        if rest.get(end) == Some(&']') {
            end += 1;
        }
        while end < rest.len() && rest[end] != ']' {
            end += 1;
        }
        if end >= rest.len() {
            return None;
        }

        let body = &rest[..end];
        let consumed = end + 1;

        if body.is_empty() {
            return Some((CharClass::Never, consumed));
        }
        if body.len() == 1 && body[0] == '!' {
            return Some((CharClass::Any, consumed));
        }

        let negated = body[0] == '!';
        let body = if negated { &body[1..] } else { body };

        let mut ranges = Vec::new();
        let mut index = 0;
        while index < body.len() {
            if index + 2 < body.len() && body[index + 1] == '-' {
                ranges.push((body[index], body[index + 2]));
                index += 3;
            } else {
                ranges.push((body[index], body[index]));
                index += 1;
            }
        }

        Some((CharClass::Set { negated, ranges }, consumed))
    }

    fn contains(&self, value: char) -> bool {
        match self {
            CharClass::Never => false,
            CharClass::Any => true,
            CharClass::Set { negated, ranges } => {
                let hit = ranges
                    .iter()
                    .any(|(low, high)| value >= *low && value <= *high);
                hit != *negated
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_name_takes_the_last_component() {
        assert_eq!(
            base_name("node_modules/sharp/build/sharp.node"),
            "sharp.node"
        );
        assert_eq!(base_name("native.node"), "native.node");
        assert_eq!(base_name(""), "");
    }

    #[test]
    fn star_matches_any_run_including_separators() {
        assert!(matches("*.node", "native.node"));
        assert!(!matches("*.node", "index.js"));
        assert!(matches("*", ""));
        assert!(matches("a*c", "a/b/c"));
        assert!(matches("**.node", "native.node"));
    }

    #[test]
    fn question_mark_matches_exactly_one_character() {
        assert!(matches("a?c", "abc"));
        assert!(!matches("a?c", "ac"));
        assert!(!matches("a?c", "abbc"));
        assert!(matches("a?c", "a/c"));
    }

    #[test]
    fn character_classes_follow_fnmatch() {
        assert!(matches("[abc].js", "b.js"));
        assert!(!matches("[abc].js", "d.js"));
        assert!(matches("[!abc].js", "d.js"));
        assert!(!matches("[!abc].js", "a.js"));
        assert!(matches("[a-c].js", "b.js"));
        assert!(!matches("[a-c].js", "d.js"));
        assert!(matches("[!a-c]", "d"));
        assert!(matches("[]].js", "].js"));
        assert!(matches("[!]].js", "a.js"));
        assert!(matches("[-a].js", "-.js"));
        assert!(matches("[a-].js", "-.js"));
    }

    #[test]
    fn a_class_that_never_closes_is_a_literal_bracket() {
        assert!(matches("[abc", "[abc"));
        assert!(!matches("[abc", "a"));
        assert!(matches("[]", "[]"));
        assert!(!matches("[]", ""));
        assert!(matches("[!]", "[!]"));
        assert!(!matches("[!]", "x"));
    }

    #[test]
    fn callers_match_against_a_single_component() {
        assert!(matches("*.node", "node_modules/sharp/build/sharp.node"));
        assert!(matches(
            "*.node",
            base_name("node_modules/sharp/build/sharp.node")
        ));
        assert!(!matches("data/*.ks", "first.ks"));
    }

    #[test]
    fn dylib_does_not_match_the_node_pattern() {
        assert!(!matches("*.node", "libsteam_api.dylib"));
        assert!(matches("*.node", "steamworksjs.darwin-arm64.node"));
    }

    #[test]
    fn matches_any_requires_one_hit() {
        let patterns = vec!["*.node".to_string(), "*.dll".to_string()];
        assert!(matches_any(&patterns, "b.dll"));
        assert!(!matches_any(&patterns, "b.so"));
        assert!(!matches_any(&[], "b.node"));
    }
}
