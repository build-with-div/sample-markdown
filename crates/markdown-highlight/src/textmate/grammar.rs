//! `src/grammar/grammar.ts`, `src/grammar/tokenizeString.ts`,
//! `src/grammar/basicScopesAttributeProvider.ts` and the `RuleFactory` of
//! `src/rule.ts`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use super::matcher::{create_matchers, Matcher};
use super::metadata::{self, TOKEN_TYPE_NOT_SET};
use super::raw::{init_grammar, parse_int, RawCaptures, RawGrammar, RawRule, Repository};
use super::registry::SyncRegistry;
use super::rule::{
    CompiledRule, RegExpSource, RegExpSourceList, Rule, RuleId, RuleKind, END_RULE_ID,
    WHILE_RULE_ID,
};
use super::scanner::ScannerError;
use super::theme::{ScopeStack, StyleAttributes};
use super::utils::CaptureIndex;

type Result<T> = std::result::Result<T, ScannerError>;

fn unsupported() -> ScannerError {
    ScannerError {
        pattern: String::new(),
        message: "Not supported!".into(),
    }
}

/// `parseInclude`.
enum Include<'a> {
    Base,
    SelfRef,
    Relative(&'a str),
    TopLevel(&'a str),
    TopLevelRepository(&'a str, &'a str),
}

fn parse_include(include: &str) -> Include<'_> {
    match include {
        "$base" => Include::Base,
        "$self" => Include::SelfRef,
        _ => match include.find('#') {
            None => Include::TopLevel(include),
            Some(0) => Include::Relative(&include[1..]),
            Some(i) => Include::TopLevelRepository(&include[..i], &include[i + 1..]),
        },
    }
}

/// `scopesAreMatching`.
fn scopes_are_matching(this_scope_name: &str, scope_name: &str) -> bool {
    if this_scope_name.is_empty() {
        return false;
    }
    this_scope_name == scope_name
        || (this_scope_name.len() > scope_name.len()
            && this_scope_name.starts_with(scope_name)
            && this_scope_name.as_bytes()[scope_name.len()] == b'.')
}

/// `nameMatcher`.
pub fn name_matcher(identifiers: &[String], scopes: &[String]) -> bool {
    if scopes.len() < identifiers.len() {
        return false;
    }
    let mut last_index = 0;
    identifiers.iter().all(|identifier| {
        for (i, scope) in scopes.iter().enumerate().skip(last_index) {
            if scopes_are_matching(scope, identifier) {
                last_index = i + 1;
                return true;
            }
        }
        false
    })
}

fn matches_scopes(matcher: &Matcher, scopes: &[String]) -> bool {
    matcher.matches(scopes, &|names: &[String], scopes: &[String]| {
        name_matcher(names, scopes)
    })
}

/// `BasicScopeAttributesProvider._toStandardTokenType`: the first match of
/// `/\b(comment|string|regex|meta\.embedded)\b/`.
fn standard_token_type(scope_name: &str) -> u32 {
    let bytes = scope_name.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    for start in 0..bytes.len() {
        if start > 0 && word(bytes[start - 1]) {
            continue;
        }
        for (text, kind) in [
            ("comment", 1),
            ("string", 2),
            ("regex", 3),
            ("meta.embedded", 0),
        ] {
            let end = start + text.len();
            if bytes[start..].starts_with(text.as_bytes())
                && (end == bytes.len() || !word(bytes[end]))
            {
                return kind;
            }
        }
    }
    TOKEN_TYPE_NOT_SET
}

/// `AttributedScopeStack`.
#[derive(Debug)]
pub struct AttributedScopeStack {
    pub parent: Option<Rc<AttributedScopeStack>>,
    pub scope_path: Rc<ScopeStack>,
    pub token_attributes: u32,
}

impl AttributedScopeStack {
    /// `mergeAttributes`. Embedded languages are never configured, so the
    /// scope's language id is 0.
    fn merge_attributes(existing: u32, scope_name: &str, style: Option<StyleAttributes>) -> u32 {
        let (font_style, foreground, background) = match style {
            Some(style) => (style.font_style, style.foreground_id, style.background_id),
            None => (-1, 0, 0),
        };
        metadata::set(
            existing,
            0,
            standard_token_type(scope_name),
            None,
            font_style,
            foreground,
            background,
        )
    }

    fn push_one(target: &Rc<Self>, scope_name: &str, grammar: &Grammar) -> Rc<Self> {
        let path = ScopeStack::push(Some(target.scope_path.clone()), &[scope_name.to_owned()])
            .expect("pushed");
        let style = grammar.registry.theme().match_path(Some(&path));
        Rc::new(Self {
            parent: Some(target.clone()),
            token_attributes: Self::merge_attributes(target.token_attributes, scope_name, style),
            scope_path: path,
        })
    }

    /// `pushAttributed`.
    pub fn push_attributed(
        self: &Rc<Self>,
        scope_path: Option<&str>,
        grammar: &Grammar,
    ) -> Rc<Self> {
        let Some(scope_path) = scope_path else {
            return self.clone();
        };
        let mut result = self.clone();
        for scope in scope_path.split(' ') {
            result = Self::push_one(&result, scope, grammar);
        }
        result
    }

