//! Search queries (FEATURES.md F-SRCH-02): words, wildcards and operators.
//!
//! - A word matches names containing it, ignoring case: `report`.
//! - `*` and `?` make a wildcard over the whole name: `*.pdf`, `img_????.jpg`.
//! - Quotes keep spaces: `"annual report"`.
//! - `ext:pdf` (or `ext:pdf,docx`), `size:>10mb` / `size:<1kb` / `size:10mb` (at least),
//!   `modified:7d` (within the last 7 days; `h`, `d`, `w`, `m` = 30 days, `y` = 365 days),
//!   `kind:folder` / `kind:file`.
//!
//! All parts must match. Parsing never panics; a bad operator value is an error that names it.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum QueryError {
    #[error("\"{0}\" is not a size, like 10mb or 500kb")]
    Size(String),
    #[error("\"{0}\" is not an age, like 7d or 12h")]
    Age(String),
    #[error("\"{0}\" is not a kind: use folder or file")]
    Kind(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindFilter {
    Folder,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeFilter {
    AtLeast(u64),
    Above(u64),
    Below(u64),
}

/// One word of the query, lowercased.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Term {
    Contains(String),
    Wildcard(Vec<char>),
}

/// A parsed query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    terms: Vec<Term>,
    /// Lowercase extensions without the dot.
    ext: Vec<String>,
    size: Option<SizeFilter>,
    /// Modified within this many 100 ns ticks before "now".
    modified_within: Option<i64>,
    kind: Option<KindFilter>,
}

/// What a query is matched against.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub name: &'a str,
    pub is_folder: bool,
    pub size: u64,
    /// FILETIME ticks (100 ns since 1601).
    pub modified: i64,
}

const TICKS_PER_HOUR: i64 = 10_000_000 * 3600;

