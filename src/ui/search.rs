//! Search and filter state: one query language, used everywhere.
//!
//! rodeo used to have two separate inputs — a fuzzy one and a regex one — and
//! the user had to know in advance which one they wanted. There is only one
//! now: a query that uses regex syntax *and* compiles is run as a regex,
//! anything else is matched fuzzily. Typing `main` fuzzy-matches, typing
//! `^main\.rs$` does what it looks like it does.

use crate::ui::textinput::TextInput;

/// Characters that mean the user is writing a regular expression rather than
/// a plain name. `-` and `_` are deliberately absent: they are far more often
/// part of a file name than of a pattern.
const REGEX_SYNTAX: &[char] = &[
    '^', '$', '.', '*', '+', '?', '[', ']', '(', ')', '{', '}', '|', '\\',
];

/// Active filter applied to a pane's entry list, or to a search's candidates.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterSpec {
    Fuzzy(String),
    Regex(String),
}

impl FilterSpec {
    /// Interprets `query` the way the user most likely meant it.
    ///
    /// A query that uses regex syntax but does not compile (`foo(` while it is
    /// still being typed) falls back to fuzzy matching, so the list keeps
    /// updating instead of freezing on an error.
    pub fn detect(query: &str) -> Self {
        if Self::looks_like_regex(query) && regex::Regex::new(query).is_ok() {
            Self::Regex(query.to_string())
        } else {
            Self::Fuzzy(query.to_string())
        }
    }

    /// Whether the query is written as a regular expression.
    pub fn looks_like_regex(query: &str) -> bool {
        query.contains(REGEX_SYNTAX)
    }

    /// `true` when the query reads as a regex but cannot be compiled — worth
    /// showing, because such a query is being matched fuzzily instead.
    pub fn is_broken_regex(query: &str) -> bool {
        Self::looks_like_regex(query) && regex::Regex::new(query).is_err()
    }

    pub fn pattern(&self) -> &str {
        match self {
            Self::Fuzzy(p) | Self::Regex(p) => p,
        }
    }

    /// Short label for the UI: which of the two modes is in force.
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Fuzzy(_) => "fuzzy",
            Self::Regex(_) => "regex",
        }
    }
}

/// A compiled query, reusable across many candidates.
///
/// Building the matcher once and scoring many names is what keeps a search
/// over tens of thousands of paths interactive.
#[derive(Debug)]
pub enum Query {
    /// An empty query: everything matches, in listing order.
    Everything,
    Fuzzy {
        pattern: nucleo::pattern::Pattern,
        matcher: nucleo::Matcher,
    },
    Regex(regex::Regex),
}

impl Query {
    pub fn new(query: &str) -> Self {
        if query.is_empty() {
            return Self::Everything;
        }
        match FilterSpec::detect(query) {
            FilterSpec::Regex(pattern) => match regex::Regex::new(&pattern) {
                Ok(re) => Self::Regex(re),
                // detect() only returns Regex for patterns that compile.
                Err(_) => Self::Everything,
            },
            FilterSpec::Fuzzy(pattern) => Self::Fuzzy {
                pattern: nucleo::pattern::Pattern::parse(
                    &pattern,
                    nucleo::pattern::CaseMatching::Smart,
                    nucleo::pattern::Normalization::Smart,
                ),
                matcher: nucleo::Matcher::new(nucleo::Config::DEFAULT),
            },
        }
    }

    /// Score for `text`, or `None` when it does not match. Higher is better;
    /// regex and empty queries score everything equally, so callers keep the
    /// original order for those.
    pub fn score(&mut self, text: &str) -> Option<u32> {
        match self {
            Self::Everything => Some(0),
            Self::Fuzzy { pattern, matcher } => {
                let mut buf = Vec::new();
                pattern.score(nucleo::Utf32Str::new(text, &mut buf), matcher)
            }
            Self::Regex(re) => re.is_match(text).then_some(0),
        }
    }
}

/// State of the search/filter input bar while it is being edited.
#[derive(Debug, Default)]
pub struct Search {
    pub input: TextInput,
    /// The query looks like a regex but does not compile yet.
    pub regex_invalid: bool,
}

impl Search {
    pub fn new(initial: String) -> Self {
        Self {
            input: TextInput::new(initial),
            regex_invalid: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_word_is_fuzzy_and_a_pattern_is_a_regex() {
        assert_eq!(
            FilterSpec::detect("main"),
            FilterSpec::Fuzzy("main".to_string())
        );
        assert_eq!(
            FilterSpec::detect("^main\\.rs$"),
            FilterSpec::Regex("^main\\.rs$".to_string())
        );
    }

    #[test]
    fn an_unfinished_regex_stays_fuzzy_instead_of_failing() {
        // Typing "(foo" on the way to "(foo|bar)" must not blank the list.
        assert_eq!(
            FilterSpec::detect("(foo"),
            FilterSpec::Fuzzy("(foo".to_string())
        );
        assert!(FilterSpec::is_broken_regex("(foo"));
        assert!(!FilterSpec::is_broken_regex("foo"));
    }

    #[test]
    fn fuzzy_scores_rank_the_better_match_higher() {
        let mut q = Query::new("main");
        // A contiguous match at the start of the name beats one buried in the
        // middle of a longer one, which is what makes the list useful.
        let exact = q.score("main.rs").unwrap();
        let buried = q.score("domain_helper.rs").unwrap();
        assert!(exact > buried, "{exact} vs {buried}");
        assert_eq!(q.score("zzz"), None);
    }

    #[test]
    fn a_regex_query_matches_by_pattern() {
        let mut q = Query::new("^a.*z$");
        assert!(q.score("abcz").is_some());
        assert!(q.score("abc").is_none());
    }

    #[test]
    fn an_empty_query_matches_everything() {
        let mut q = Query::new("");
        assert!(q.score("anything").is_some());
    }
}