    pub fn scope_names(&self) -> Vec<String> {
        self.scope_path.segments()
    }
}

/// `StateStackImpl`: the tokenizer state carried from line to line.
#[derive(Debug)]
pub struct StateStack {
    pub parent: Option<Rc<StateStack>>,
    pub rule_id: RuleId,
    enter_pos: Cell<isize>,
    anchor_pos: Cell<isize>,
    pub begin_rule_captured_eol: bool,
    pub end_rule: Option<String>,
    pub name_scopes_list: Rc<AttributedScopeStack>,
    pub content_name_scopes_list: Rc<AttributedScopeStack>,
    pub depth: usize,
}

impl StateStack {
    #[allow(clippy::too_many_arguments)]
    fn new(
        parent: Option<Rc<StateStack>>,
        rule_id: RuleId,
        enter_pos: isize,
        anchor_pos: isize,
        begin_rule_captured_eol: bool,
        end_rule: Option<String>,
        name_scopes_list: Rc<AttributedScopeStack>,
        content_name_scopes_list: Rc<AttributedScopeStack>,
    ) -> Rc<Self> {
        let depth = parent.as_ref().map_or(1, |p| p.depth + 1);
        Rc::new(Self {
            parent,
            rule_id,
            enter_pos: Cell::new(enter_pos),
            anchor_pos: Cell::new(anchor_pos),
            begin_rule_captured_eol,
            end_rule,
            name_scopes_list,
            content_name_scopes_list,
            depth,
        })
    }

    fn reset(self: &Rc<Self>) {
        let mut element = Some(self.clone());
        while let Some(current) = element {
            current.enter_pos.set(-1);
            current.anchor_pos.set(-1);
            element = current.parent.clone();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        self: &Rc<Self>,
        rule_id: RuleId,
        enter_pos: isize,
        anchor_pos: isize,
        begin_rule_captured_eol: bool,
        end_rule: Option<String>,
        name_scopes_list: Rc<AttributedScopeStack>,
        content_name_scopes_list: Rc<AttributedScopeStack>,
    ) -> Rc<Self> {
        Self::new(
            Some(self.clone()),
            rule_id,
            enter_pos,
            anchor_pos,
            begin_rule_captured_eol,
            end_rule,
            name_scopes_list,
            content_name_scopes_list,
        )
    }

    fn pop(&self) -> Option<Rc<Self>> {
        self.parent.clone()
    }

    fn safe_pop(self: &Rc<Self>) -> Rc<Self> {
        self.parent.clone().unwrap_or_else(|| self.clone())
    }

    fn with_content_name_scopes_list(self: &Rc<Self>, list: Rc<AttributedScopeStack>) -> Rc<Self> {
        if Rc::ptr_eq(&self.content_name_scopes_list, &list) {
            return self.clone();
        }
        Self::new(
            self.parent.clone(),
            self.rule_id,
            self.enter_pos.get(),
            self.anchor_pos.get(),
            self.begin_rule_captured_eol,
            self.end_rule.clone(),
            self.name_scopes_list.clone(),
            list,
        )
    }

    fn with_end_rule(self: &Rc<Self>, end_rule: String) -> Rc<Self> {
        if self.end_rule.as_deref() == Some(end_rule.as_str()) {
            return self.clone();
        }
        Self::new(
            self.parent.clone(),
            self.rule_id,
            self.enter_pos.get(),
            self.anchor_pos.get(),
            self.begin_rule_captured_eol,
            Some(end_rule),
            self.name_scopes_list.clone(),
            self.content_name_scopes_list.clone(),
        )
    }

    /// `hasSameRuleAs`: used to detect endless loops.
    fn has_same_rule_as(self: &Rc<Self>, other: &StateStack) -> bool {
        let mut element = Some(self.clone());
        while let Some(current) = element {
            if current.enter_pos.get() != other.enter_pos.get() {
                break;
            }
            if current.rule_id == other.rule_id {
                return true;
            }
            element = current.parent.clone();
        }
        false
    }
}

/// `BalancedBracketSelectors`.
pub struct BalancedBracketSelectors {
    balanced: Vec<Matcher>,
    unbalanced: Vec<Matcher>,
    allow_any: bool,
}

impl BalancedBracketSelectors {
    pub fn new(balanced: &[String], unbalanced: &[String]) -> Self {
        let mut allow_any = false;
        let mut balanced_matchers = Vec::new();
        for selector in balanced {
            if selector == "*" {
                allow_any = true;
                continue;
            }
            balanced_matchers.extend(create_matchers(selector).into_iter().map(|m| m.matcher));
        }
        Self {
            balanced: balanced_matchers,
            unbalanced: unbalanced
                .iter()
                .flat_map(|selector| create_matchers(selector).into_iter().map(|m| m.matcher))
                .collect(),
            allow_any,
        }
    }

    fn matches_always(&self) -> bool {
        self.allow_any && self.unbalanced.is_empty()
    }

    fn matches_never(&self) -> bool {
        self.balanced.is_empty() && !self.allow_any
    }