fn split(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    for ch in input.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

fn parse_size(raw: &str) -> Option<u64> {
    let raw = raw.trim().to_lowercase();
    let digits = raw
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(raw.len());
    let (number, unit) = raw.split_at(digits);
    let number: f64 = number.parse().ok()?;
    let scale: f64 = match unit {
        "" | "b" => 1.0,
        "kb" | "k" => 1024.0,
        "mb" | "m" => 1024.0 * 1024.0,
        "gb" | "g" => 1024.0 * 1024.0 * 1024.0,
        "tb" | "t" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    let bytes = number * scale;
    // In range checked just before: the cast cannot truncate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole = bytes as u64;
    (bytes.is_finite() && (0.0..1.8e19).contains(&bytes)).then_some(whole)
}

fn parse_age(raw: &str) -> Option<i64> {
    let raw = raw.trim().to_lowercase();
    let split = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    let (number, unit) = raw.split_at(split);
    let number: i64 = number.parse().ok()?;
    let hours = match unit {
        "h" => 1,
        "" | "d" => 24,
        "w" => 24 * 7,
        "m" => 24 * 30,
        "y" => 24 * 365,
        _ => return None,
    };
    number.checked_mul(hours)?.checked_mul(TICKS_PER_HOUR)
}

/// Case-insensitive wildcard match over the whole name (`*` any run, `?` one character).
fn wildcard(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
            }
            Some(&c) if c == '?' || c == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    p = sp + 1;
                    n = sn + 1;
                    star = Some((sp, sn + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

impl Query {
    pub fn parse(input: &str) -> Result<Self, QueryError> {
        let mut query = Self::default();
        for word in split(input) {
            let lower = word.to_lowercase();
            if let Some(value) = lower.strip_prefix("ext:") {
                query.ext.extend(
                    value
                        .split(',')
                        .map(|e| e.trim().trim_start_matches('.').to_string())
                        .filter(|e| !e.is_empty()),
                );
            } else if let Some(value) = lower.strip_prefix("size:") {
                let bad = || QueryError::Size(value.to_string());
                query.size = Some(if let Some(v) = value.strip_prefix('>') {
                    SizeFilter::Above(parse_size(v).ok_or_else(bad)?)
                } else if let Some(v) = value.strip_prefix('<') {
                    SizeFilter::Below(parse_size(v).ok_or_else(bad)?)
                } else {
                    SizeFilter::AtLeast(parse_size(value).ok_or_else(bad)?)
                });
            } else if let Some(value) = lower.strip_prefix("modified:") {
                query.modified_within =
                    Some(parse_age(value).ok_or_else(|| QueryError::Age(value.to_string()))?);
            } else if let Some(value) = lower.strip_prefix("kind:") {
                query.kind = Some(match value {
                    "folder" | "dir" | "folders" => KindFilter::Folder,
                    "file" | "files" => KindFilter::File,
                    other => return Err(QueryError::Kind(other.to_string())),
                });
            } else if lower.contains(['*', '?']) {
                query.terms.push(Term::Wildcard(lower.chars().collect()));
            } else {
                query.terms.push(Term::Contains(lower));
            }
        }
        Ok(query)
    }

    /// A query with nothing in it matches everything; searching with it is not useful.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether `candidate` matches, `now` being the current FILETIME.
    pub fn matches(&self, candidate: &Candidate<'_>, now: i64) -> bool {
        if let Some(kind) = self.kind
            && (kind == KindFilter::Folder) != candidate.is_folder
        {
            return false;
        }
        if !self.ext.is_empty() {
            let ext = candidate
                .name
                .rsplit_once('.')
                .map(|(_, ext)| ext.to_lowercase());
            if candidate.is_folder || !ext.is_some_and(|ext| self.ext.contains(&ext)) {
                return false;
            }
        }
        if let Some(size) = self.size {
            if candidate.is_folder {
                return false;
            }
            let ok = match size {
                SizeFilter::AtLeast(n) => candidate.size >= n,
                SizeFilter::Above(n) => candidate.size > n,
                SizeFilter::Below(n) => candidate.size < n,
            };
            if !ok {
                return false;
            }
        }
        if let Some(within) = self.modified_within
            && now.saturating_sub(candidate.modified) > within
        {
            return false;
        }
        if self.terms.is_empty() {
            return true;
        }
        let name = candidate.name.to_lowercase();
        let chars: Vec<char> = if self.terms.iter().any(|t| matches!(t, Term::Wildcard(_))) {
            name.chars().collect()
        } else {
            Vec::new()
        };
        self.terms.iter().all(|term| match term {
            Term::Contains(word) => name.contains(word.as_str()),
            Term::Wildcard(pattern) => wildcard(pattern, &chars),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 133_000_000_000_000_000;

    fn file(name: &str, size: u64, age_hours: i64) -> Candidate<'_> {
        Candidate {
            name,
            is_folder: false,
            size,
            modified: NOW - age_hours * TICKS_PER_HOUR,
        }
    }

    fn hits(query: &str, candidate: &Candidate<'_>) -> bool {
        Query::parse(query).unwrap().matches(candidate, NOW)
    }

    #[test]
    fn words_wildcards_and_quotes() {
        let report = file("Annual Report 2026.PDF", 10, 1);
        assert!(hits("report", &report));
        assert!(hits("annual 2026", &report));
        assert!(!hits("annual budget", &report));
        assert!(hits("\"annual report\"", &report));
        assert!(!hits("\"report annual\"", &report));
        assert!(hits("*.pdf", &report));
        assert!(!hits("*.doc", &report));
        assert!(hits("annual?report*", &report));
        assert!(hits("*", &report));
        assert!(Query::parse("  ").unwrap().is_empty());
    }

    #[test]
    fn operators() {
        let big = file("movie.mkv", 2 * 1024 * 1024 * 1024, 24 * 10);
        let small = file("notes.txt", 500, 2);
        assert!(hits("ext:mkv", &big) && !hits("ext:txt", &big));
        assert!(hits("ext:.txt,md", &small));
        assert!(hits("size:>1gb", &big) && !hits("size:>1gb", &small));
        assert!(hits("size:<1kb", &small) && hits("size:500", &small));
        assert!(hits("size:1.5gb", &big));
        assert!(hits("modified:7d", &small) && !hits("modified:7d", &big));
        assert!(hits("modified:2w", &big) && hits("modified:3h", &small));
        let folder = Candidate {
            name: "Projects",
            is_folder: true,
            size: 0,
            modified: NOW,
        };
        assert!(hits("kind:folder", &folder) && !hits("kind:file", &folder));
        assert!(!hits("ext:txt", &folder) && !hits("size:>0", &folder));
        assert!(hits("kind:file notes ext:txt size:<1kb", &small));
    }

    #[test]
    fn bad_values_name_themselves() {
        assert_eq!(
            Query::parse("size:>lots"),
            Err(QueryError::Size(">lots".into()))
        );
        assert_eq!(
            Query::parse("modified:soon"),
            Err(QueryError::Age("soon".into()))
        );
        assert_eq!(
            Query::parse("kind:cat"),
            Err(QueryError::Kind("cat".into()))
        );
        // Nothing panics on odd input.
        for input in [
            "size:",
            "modified:99999999999999999999d",
            "\"",
            "ext:",
            "*?*?**",
            "size:>-1",
        ] {
            let _ = Query::parse(input);
        }
    }

    #[test]
    fn wildcard_backtracks() {
        let chars = |s: &str| s.chars().collect::<Vec<_>>();
        assert!(wildcard(&chars("a*b*c"), &chars("axxbyyc")));
        assert!(!wildcard(&chars("a*b*c"), &chars("axxbyy")));
        assert!(wildcard(&chars("*.tar.gz"), &chars("x.tar.gz")));
        assert!(!wildcard(&chars("?"), &chars("")));
    }
}
