//! Shell-style wildcard matching (`*`, `?`).
//!
//! A leaf module with no dependencies beyond `std`, so it can be used from
//! both `ui::panes` (pane selection, `:select`) and `config` (the
//! `[[actions]]` "open with" mapping) without giving `config` a dependency on
//! `ui` — a dependency direction rodeo deliberately keeps one-way.

/// Matches `name` against a shell-style wildcard `pattern` supporting `*`
/// (any sequence, including empty) and `?` (exactly one character).
/// Case-sensitive, like glob.
pub fn wildcard_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();

    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None; // (pattern idx after '*', name idx at '*')

    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi + 1, ni));
            pi += 1;
        } else if let Some((sp, sn)) = star {
            // Backtrack: let '*' consume one more character.
            pi = sp;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }

    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_matches_everything() {
        assert!(wildcard_match("*", "anything.rs"));
        assert!(wildcard_match("*", ""));
    }

    #[test]
    fn extension_pattern() {
        assert!(wildcard_match("*.rs", "main.rs"));
        assert!(!wildcard_match("*.rs", "main.toml"));
    }

    #[test]
    fn question_mark_matches_single_char() {
        assert!(wildcard_match("?.rs", "a.rs"));
        assert!(!wildcard_match("?.rs", "ab.rs"));
    }

    #[test]
    fn prefix_and_suffix() {
        assert!(wildcard_match("foo*", "foobar"));
        assert!(!wildcard_match("foo*", "barfoo"));
        assert!(wildcard_match("*bar", "foobar"));
        assert!(!wildcard_match("*bar", "barfoo"));
    }

    #[test]
    fn middle_star_backtracks() {
        assert!(wildcard_match("f*b*r", "foobar"));
        assert!(wildcard_match("f*b*r", "foobazbar"));
        assert!(!wildcard_match("f*b*r", "foobaz"));
    }

    #[test]
    fn exact_match_required_without_wildcards() {
        assert!(wildcard_match("exact", "exact"));
        assert!(!wildcard_match("exact", "exactly"));
    }

    #[test]
    fn unicode_names() {
        assert!(wildcard_match("*.txt", "日本語.txt"));
        assert!(wildcard_match("日?", "日本"));
    }
}