    fn matches(&self, scopes: &[String]) -> bool {
        if self.unbalanced.iter().any(|m| matches_scopes(m, scopes)) {
            return false;
        }
        if self.balanced.iter().any(|m| matches_scopes(m, scopes)) {
            return true;
        }
        self.allow_any
    }
}

/// `LineTokens`, emitting binary tokens: start offsets and metadata.
struct LineTokens<'a> {
    balanced_bracket_selectors: &'a BalancedBracketSelectors,
    binary_tokens: Vec<u32>,
    last_token_end_index: isize,
}

impl LineTokens<'_> {
    fn produce(&mut self, stack: &StateStack, end_index: usize) {
        self.produce_from_scopes(&stack.content_name_scopes_list, end_index);
    }

    fn produce_from_scopes(&mut self, scopes_list: &AttributedScopeStack, end_index: usize) {
        if self.last_token_end_index >= end_index as isize {
            return;
        }
        let selectors = self.balanced_bracket_selectors;
        let mut metadata = scopes_list.token_attributes;
        let mut contains_balanced_brackets = selectors.matches_always();
        if !selectors.matches_always() && !selectors.matches_never() {
            contains_balanced_brackets = selectors.matches(&scopes_list.scope_names());
        }
        if contains_balanced_brackets {
            metadata = metadata::set(metadata, 0, TOKEN_TYPE_NOT_SET, Some(true), -1, 0, 0);
        }
        if self.binary_tokens.last() == Some(&metadata) {
            self.last_token_end_index = end_index as isize;
            return;
        }
        self.binary_tokens.push(self.last_token_end_index as u32);
        self.binary_tokens.push(metadata);
        self.last_token_end_index = end_index as isize;
    }

    fn binary_result(mut self, stack: &StateStack, line_length: usize) -> Vec<u32> {
        let len = self.binary_tokens.len();
        if len > 0 && self.binary_tokens[len - 2] as usize == line_length - 1 {
            self.binary_tokens.truncate(len - 2);
        }
        if self.binary_tokens.is_empty() {
            self.last_token_end_index = -1;
            self.produce(stack, line_length);
            let len = self.binary_tokens.len();
            self.binary_tokens[len - 2] = 0;
        }
        self.binary_tokens
    }
}

/// An injection: `collectInjections`.
struct Injection {
    matcher: Matcher,
    rule_id: RuleId,
    priority: i32,
}

/// The result of `tokenizeLine2`.
pub struct TokenizeLineResult {
    /// Pairs of a start offset (UTF-8 bytes) and metadata.
    pub tokens: Vec<u32>,
    pub rule_stack: Rc<StateStack>,
    pub stopped_early: bool,
}

/// `Grammar`.
pub struct Grammar {
    root_scope_name: String,
    registry: Rc<SyncRegistry>,
    initial_language: u32,
    balanced_bracket_selectors: BalancedBracketSelectors,
    root_id: Cell<RuleId>,
    rules: RefCell<Vec<Option<Rc<Rule>>>>,
    included_grammars: RefCell<HashMap<String, Rc<RawGrammar>>>,
    grammar: Rc<RawGrammar>,
    injections: RefCell<Option<Rc<Vec<Injection>>>>,
}

impl Grammar {
    pub fn new(
        root_scope_name: &str,
        raw: &serde_json::Value,
        initial_language: u32,
        balanced_bracket_selectors: BalancedBracketSelectors,
        registry: Rc<SyncRegistry>,
    ) -> Self {
        Self {
            root_scope_name: root_scope_name.to_owned(),
            registry,
            initial_language,
            balanced_bracket_selectors,
            root_id: Cell::new(-1),
            rules: RefCell::new(vec![None]),
            included_grammars: RefCell::new(HashMap::new()),
            grammar: init_grammar(raw, None),
            injections: RefCell::new(None),
        }
    }

    fn rule(&self, id: RuleId) -> Rc<Rule> {
        self.rules.borrow()[id as usize]
            .clone()
            .expect("rule is registered")
    }

    fn try_rule(&self, id: RuleId) -> Option<Rc<Rule>> {
        self.rules.borrow().get(id as usize).cloned().flatten()
    }

    fn reserve_rule_id(&self) -> RuleId {
        let mut rules = self.rules.borrow_mut();
        rules.push(None);
        (rules.len() - 1) as RuleId
    }

    fn store_rule(&self, rule: Rule) -> RuleId {
        let id = rule.id;
        self.rules.borrow_mut()[id as usize] = Some(Rc::new(rule));
        id
    }

    /// `getExternalGrammar`.
    fn external_grammar(
        &self,
        scope_name: &str,
        repository: Option<&Rc<Repository>>,
    ) -> Option<Rc<RawGrammar>> {
        if let Some(grammar) = self.included_grammars.borrow().get(scope_name) {
            return Some(grammar.clone());
        }
        let raw = self.registry.lookup(scope_name)?;
        let base = repository.and_then(|r| r.get("$base").cloned());
        let grammar = init_grammar(&raw, base);
        self.included_grammars
            .borrow_mut()
            .insert(scope_name.to_owned(), grammar.clone());
        Some(grammar)
    }

