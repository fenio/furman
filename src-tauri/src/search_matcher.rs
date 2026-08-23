use crate::models::FmError;
use regex::{Regex, RegexBuilder};

pub enum SearchMatcher {
    Literal(String),
    Regex(Regex),
}

impl SearchMatcher {
    pub fn new(query: &str, use_regex: bool) -> Result<Self, FmError> {
        if use_regex {
            let regex = RegexBuilder::new(query)
                .case_insensitive(true)
                .build()
                .map_err(|error| FmError::Other(format!("Invalid regular expression: {error}")))?;
            Ok(Self::Regex(regex))
        } else {
            Ok(Self::Literal(query.to_lowercase()))
        }
    }

    pub fn is_match(&self, text: &str) -> bool {
        match self {
            Self::Literal(query) => text.to_lowercase().contains(query),
            Self::Regex(regex) => regex.is_match(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SearchMatcher;

    #[test]
    fn literal_search_is_case_insensitive() {
        let matcher = SearchMatcher::new("report", false).unwrap();

        assert!(matcher.is_match("Annual Report.pdf"));
        assert!(!matcher.is_match("summary.pdf"));
    }

    #[test]
    fn regex_search_is_case_insensitive_and_supports_anchors() {
        let matcher = SearchMatcher::new(r"^img_\d+\.webp$", true).unwrap();

        assert!(matcher.is_match("IMG_42.WEBP"));
        assert!(!matcher.is_match("old_IMG_42.webp"));
    }

    #[test]
    fn invalid_regex_returns_an_error() {
        let error = SearchMatcher::new("[", true).err().unwrap();

        assert!(error.to_string().starts_with("Invalid regular expression:"));
    }
}
