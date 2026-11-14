//! `src/rule.ts`: compiled rules and the regular expression lists their
//! scanners search.

use std::cell::RefCell;
use std::rc::Rc;

use super::scanner::{Scanner, ScannerError};
use super::utils::{escape_regexp_characters, has_captures, replace_captures, CaptureIndex};

/// A rule id: positive for a rule, [`END_RULE_ID`] or [`WHILE_RULE_ID`].
pub type RuleId = isize;
pub const END_RULE_ID: RuleId = -1;
pub const WHILE_RULE_ID: RuleId = -2;

/// The `￿` that stands for a disabled anchor or a missing `end`.
const NEVER: &str = "\u{ffff}";

/// `RegExpSource`.
#[derive(Clone, Debug)]
pub struct RegExpSource {
    pub source: String,
    pub rule_id: RuleId,
    pub has_anchor: bool,
    pub has_back_references: bool,
    anchor_cache: Option<[String; 4]>,
}

/// `HAS_BACK_REFERENCES`, `/\\(\d+)/`.
fn has_back_references(source: &str) -> bool {
    let bytes = source.as_bytes();
    bytes
        .windows(2)
        .any(|pair| pair[0] == b'\\' && pair[1].is_ascii_digit())
}

impl RegExpSource {
    pub fn new(source: &str, rule_id: RuleId) -> Self {
        let mut output = String::new();
        let mut last_pushed = 0;
        let mut has_anchor = false;
        let mut chars = source.char_indices().peekable();
        while let Some((pos, ch)) = chars.next() {
            if ch == '\\' {
                if let Some(&(next_pos, next)) = chars.peek() {
                    if next == 'z' {
                        output.push_str(&source[last_pushed..pos]);
                        output.push_str("$(?!\\n)(?<!\\n)");
                        last_pushed = next_pos + 1;
                    } else if next == 'A' || next == 'G' {
                        has_anchor = true;
                    }
                    chars.next();
                }
            }
        }
        let source = if last_pushed == 0 {
            source.to_owned()
        } else {
            output.push_str(&source[last_pushed..]);
            output
        };
        let mut result = Self {
            has_back_references: has_back_references(&source),
            source,
            rule_id,
            has_anchor,
            anchor_cache: None,
        };
        if has_anchor {
            result.anchor_cache = Some(result.build_anchor_cache());
        }
        result
    }

    pub fn set_source(&mut self, source: &str) {
        if self.source == source {
            return;
        }
        self.source = source.to_owned();
        if self.has_anchor {
            self.anchor_cache = Some(self.build_anchor_cache());
        }
    }

    /// `resolveBackReferences`: `\N` becomes capture N's text, escaped.
    pub fn resolve_back_references(&self, line: &str, captures: &[CaptureIndex]) -> String {
        let bytes = self.source.as_bytes();
        let mut out = String::new();
        let mut last = 0;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                let digits = bytes[i + 1..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                if digits > 0 {
                    out.push_str(&self.source[last..i]);
                    let index: usize = self.source[i + 1..i + 1 + digits]
                        .parse()
                        .unwrap_or(usize::MAX);
                    let value = captures
                        .get(index)
                        .map(|c| &line[c.start.min(line.len())..c.end.min(line.len())])
                        .unwrap_or("");
                    out.push_str(&escape_regexp_characters(value));
                    i += 1 + digits;
                    last = i;
                    continue;
                }
            }
            i += 1;
        }
        out.push_str(&self.source[last..]);
        out
    }

    /// `_buildAnchorCache`: the source with `\A` and `\G` each kept or
    /// replaced by a character no line contains. Index `a * 2 + g`.
    fn build_anchor_cache(&self) -> [String; 4] {
        let mut results: [String; 4] = Default::default();
        let mut chars = self.source.chars().peekable();
        while let Some(ch) = chars.next() {
            for result in results.iter_mut() {
                result.push(ch);
            }
            if ch == '\\' {
                if let Some(&next) = chars.peek() {
                    chars.next();
                    for (index, result) in results.iter_mut().enumerate() {
                        let (allow_a, allow_g) = (index >= 2, index % 2 == 1);
                        match next {
                            'A' if !allow_a => result.push_str(NEVER),
                            'G' if !allow_g => result.push_str(NEVER),
                            _ => result.push(next),
                        }
                    }
                }
            }
        }
        results
    }

    pub fn resolve_anchors(&self, allow_a: bool, allow_g: bool) -> &str {
        match &self.anchor_cache {
            Some(cache) if self.has_anchor => {
                &cache[usize::from(allow_a) * 2 + usize::from(allow_g)]
            }
            _ => &self.source,
        }
    }
}

/// `CompiledRule`: a scanner and the rule each of its patterns belongs to.
pub struct CompiledRule {
    pub scanner: Scanner,
    pub rules: Vec<RuleId>,
}

/// A match: the rule and the captures of its pattern.
pub struct FoundMatch {
    pub rule_id: RuleId,
    pub capture_indices: Vec<CaptureIndex>,
}

impl CompiledRule {
    pub fn find_next_match(&self, line: &str, start: usize) -> Option<FoundMatch> {
        self.scanner
            .find_next_match(line, start)
            .map(|(index, capture_indices)| FoundMatch {
                rule_id: self.rules[index],
                capture_indices,
            })
    }
}