    /// `RuleFactory.getCompiledRuleId`.
    fn compiled_rule_id(&self, desc: &Rc<RawRule>, repository: &Rc<Repository>) -> RuleId {
        if let Some(id) = desc.id.get() {
            return id as RuleId;
        }
        let id = self.reserve_rule_id();
        desc.id.set(Some(id as usize));
        let rule = if let Some(source) = &desc.match_ {
            let captures = self.compile_captures(desc.captures.as_ref(), repository);
            Rule::new(
                id,
                desc.name.clone(),
                None,
                RuleKind::Match {
                    regex: RegExpSource::new(source, id),
                    captures,
                },
            )
        } else if desc.begin.is_none() {
            let repository = match &desc.repository {
                Some(own) => {
                    let mut merged = (**repository).clone();
                    merged.extend(own.iter().map(|(k, v)| (k.clone(), v.clone())));
                    Rc::new(merged)
                }
                None => repository.clone(),
            };
            let patterns = match (&desc.patterns, &desc.include) {
                (None, Some(include)) => Some(Rc::new(vec![Rc::new(RawRule {
                    include: Some(include.clone()),
                    ..RawRule::default()
                })])),
                (patterns, _) => patterns.clone(),
            };
            let (patterns, has_missing_patterns) =
                self.compile_patterns(patterns.as_deref(), &repository);
            Rule::new(
                id,
                desc.name.clone(),
                desc.content_name.clone(),
                RuleKind::IncludeOnly {
                    patterns,
                    has_missing_patterns,
                },
            )
        } else if let Some(while_) = &desc.while_ {
            let begin_captures = self.compile_captures(
                desc.begin_captures.as_ref().or(desc.captures.as_ref()),
                repository,
            );
            let while_captures = self.compile_captures(
                desc.while_captures.as_ref().or(desc.captures.as_ref()),
                repository,
            );
            let (patterns, has_missing_patterns) =
                self.compile_patterns(desc.patterns.as_deref(), repository);
            Rule::new(
                id,
                desc.name.clone(),
                desc.content_name.clone(),
                RuleKind::BeginWhile {
                    begin: RegExpSource::new(desc.begin.as_deref().unwrap_or_default(), id),
                    begin_captures,
                    while_: RegExpSource::new(while_, WHILE_RULE_ID),
                    while_captures,
                    patterns,
                    has_missing_patterns,
                },
            )
        } else {
            let begin_captures = self.compile_captures(
                desc.begin_captures.as_ref().or(desc.captures.as_ref()),
                repository,
            );
            let end_captures = self.compile_captures(
                desc.end_captures.as_ref().or(desc.captures.as_ref()),
                repository,
            );
            let (patterns, has_missing_patterns) =
                self.compile_patterns(desc.patterns.as_deref(), repository);
            Rule::new(
                id,
                desc.name.clone(),
                desc.content_name.clone(),
                RuleKind::BeginEnd {
                    begin: RegExpSource::new(desc.begin.as_deref().unwrap_or_default(), id),
                    begin_captures,
                    end: RegExpSource::new(desc.end.as_deref().unwrap_or("\u{ffff}"), END_RULE_ID),
                    end_captures,
                    apply_end_pattern_last: desc.apply_end_pattern_last,
                    patterns,
                    has_missing_patterns,
                },
            )
        };
        self.store_rule(rule)
    }

    /// `RuleFactory._compileCaptures`.
    fn compile_captures(
        &self,
        captures: Option<&RawCaptures>,
        repository: &Rc<Repository>,
    ) -> Vec<Option<RuleId>> {
        let Some(captures) = captures else {
            return Vec::new();
        };
        let maximum = captures
            .iter()
            .filter_map(|(key, _)| parse_int(key))
            .fold(0, i64::max);
        let mut result = vec![None; maximum as usize + 1];
        for (key, capture) in captures.iter() {
            let mut retokenize = 0;
            if capture.patterns.is_some() {
                retokenize = self.compiled_rule_id(capture, repository);
            }
            let id = self.reserve_rule_id();
            self.store_rule(Rule::new(
                id,
                capture.name.clone(),
                capture.content_name.clone(),
                RuleKind::Capture {
                    retokenize_captured_with_rule_id: retokenize,
                },
            ));
            if let Some(index) = parse_int(key).filter(|n| *n >= 0) {
                result[index as usize] = Some(id);
            }
        }
        result
    }

