//! Shared parser for the tab-separated dictionary format.

use senti_core::Tag;

/// Error raised when a dictionary file is malformed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct LexiconError {
    pub line: usize,
    pub message: String,
}

impl LexiconError {
    pub(crate) fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

/// One non-comment line split into fields, with its 1-based line number.
pub(crate) struct Record<'a> {
    pub line: usize,
    pub fields: Vec<&'a str>,
}

impl<'a> Record<'a> {
    pub fn field(&self, index: usize, name: &str) -> Result<&'a str, LexiconError> {
        self.fields
            .get(index)
            .map(|f| f.trim())
            .filter(|f| !f.is_empty())
            .ok_or_else(|| LexiconError::new(self.line, format!("missing {name}")))
    }

    pub fn number(&self, index: usize, name: &str) -> Result<u32, LexiconError> {
        self.field(index, name)?
            .parse()
            .map_err(|_| LexiconError::new(self.line, format!("{name} must be a number")))
    }

    /// Optional comma separated tag list.
    pub fn tags(&self, index: usize) -> Result<Vec<Tag>, LexiconError> {
        let Some(raw) = self.fields.get(index).map(|f| f.trim()) else {
            return Ok(Vec::new());
        };
        raw.split(',')
            .filter(|t| !t.trim().is_empty())
            .map(|t| {
                t.parse()
                    .map_err(|e: String| LexiconError::new(self.line, e))
            })
            .collect()
    }
}

/// Iterate over the data lines of a TSV source, skipping blanks and `#` comments.
pub(crate) fn records(source: &str) -> impl Iterator<Item = Record<'_>> {
    source
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|(i, l)| Record {
            line: i + 1,
            fields: l.split('\t').collect(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fields_tags_and_reports_line_numbers() {
        let src = "# comment\n\nhello\t10\tcasual,positive\nbad\tx\n";
        let recs: Vec<_> = records(src).collect();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].line, 3);
        assert_eq!(recs[0].number(1, "freq"), Ok(10));
        assert_eq!(recs[0].tags(2).unwrap(), vec![Tag::Casual, Tag::Positive]);
        assert_eq!(recs[1].number(1, "freq").unwrap_err().line, 4);
        assert!(recs[1].field(5, "x").is_err());
    }
}
