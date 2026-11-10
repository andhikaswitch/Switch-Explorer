//! Fuzzy matching for the command palette and quick open (FEATURES.md F-SRCH-04, ADR-023): the
//! query's characters must appear in the candidate in order, ignoring case and spaces. Matches
//! at word starts and in runs score higher, so "nf" ranks "New folder" above "Information".
//!
//! The match is found the way fzf v1 does: forward to the first place the whole query fits, then
//! backward from there to the shortest window, then scored. The candidates are short labels (a
//! few hundred at most), so this costs microseconds per keystroke.

/// A candidate that matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    pub score: i32,
    /// Byte offsets of the matched characters in the candidate, ascending (for highlighting).
    pub positions: Vec<usize>,
}

const MATCH: i32 = 16;
/// The character starts a word: the start, after a separator, or a capital after a lowercase.
const BOUNDARY: i32 = 10;
/// The character follows the previous match directly.
const CONSECUTIVE: i32 = 8;
/// The first query character is the candidate's first character.
const PREFIX: i32 = 8;
const GAP_START: i32 = -3;
const GAP_EXTEND: i32 = -1;

fn is_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '_' | '-' | '.' | '/' | '\\' | ':' | '(' | ')' | ',')
}

/// Lowercase form of `c` when it lowercases to one character; else `c` itself.
fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Matches `query` against `candidate`; `None` when not every query character appears in order.
/// An empty query matches everything with score 0.
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<FuzzyMatch> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(fold)
        .collect();
    if query.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            positions: Vec::new(),
        });
    }
    let chars: Vec<(usize, char, bool)> = {
        let mut prev: Option<char> = None;
        candidate
            .char_indices()
            .map(|(at, c)| {
                let boundary = match prev {
                    None => true,
                    Some(p) => {
                        is_separator(p)
                            || (p.is_lowercase() && c.is_uppercase())
                            || (!p.is_ascii_digit() && c.is_ascii_digit())
                    }
                };
                prev = Some(c);
                (at, fold(c), boundary)
            })
            .collect()
    };
    // Forward: the end of the first place the whole query fits.
    let mut qi = 0;
    let mut end = None;
    for (ci, &(_, c, _)) in chars.iter().enumerate() {
        if c == query[qi] {
            qi += 1;
            if qi == query.len() {
                end = Some(ci);
                break;
            }
        }
    }
    let end = end?;
    // Backward: the latest start that still fits, giving the shortest window.
    let mut qi = query.len();
    let mut start = end;
    for ci in (0..=end).rev() {
        if chars[ci].1 == query[qi - 1] {
            qi -= 1;
            if qi == 0 {
                start = ci;
                break;
            }
        }
    }
    // Within the window: each query character at its first fit, preferring a word start when
    // one comes before the next character is needed.
    let mut positions = Vec::with_capacity(query.len());
    let mut ci = start;
    for (qi, &q) in query.iter().enumerate() {
        let remaining = &query[qi + 1..];
        let mut pick = None;
        let mut scan = ci;
        while scan <= end {
            let (_, c, boundary) = chars[scan];
            if c == q {
                if pick.is_none() {
                    pick = Some(scan);
                }
                if boundary && fits(&chars[scan + 1..=end], remaining) {
                    pick = Some(scan);
                    break;
                }
                if !fits(&chars[scan + 1..=end], remaining) {
                    break;
                }
            }
            scan += 1;
        }
        let at = pick?;
        positions.push(at);
        ci = at + 1;
    }
    let mut score = 0;
    let mut prev: Option<usize> = None;
    for &at in &positions {
        score += MATCH;
        if chars[at].2 {
            score += BOUNDARY;
        }
        match prev {
            Some(p) if at == p + 1 => score += CONSECUTIVE,
            Some(p) => score += GAP_START + GAP_EXTEND * (at - p - 2) as i32,
            None if at == 0 => score += PREFIX,
            None => {}
        }
        prev = Some(at);
    }
    Some(FuzzyMatch {
        score,
        positions: positions.into_iter().map(|at| chars[at].0).collect(),
    })
}

/// Whether `query` still fits, in order, in `rest`.
fn fits(rest: &[(usize, char, bool)], query: &[char]) -> bool {
    let mut qi = 0;
    for &(_, c, _) in rest {
        if qi == query.len() {
            return true;
        }
        if c == query[qi] {
            qi += 1;
        }
    }
    qi == query.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(query: &str, candidate: &str) -> Option<i32> {
        fuzzy_match(query, candidate).map(|m| m.score)
    }

    #[test]
    fn letters_must_come_in_order() {
        assert!(score("nf", "New folder").is_some());
        assert!(score("fn", "New folder").is_none());
        assert!(score("NEW FOL", "new folder").is_some());
        assert!(score("xyz", "New folder").is_none());
        assert_eq!(score("", "anything"), Some(0));
    }

    #[test]
    fn word_starts_and_runs_rank_first() {
        let new_folder = score("nf", "New folder").unwrap();
        let information = score("nf", "Information").unwrap();
        assert!(new_folder > information, "{new_folder} vs {information}");
        // A run beats scattered letters.
        assert!(score("copy", "Copy path").unwrap() > score("copy", "Clear old proxy").unwrap());
        // camelCase and digits count as word starts.
        let camel = fuzzy_match("sh", "showHidden").unwrap();
        assert_eq!(camel.positions, [0, 1]);
        let word = fuzzy_match("hd", "show hidden").unwrap();
        assert_eq!(word.positions, [5, 7]);
    }

    #[test]
    fn positions_are_byte_offsets() {
        let found = fuzzy_match("ér", "Café — Réunion").unwrap();
        let text = "Café — Réunion";
        let picked: String = found
            .positions
            .iter()
            .map(|&at| text[at..].chars().next().unwrap())
            .collect();
        assert_eq!(picked.to_lowercase(), "ér");
    }

    #[test]
    fn prefers_a_later_word_start_when_the_rest_still_fits() {
        // "t" of "tab" (a word start), not the "t" inside "next".
        let found = fuzzy_match("tab", "Next tab").unwrap();
        assert_eq!(found.positions, [5, 6, 7]);
    }
}