    /// `RuleFactory._compilePatterns`.
    fn compile_patterns(
        &self,
        patterns: Option<&Vec<Rc<RawRule>>>,
        repository: &Rc<Repository>,
    ) -> (Vec<RuleId>, bool) {
        let mut result = Vec::new();
        let Some(patterns) = patterns else {
            return (result, false);
        };
        for pattern in patterns {
            let mut rule_id = -1;
            if let Some(include) = &pattern.include {
                match parse_include(include) {
                    Include::Base | Include::SelfRef => {
                        if let Some(desc) = repository.get(include.as_str()) {
                            rule_id = self.compiled_rule_id(desc, repository);
                        }
                    }
                    Include::Relative(name) => {
                        if let Some(desc) = repository.get(name) {
                            rule_id = self.compiled_rule_id(desc, repository);
                        }
                    }
                    Include::TopLevel(scope_name) | Include::TopLevelRepository(scope_name, _) => {
                        let rule_name = match parse_include(include) {
                            Include::TopLevelRepository(_, name) if !name.is_empty() => Some(name),
                            _ => None,
                        };
                        if let Some(external) = self.external_grammar(scope_name, Some(repository))
                        {
                            match rule_name {
                                Some(name) => {
                                    if let Some(desc) = external.repository.get(name) {
                                        rule_id = self.compiled_rule_id(desc, &external.repository);
                                    }
                                }
                                None => {
                                    let desc = external.repository["$self"].clone();
                                    rule_id = self.compiled_rule_id(&desc, &external.repository);
                                }
                            }
                        }
                    }
                }
            } else {
                rule_id = self.compiled_rule_id(pattern, repository);
            }
            if rule_id != -1 {
                let skip = self
                    .try_rule(rule_id)
                    .and_then(|rule| rule.patterns().map(|(p, missing)| missing && p.is_empty()))
                    .unwrap_or(false);
                if !skip {
                    result.push(rule_id);
                }
            }
        }
        let missing = patterns.len() != result.len();
        (result, missing)
    }

    /// `collectPatterns`.
    fn collect_patterns(&self, rule: &Rule, out: &mut RegExpSourceList) -> Result<()> {
        match &rule.kind {
            RuleKind::Capture { .. } => return Err(unsupported()),
            RuleKind::Match { regex, .. } => out.push(regex.clone()),
            RuleKind::IncludeOnly { patterns, .. } => {
                for pattern in patterns {
                    self.collect_patterns(&self.rule(*pattern), out)?;
                }
            }
            RuleKind::BeginEnd { begin, .. } | RuleKind::BeginWhile { begin, .. } => {
                out.push(begin.clone())
            }
        }
        Ok(())
    }

    /// `compileAG`.
    fn compile_ag(
        &self,
        rule: &Rule,
        end_source: Option<&str>,
        allow_a: bool,
        allow_g: bool,
    ) -> Result<Rc<CompiledRule>> {
        let mut cache = rule.cached_patterns.borrow_mut();
        if cache.is_none() {
            let mut list = RegExpSourceList::default();
            match &rule.kind {
                RuleKind::Capture { .. } => return Err(unsupported()),
                RuleKind::Match { .. } => self.collect_patterns(rule, &mut list)?,
                RuleKind::IncludeOnly { patterns, .. } | RuleKind::BeginWhile { patterns, .. } => {
                    for pattern in patterns {
                        self.collect_patterns(&self.rule(*pattern), &mut list)?;
                    }
                }
                RuleKind::BeginEnd {
                    patterns,
                    end,
                    apply_end_pattern_last,
                    ..
                } => {
                    for pattern in patterns {
                        self.collect_patterns(&self.rule(*pattern), &mut list)?;
                    }
                    if *apply_end_pattern_last {
                        list.push(end.clone());
                    } else {
                        list.unshift(end.clone());
                    }
                }
            }
            *cache = Some(list);
        }
        let list = cache.as_mut().expect("cached");
        if let RuleKind::BeginEnd {
            end,
            apply_end_pattern_last,
            ..
        } = &rule.kind
        {
            if end.has_back_references {
                if let Some(source) = end_source {
                    let index = if *apply_end_pattern_last {
                        list.len() - 1
                    } else {
                        0
                    };
                    list.set_source(index, source);
                }
            }
        }
        list.compile_ag(allow_a, allow_g)
    }

    /// `compileWhileAG`.
    fn compile_while_ag(
        &self,
        rule: &Rule,
        end_source: Option<&str>,
        allow_a: bool,
        allow_g: bool,
    ) -> Result<Rc<CompiledRule>> {
        let RuleKind::BeginWhile { while_, .. } = &rule.kind else {
            return Err(unsupported());
        };
        let mut cache = rule.cached_while_patterns.borrow_mut();
        let list = cache.get_or_insert_with(|| {
            let mut list = RegExpSourceList::default();
            list.push(while_.clone());
            list
        });
        if while_.has_back_references {
            list.set_source(0, end_source.unwrap_or("\u{ffff}"));
        }
        list.compile_ag(allow_a, allow_g)
    }

    /// `_collectInjections`.
    fn collect_injections(&self) -> Vec<Injection> {
        let mut result = Vec::new();
        let mut collect = |selector: &str, rule: &Rc<RawRule>, grammar: &RawGrammar| {
            let rule_id = self.compiled_rule_id(rule, &grammar.repository);
            for matcher in create_matchers(selector) {
                result.push(Injection {
                    matcher: matcher.matcher,
                    rule_id,
                    priority: matcher.priority,
                });
            }
        };
        let grammar = self.grammar.clone();
        for (expression, rule) in &grammar.injections {
            collect(expression, rule, &grammar);
        }
        if let Some(scope_names) = self.registry.injections(&self.root_scope_name) {
            for scope_name in scope_names {
                if let Some(injection_grammar) = self.external_grammar(&scope_name, None) {
                    if let Some(selector) = &injection_grammar.injection_selector {
                        collect(selector, &injection_grammar.as_rule, &injection_grammar);
                    }
                }
            }
        }
        result.sort_by_key(|injection| injection.priority);
        result
    }

