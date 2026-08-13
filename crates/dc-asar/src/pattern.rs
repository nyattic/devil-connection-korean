pub fn matches(pattern: &str, rel_path: &str) -> bool {
    let target = if pattern.contains('/') {
        rel_path
    } else {
        rel_path.rsplit('/').next().unwrap_or(rel_path)
    };
    glob(pattern.as_bytes(), target.as_bytes())
}

pub fn matches_any(patterns: &[String], rel_path: &str) -> bool {
    patterns.iter().any(|p| matches(p, rel_path))
}

fn glob(pattern: &[u8], text: &[u8]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }

    match pattern[0] {
        b'*' if pattern.get(1) == Some(&b'*') => {
            let rest = &pattern[2..];
            let rest = rest.strip_prefix(b"/").unwrap_or(rest);
            (0..=text.len()).any(|i| glob(rest, &text[i..]))
        }
        b'*' => {
            for i in 0..=text.len() {
                if glob(&pattern[1..], &text[i..]) {
                    return true;
                }
                if text.get(i) == Some(&b'/') {
                    break;
                }
            }
            false
        }
        b'?' => !text.is_empty() && text[0] != b'/' && glob(&pattern[1..], &text[1..]),
        c => !text.is_empty() && text[0] == c && glob(&pattern[1..], &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_base_when_pattern_has_no_slash() {
        assert!(matches("*.node", "native.node"));
        assert!(matches("*.node", "node_modules/sharp/build/sharp.node"));
        assert!(!matches("*.node", "node_modules/sharp/index.js"));
    }

    #[test]
    fn full_path_when_pattern_has_slash() {
        assert!(matches("data/*.ks", "data/first.ks"));
        assert!(!matches("data/*.ks", "data/scenario/first.ks"));
        assert!(matches("data/**/*.ks", "data/scenario/first.ks"));
        assert!(matches("data/**/*.ks", "data/first.ks"));
    }

    #[test]
    fn question_mark_does_not_cross_separator() {
        assert!(matches("a?c", "abc"));
        assert!(!matches("a/?/c", "a//c"));
    }

    #[test]
    fn literal_pattern() {
        assert!(matches("tyrano/tyrano.css", "tyrano/tyrano.css"));
        assert!(!matches("tyrano/tyrano.css", "tyrano/lang.js"));
    }

    #[test]
    fn matches_any_requires_one_hit() {
        let patterns = vec!["*.node".to_string(), "*.dll".to_string()];
        assert!(matches_any(&patterns, "a/b.dll"));
        assert!(!matches_any(&patterns, "a/b.so"));
        assert!(!matches_any(&[], "a/b.node"));
    }
}
