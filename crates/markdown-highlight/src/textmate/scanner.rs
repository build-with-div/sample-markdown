//! `OnigScanner`: what `@shikijs/engine-oniguruma` does with Oniguruma
//! compiled to WebAssembly (vscode-oniguruma's `onig.cc`), on the `onig`
//! crate's Oniguruma.
//!
//! Each pattern is compiled with `ONIG_OPTION_CAPTURE_GROUP`, UTF-8 and the
//! Oniguruma syntax. A search returns the match that starts first; on a tie,
//! the pattern listed first (`ONIG_REGSET_POSITION_LEAD`). vscode-textmate
//! compiles its `\A` and `\G` variants itself, so the search options are
//! always none. Positions are UTF-8 byte offsets throughout the port, where
//! vscode-textmate's are UTF-16: the engine converts both ways, so the
//! matches are the same.

use std::fmt;

use onig::{Regex, RegexOptions, Region, SearchOptions, Syntax};

use super::utils::CaptureIndex;

/// A pattern Oniguruma rejects. Shiki throws for it too.
#[derive(Debug)]
pub struct ScannerError {
    pub pattern: String,
    pub message: String,
}

impl fmt::Display for ScannerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} in /{}/", self.message, self.pattern)
    }
}

impl std::error::Error for ScannerError {}

pub struct Scanner {
    regexes: Vec<Regex>,
}

impl Scanner {
    pub fn new(patterns: &[&str]) -> Result<Self, ScannerError> {
        let regexes = patterns
            .iter()
            .map(|pattern| {
                Regex::with_options(
                    pattern,
                    RegexOptions::REGEX_OPTION_CAPTURE_GROUP,
                    Syntax::oniguruma(),
                )
                .map_err(|error| ScannerError {
                    pattern: (*pattern).to_owned(),
                    message: error.to_string(),
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { regexes })
    }

    /// `findNextMatchSync`: the pattern index and its captures. A group that
    /// did not participate is an empty span at the end of `line`.
    pub fn find_next_match(&self, line: &str, start: usize) -> Option<(usize, Vec<CaptureIndex>)> {
        let mut best: Option<(usize, usize, Region)> = None;
        for (index, regex) in self.regexes.iter().enumerate() {
            let mut region = Region::new();
            let found = regex.search_with_options(
                line,
                start,
                line.len(),
                SearchOptions::SEARCH_OPTION_NONE,
                Some(&mut region),
            );
            if let Some(position) = found {
                if best
                    .as_ref()
                    .is_none_or(|(_, best_position, _)| position < *best_position)
                {
                    let done = position == start;
                    best = Some((index, position, region));
                    if done {
                        break;
                    }
                }
            }
        }
        best.map(|(index, _, region)| {
            let captures = (0..region.len())
                .map(|group| match region.pos(group) {
                    Some((start, end)) => CaptureIndex { start, end },
                    None => CaptureIndex {
                        start: line.len(),
                        end: line.len(),
                    },
                })
                .collect();
            (index, captures)
        })
    }
}