    fn injections(&self) -> Rc<Vec<Injection>> {
        if let Some(injections) = &*self.injections.borrow() {
            return injections.clone();
        }
        let injections = Rc::new(self.collect_injections());
        *self.injections.borrow_mut() = Some(injections.clone());
        injections
    }

    /// `tokenizeLine2` with Shiki's `tokenizeTimeLimit` in milliseconds.
    pub fn tokenize_line2(
        &self,
        line: &str,
        prev_state: Option<&Rc<StateStack>>,
        time_limit: u64,
    ) -> Result<TokenizeLineResult> {
        if self.root_id.get() == -1 {
            let root = self.grammar.repository["$self"].clone();
            self.root_id
                .set(self.compiled_rule_id(&root, &self.grammar.repository));
            self.injections();
        }
        let (is_first_line, prev_state) = match prev_state {
            None => {
                let defaults = self.registry.theme().defaults();
                let default_metadata = metadata::set(
                    0,
                    self.initial_language,
                    TOKEN_TYPE_NOT_SET,
                    None,
                    defaults.font_style,
                    defaults.foreground_id,
                    defaults.background_id,
                );
                let root_scope_name = self.rule(self.root_id.get()).name(None, None);
                let scope_list = match root_scope_name {
                    Some(name) => {
                        let path =
                            ScopeStack::push(None, std::slice::from_ref(&name)).expect("root");
                        let style = self.registry.theme().match_path(Some(&path));
                        Rc::new(AttributedScopeStack {
                            parent: None,
                            token_attributes: AttributedScopeStack::merge_attributes(
                                default_metadata,
                                &name,
                                style,
                            ),
                            scope_path: path,
                        })
                    }
                    None => Rc::new(AttributedScopeStack {
                        parent: None,
                        scope_path: ScopeStack::push(None, &["unknown".to_owned()]).expect("root"),
                        token_attributes: default_metadata,
                    }),
                };
                let state = StateStack::new(
                    None,
                    self.root_id.get(),
                    -1,
                    -1,
                    false,
                    None,
                    scope_list.clone(),
                    scope_list,
                );
                (true, state)
            }
            Some(state) => {
                state.reset();
                (false, state.clone())
            }
        };
        let line = format!("{line}\n");
        let line_length = line.len();
        let mut line_tokens = LineTokens {
            balanced_bracket_selectors: &self.balanced_bracket_selectors,
            binary_tokens: Vec::new(),
            last_token_end_index: 0,
        };
        let (stack, stopped_early) = self.tokenize_string(
            &line,
            is_first_line,
            0,
            prev_state,
            &mut line_tokens,
            true,
            time_limit,
        )?;
        Ok(TokenizeLineResult {
            tokens: line_tokens.binary_result(&stack, line_length),
            rule_stack: stack,
            stopped_early,
        })
    }

