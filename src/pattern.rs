/// Returns true when `value` matches the simple glob-style `pattern`.
/// Supported wildcards:
/// - `*` matches any substring (including empty)
/// - `?` matches a single character.
pub fn matches_pattern(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    simple_match(pattern.as_bytes(), value.as_bytes())
}

fn simple_match(pat: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_idx = None;
    let mut match_idx = 0;

    while ti < text.len() {
        if pi < pat.len() && (pat[pi] == b'?' || pat[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == b'*' {
            star_idx = Some(pi);
            pi += 1;
            match_idx = ti;
        } else if let Some(star) = star_idx {
            pi = star + 1;
            match_idx += 1;
            ti = match_idx;
        } else {
            return false;
        }
    }

    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }

    pi == pat.len()
}

#[cfg(test)]
mod tests {
    use super::matches_pattern;

    #[test]
    fn wildcard_matching() {
        assert!(matches_pattern("greentic.*", "greentic.repo.build"));
        assert!(matches_pattern("foo?bar", "fooxbar"));
        assert!(matches_pattern("*", "anything"));
        assert!(!matches_pattern("exact", "other"));
        assert!(!matches_pattern("prefix*", "pre"));
    }
}