/// `RegExpSourceList`, with its compiled scanners cached.
#[derive(Default)]
pub struct RegExpSourceList {
    items: Vec<RegExpSource>,
    has_anchors: bool,
    cached: Option<Rc<CompiledRule>>,
    anchor_cache: [Option<Rc<CompiledRule>>; 4],
}

impl RegExpSourceList {
    pub fn push(&mut self, item: RegExpSource) {
        self.has_anchors = self.has_anchors || item.has_anchor;
        self.items.push(item);
    }

    pub fn unshift(&mut self, item: RegExpSource) {
        self.has_anchors = self.has_anchors || item.has_anchor;
        self.items.insert(0, item);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn set_source(&mut self, index: usize, source: &str) {
        if self.items[index].source != source {
            self.cached = None;
            self.anchor_cache = Default::default();
            self.items[index].set_source(source);
        }
    }

    fn build(&self, sources: Vec<&str>) -> Result<Rc<CompiledRule>, ScannerError> {
        Ok(Rc::new(CompiledRule {
            scanner: Scanner::new(&sources)?,
            rules: self.items.iter().map(|item| item.rule_id).collect(),
        }))
    }

    pub fn compile(&mut self) -> Result<Rc<CompiledRule>, ScannerError> {
        if self.cached.is_none() {
            let sources = self.items.iter().map(|item| item.source.as_str()).collect();
            self.cached = Some(self.build(sources)?);
        }
        Ok(self.cached.clone().expect("compiled"))
    }

    pub fn compile_ag(
        &mut self,
        allow_a: bool,
        allow_g: bool,
    ) -> Result<Rc<CompiledRule>, ScannerError> {
        if !self.has_anchors {
            return self.compile();
        }
        let index = usize::from(allow_a) * 2 + usize::from(allow_g);
        if self.anchor_cache[index].is_none() {
            let sources = self
                .items
                .iter()
                .map(|item| item.resolve_anchors(allow_a, allow_g))
                .collect();
            self.anchor_cache[index] = Some(self.build(sources)?);
        }
        Ok(self.anchor_cache[index].clone().expect("compiled"))
    }
}

/// The kinds of `Rule`.
pub enum RuleKind {
    Capture {
        retokenize_captured_with_rule_id: RuleId,
    },
    Match {
        regex: RegExpSource,
        captures: Vec<Option<RuleId>>,
    },
    IncludeOnly {
        patterns: Vec<RuleId>,
        has_missing_patterns: bool,
    },
    BeginEnd {
        begin: RegExpSource,
        begin_captures: Vec<Option<RuleId>>,
        end: RegExpSource,
        end_captures: Vec<Option<RuleId>>,
        apply_end_pattern_last: bool,
        patterns: Vec<RuleId>,
        has_missing_patterns: bool,
    },
    BeginWhile {
        begin: RegExpSource,
        begin_captures: Vec<Option<RuleId>>,
        while_: RegExpSource,
        while_captures: Vec<Option<RuleId>>,
        patterns: Vec<RuleId>,
        has_missing_patterns: bool,
    },
}

/// `Rule` and its subclasses.
pub struct Rule {
    pub id: RuleId,
    name: Option<String>,
    name_is_capturing: bool,
    content_name: Option<String>,
    content_name_is_capturing: bool,
    pub kind: RuleKind,
    pub cached_patterns: RefCell<Option<RegExpSourceList>>,
    pub cached_while_patterns: RefCell<Option<RegExpSourceList>>,
}

impl Rule {
    pub fn new(
        id: RuleId,
        name: Option<String>,
        content_name: Option<String>,
        kind: RuleKind,
    ) -> Self {
        Self {
            id,
            name_is_capturing: has_captures(name.as_deref()),
            name,
            content_name_is_capturing: has_captures(content_name.as_deref()),
            content_name,
            kind,
            cached_patterns: RefCell::new(None),
            cached_while_patterns: RefCell::new(None),
        }
    }

    /// `getName`. `line` and `captures` are `None` for the root rule.
    pub fn name(&self, line: Option<&str>, captures: Option<&[CaptureIndex]>) -> Option<String> {
        match (&self.name, line, captures) {
            (Some(name), Some(line), Some(captures)) if self.name_is_capturing => {
                Some(replace_captures(name, line, captures))
            }
            (name, _, _) => name.clone(),
        }
    }

    /// `getContentName`.
    pub fn content_name(&self, line: &str, captures: &[CaptureIndex]) -> Option<String> {
        match &self.content_name {
            Some(name) if self.content_name_is_capturing => {
                Some(replace_captures(name, line, captures))
            }
            name => name.clone(),
        }
    }

    /// The `patterns` and `hasMissingPatterns` of the rules that have them.
    pub fn patterns(&self) -> Option<(&[RuleId], bool)> {
        match &self.kind {
            RuleKind::IncludeOnly {
                patterns,
                has_missing_patterns,
            }
            | RuleKind::BeginEnd {
                patterns,
                has_missing_patterns,
                ..
            }
            | RuleKind::BeginWhile {
                patterns,
                has_missing_patterns,
                ..
            } => Some((patterns, *has_missing_patterns)),
            _ => None,
        }
    }
}