    /// `_tokenizeString`.
    #[allow(clippy::too_many_arguments)]
    fn tokenize_string(
        &self,
        line: &str,
        mut is_first_line: bool,
        mut line_pos: usize,
        mut stack: Rc<StateStack>,
        line_tokens: &mut LineTokens,
        check_while_conditions: bool,
        time_limit: u64,
    ) -> Result<(Rc<StateStack>, bool)> {
        let line_length = line.len();
        let mut anchor_position: isize = -1;
        if check_while_conditions {
            let result =
                self.check_while_conditions(line, is_first_line, line_pos, stack, line_tokens)?;
            stack = result.0;
            line_pos = result.1;
            anchor_position = result.2;
            is_first_line = result.3;
        }
        let start_time = Instant::now();
        loop {
            if time_limit != 0 && start_time.elapsed().as_millis() > u128::from(time_limit) {
                return Ok((stack, true));
            }
            let Some((capture_indices, matched_rule_id)) = self.match_rule_or_injections(
                line,
                is_first_line,
                line_pos,
                &stack,
                anchor_position,
            )?
            else {
                line_tokens.produce(&stack, line_length);
                return Ok((stack, false));
            };
            let whole = capture_indices[0];
            let has_advanced = whole.end > line_pos;
            if matched_rule_id == END_RULE_ID {
                let popped_rule = self.rule(stack.rule_id);
                let RuleKind::BeginEnd { end_captures, .. } = &popped_rule.kind else {
                    return Err(unsupported());
                };
                line_tokens.produce(&stack, whole.start);
                stack = stack.with_content_name_scopes_list(stack.name_scopes_list.clone());
                self.handle_captures(
                    line,
                    is_first_line,
                    &stack,
                    line_tokens,
                    end_captures,
                    &capture_indices,
                )?;
                line_tokens.produce(&stack, whole.end);
                let popped = stack.clone();
                stack = stack.pop().expect("an end rule has a parent");
                anchor_position = popped.anchor_pos.get();
                if !has_advanced && popped.enter_pos.get() == line_pos as isize {
                    // Endless loop: the grammar pushed and popped a rule
                    // without advancing.
                    stack = popped;
                    line_tokens.produce(&stack, line_length);
                    return Ok((stack, false));
                }
            } else {
                let rule = self.rule(matched_rule_id);
                line_tokens.produce(&stack, whole.start);
                let before_push = stack.clone();
                let scope_name = rule.name(Some(line), Some(&capture_indices));
                let name_scopes_list = stack
                    .content_name_scopes_list
                    .push_attributed(scope_name.as_deref(), self);
                stack = stack.push(
                    matched_rule_id,
                    line_pos as isize,
                    anchor_position,
                    whole.end == line_length,
                    None,
                    name_scopes_list.clone(),
                    name_scopes_list.clone(),
                );
                match &rule.kind {
                    RuleKind::BeginEnd {
                        begin_captures,
                        end,
                        ..
                    }
                    | RuleKind::BeginWhile {
                        begin_captures,
                        while_: end,
                        ..
                    } => {
                        self.handle_captures(
                            line,
                            is_first_line,
                            &stack,
                            line_tokens,
                            begin_captures,
                            &capture_indices,
                        )?;
                        line_tokens.produce(&stack, whole.end);
                        anchor_position = whole.end as isize;
                        let content_name = rule.content_name(line, &capture_indices);
                        let content_name_scopes_list =
                            name_scopes_list.push_attributed(content_name.as_deref(), self);
                        stack = stack.with_content_name_scopes_list(content_name_scopes_list);
                        if end.has_back_references {
                            stack = stack
                                .with_end_rule(end.resolve_back_references(line, &capture_indices));
                        }
                        if !has_advanced && before_push.has_same_rule_as(&stack) {
                            // Endless loop: the same rule pushed without
                            // advancing.
                            stack = stack.pop().expect("pushed");
                            line_tokens.produce(&stack, line_length);
                            return Ok((stack, false));
                        }
                    }
                    RuleKind::Match { captures, .. } => {
                        self.handle_captures(
                            line,
                            is_first_line,
                            &stack,
                            line_tokens,
                            captures,
                            &capture_indices,
                        )?;
                        line_tokens.produce(&stack, whole.end);
                        stack = stack.pop().expect("pushed");
                        if !has_advanced {
                            // Endless loop: neither advancing nor pushing.
                            stack = stack.safe_pop();
                            line_tokens.produce(&stack, line_length);
                            return Ok((stack, false));
                        }
                    }
                    // Scanners list only match, begin/end and begin/while
                    // rules.
                    _ => return Err(unsupported()),
                }
            }
            if whole.end > line_pos {
                line_pos = whole.end;
                is_first_line = false;
            }
        }
    }

    /// `_checkWhileConditions`: (stack, linePos, anchorPosition, isFirstLine).
    fn check_while_conditions(
        &self,
        line: &str,
        mut is_first_line: bool,
        mut line_pos: usize,
        mut stack: Rc<StateStack>,
        line_tokens: &mut LineTokens,
    ) -> Result<(Rc<StateStack>, usize, isize, bool)> {
        let mut anchor_position: isize = if stack.begin_rule_captured_eol { 0 } else { -1 };
        let mut while_rules = Vec::new();
        let mut node = Some(stack.clone());
        while let Some(current) = node {
            let rule = self.rule(current.rule_id);
            if matches!(rule.kind, RuleKind::BeginWhile { .. }) {
                while_rules.push((rule, current.clone()));
            }
            node = current.pop();
        }
        while let Some((rule, rule_stack)) = while_rules.pop() {
            let scanner = self.compile_while_ag(
                &rule,
                rule_stack.end_rule.as_deref(),
                is_first_line,
                line_pos as isize == anchor_position,
            )?;
            match scanner.find_next_match(line, line_pos) {
                Some(found) => {
                    if found.rule_id != WHILE_RULE_ID {
                        stack = rule_stack.pop().expect("a while rule has a parent");
                        break;
                    }
                    if !found.capture_indices.is_empty() {
                        let whole = found.capture_indices[0];
                        let RuleKind::BeginWhile { while_captures, .. } = &rule.kind else {
                            unreachable!()
                        };
                        line_tokens.produce(&rule_stack, whole.start);
                        self.handle_captures(
                            line,
                            is_first_line,
                            &rule_stack,
                            line_tokens,
                            while_captures,
                            &found.capture_indices,
                        )?;
                        line_tokens.produce(&rule_stack, whole.end);
                        anchor_position = whole.end as isize;
                        if whole.end > line_pos {
                            line_pos = whole.end;
                            is_first_line = false;
                        }
                    }
                }
                None => {
                    stack = rule_stack.pop().expect("a while rule has a parent");
                    break;
                }
            }
        }
        Ok((stack, line_pos, anchor_position, is_first_line))
    }

    /// `matchRuleOrInjections`: (captures, rule id).
    fn match_rule_or_injections(
        &self,
        line: &str,
        is_first_line: bool,
        line_pos: usize,
        stack: &Rc<StateStack>,
        anchor_position: isize,
    ) -> Result<Option<(Vec<CaptureIndex>, RuleId)>> {
        let rule = self.rule(stack.rule_id);
        let scanner = self.compile_ag(
            &rule,
            stack.end_rule.as_deref(),
            is_first_line,
            line_pos as isize == anchor_position,
        )?;
        let match_result = scanner
            .find_next_match(line, line_pos)
            .map(|found| (found.capture_indices, found.rule_id));
        let injections = self.injections();
        if injections.is_empty() {
            return Ok(match_result);
        }
        let Some((priority_match, injection_captures, injection_rule)) = self.match_injections(
            &injections,
            line,
            is_first_line,
            line_pos,
            stack,
            anchor_position,
        )?
        else {
            return Ok(match_result);
        };
        let Some((captures, rule_id)) = match_result else {
            return Ok(Some((injection_captures, injection_rule)));
        };
        let match_score = captures[0].start;
        let injection_score = injection_captures[0].start;
        if injection_score < match_score || (priority_match && injection_score == match_score) {
            return Ok(Some((injection_captures, injection_rule)));
        }
        Ok(Some((captures, rule_id)))
    }

    /// `matchInjections`: (priorityMatch, captures, rule id).
    fn match_injections(
        &self,
        injections: &[Injection],
        line: &str,
        is_first_line: bool,
        line_pos: usize,
        stack: &Rc<StateStack>,
        anchor_position: isize,
    ) -> Result<Option<(bool, Vec<CaptureIndex>, RuleId)>> {
        let mut best_rating = usize::MAX;
        let mut best: Option<(Vec<CaptureIndex>, RuleId, i32)> = None;
        let scopes = stack.content_name_scopes_list.scope_names();
        for injection in injections {
            if !matches_scopes(&injection.matcher, &scopes) {
                continue;
            }
            let rule = self.rule(injection.rule_id);
            let scanner = self.compile_ag(
                &rule,
                None,
                is_first_line,
                line_pos as isize == anchor_position,
            )?;
            let Some(found) = scanner.find_next_match(line, line_pos) else {
                continue;
            };
            let rating = found.capture_indices[0].start;
            if rating >= best_rating {
                continue;
            }
            best_rating = rating;
            best = Some((found.capture_indices, found.rule_id, injection.priority));
            if best_rating == line_pos {
                break;
            }
        }
        Ok(best.map(|(captures, rule_id, priority)| (priority == -1, captures, rule_id)))
    }

    /// `handleCaptures`.
    fn handle_captures(
        &self,
        line: &str,
        is_first_line: bool,
        stack: &Rc<StateStack>,
        line_tokens: &mut LineTokens,
        captures: &[Option<RuleId>],
        capture_indices: &[CaptureIndex],
    ) -> Result<()> {
        if captures.is_empty() {
            return Ok(());
        }
        let len = captures.len().min(capture_indices.len());
        let mut local_stack: Vec<(Rc<AttributedScopeStack>, usize)> = Vec::new();
        let max_end = capture_indices[0].end;
        for i in 0..len {
            let Some(capture_rule_id) = captures[i] else {
                continue;
            };
            let capture_index = capture_indices[i];
            if capture_index.length() == 0 {
                continue;
            }
            if capture_index.start > max_end {
                break;
            }
            while let Some((scopes, end_pos)) = local_stack.last() {
                if *end_pos > capture_index.start {
                    break;
                }
                line_tokens.produce_from_scopes(scopes, *end_pos);
                local_stack.pop();
            }
            match local_stack.last() {
                Some((scopes, _)) => line_tokens.produce_from_scopes(scopes, capture_index.start),
                None => line_tokens.produce(stack, capture_index.start),
            }
            let capture_rule = self.rule(capture_rule_id);
            let RuleKind::Capture {
                retokenize_captured_with_rule_id: retokenize,
            } = capture_rule.kind
            else {
                return Err(unsupported());
            };
            if retokenize != 0 {
                let scope_name = capture_rule.name(Some(line), Some(capture_indices));
                let name_scopes_list = stack
                    .content_name_scopes_list
                    .push_attributed(scope_name.as_deref(), self);
                let content_name = capture_rule.content_name(line, capture_indices);
                let content_name_scopes_list =
                    name_scopes_list.push_attributed(content_name.as_deref(), self);
                let stack_clone = stack.push(
                    retokenize,
                    capture_index.start as isize,
                    -1,
                    false,
                    None,
                    name_scopes_list,
                    content_name_scopes_list,
                );
                self.tokenize_string(
                    &line[..capture_index.end],
                    is_first_line && capture_index.start == 0,
                    capture_index.start,
                    stack_clone,
                    line_tokens,
                    false,
                    0,
                )?;
                continue;
            }
            if let Some(scope_name) = capture_rule.name(Some(line), Some(capture_indices)) {
                let base = match local_stack.last() {
                    Some((scopes, _)) => scopes.clone(),
                    None => stack.content_name_scopes_list.clone(),
                };
                local_stack.push((
                    base.push_attributed(Some(&scope_name), self),
                    capture_index.end,
                ));
            }
        }
        while let Some((scopes, end_pos)) = local_stack.pop() {
            line_tokens.produce_from_scopes(&scopes, end_pos);
        }
        Ok(())
    }
}
