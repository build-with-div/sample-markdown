//! `src/theme.ts`: parsing a theme and resolving a scope path to a style.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

use serde_json::Value;

use super::utils::{js_strcmp, js_trim, str_arr_cmp, truthy};

/// `FontStyle`.
pub mod font_style {
    pub const NOT_SET: i32 = -1;
    pub const NONE: i32 = 0;
    pub const ITALIC: i32 = 1;
    pub const BOLD: i32 = 2;
    pub const UNDERLINE: i32 = 4;
    pub const STRIKETHROUGH: i32 = 8;
}

/// `ScopeStack`: a scope path, innermost last.
#[derive(Debug)]
pub struct ScopeStack {
    pub parent: Option<Rc<ScopeStack>>,
    pub scope_name: String,
}

impl ScopeStack {
    pub fn push(path: Option<Rc<ScopeStack>>, scope_names: &[String]) -> Option<Rc<ScopeStack>> {
        let mut path = path;
        for name in scope_names {
            path = Some(Rc::new(ScopeStack {
                parent: path,
                scope_name: name.clone(),
            }));
        }
        path
    }

    pub fn segments(self: &Rc<Self>) -> Vec<String> {
        let mut item = Some(self.clone());
        let mut result = Vec::new();
        while let Some(current) = item {
            result.push(current.scope_name.clone());
            item = current.parent.clone();
        }
        result.reverse();
        result
    }
}

fn scope_path_matches_parent_scopes(
    scope_path: Option<&Rc<ScopeStack>>,
    parent_scopes: &[String],
) -> bool {
    if parent_scopes.is_empty() {
        return true;
    }
    let mut scope_path = scope_path.cloned();
    let mut index = 0;
    while index < parent_scopes.len() {
        let mut scope_pattern = &parent_scopes[index];
        let mut scope_must_match = false;
        if scope_pattern == ">" {
            if index == parent_scopes.len() - 1 {
                return false;
            }
            index += 1;
            scope_pattern = &parent_scopes[index];
            scope_must_match = true;
        }
        while let Some(current) = &scope_path {
            if matches_scope(&current.scope_name, scope_pattern) {
                break;
            }
            if scope_must_match {
                return false;
            }
            scope_path = current.parent.clone();
        }
        match scope_path {
            None => return false,
            Some(current) => scope_path = current.parent.clone(),
        }
        index += 1;
    }
    true
}

fn matches_scope(scope_name: &str, scope_pattern: &str) -> bool {
    scope_pattern == scope_name
        || (scope_name.starts_with(scope_pattern)
            && scope_name.as_bytes().get(scope_pattern.len()) == Some(&b'.'))
}

/// `StyleAttributes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleAttributes {
    pub font_style: i32,
    pub foreground_id: u32,
    pub background_id: u32,
}

/// `ParsedThemeRule`.
#[derive(Clone, Debug)]
struct ParsedThemeRule {
    scope: String,
    parent_scopes: Option<Vec<String>>,
    index: usize,
    font_style: i32,
    foreground: Option<String>,
    background: Option<String>,
}

fn is_valid_hex_color(hex: &str) -> bool {
    let Some(digits) = hex.strip_prefix('#') else {
        return false;
    };
    matches!(digits.len(), 3 | 4 | 6 | 8) && digits.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `parseTheme`, over a theme's `settings` array.
fn parse_theme(settings: &[Value]) -> Vec<ParsedThemeRule> {
    let mut result = Vec::new();
    for (i, entry) in settings.iter().enumerate() {
        let Some(entry_settings) = entry.get("settings").filter(|s| truthy(s)) else {
            continue;
        };
        let scopes: Vec<String> = match entry.get("scope") {
            Some(Value::String(scope)) => {
                let scope = scope.trim_start_matches(',').trim_end_matches(',');
                scope.split(',').map(str::to_owned).collect()
            }
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| item.as_str().unwrap_or("").to_owned())
                .collect(),
            _ => vec![String::new()],
        };
        let mut font_style = font_style::NOT_SET;
        if let Some(Value::String(value)) = entry_settings.get("fontStyle") {
            font_style = font_style::NONE;
            for segment in value.split(' ') {
                match segment {
                    "italic" => font_style |= font_style::ITALIC,
                    "bold" => font_style |= font_style::BOLD,
                    "underline" => font_style |= font_style::UNDERLINE,
                    "strikethrough" => font_style |= font_style::STRIKETHROUGH,
                    _ => {}
                }
            }
        }
        let color = |key: &str| match entry_settings.get(key) {
            Some(Value::String(value)) if is_valid_hex_color(value) => Some(value.clone()),
            _ => None,
        };
        let foreground = color("foreground");
        let background = color("background");
        for scope in scopes {
            let segments: Vec<&str> = js_trim(&scope).split(' ').collect();
            let last = segments[segments.len() - 1].to_owned();
            let parent_scopes = (segments.len() > 1).then(|| {
                let mut parents: Vec<String> = segments[..segments.len() - 1]
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect();
                parents.reverse();
                parents
            });
            result.push(ParsedThemeRule {
                scope: last,
                parent_scopes,
                index: i,
                font_style,
                foreground: foreground.clone(),
                background: background.clone(),
            });
        }
    }
    result
}

/// `ColorMap`. Id 0 means "no color"; colors are stored upper-case.
#[derive(Debug)]
pub struct ColorMap {
    id2color: Vec<String>,
    color2id: HashMap<String, u32>,
}

impl ColorMap {
    fn new() -> Self {
        Self {
            id2color: vec![String::new()],
            color2id: HashMap::new(),
        }
    }

    fn id(&mut self, color: Option<&str>) -> u32 {
        let Some(color) = color else {
            return 0;
        };
        let color = color.to_uppercase();
        if let Some(&id) = self.color2id.get(&color) {
            return id;
        }
        let id = self.id2color.len() as u32;
        self.color2id.insert(color.clone(), id);
        self.id2color.push(color);
        id
    }
}

/// `ThemeTrieElementRule`.
#[derive(Clone, Debug)]
struct ThemeTrieElementRule {
    scope_depth: usize,
    parent_scopes: Vec<String>,
    font_style: i32,
    foreground: u32,
    background: u32,
}

impl ThemeTrieElementRule {
    fn accept_overwrite(
        &mut self,
        scope_depth: usize,
        font_style: i32,
        foreground: u32,
        background: u32,
    ) {
        if self.scope_depth <= scope_depth {
            self.scope_depth = scope_depth;
        }
        if font_style != font_style::NOT_SET {
            self.font_style = font_style;
        }
        if foreground != 0 {
            self.foreground = foreground;
        }
        if background != 0 {
            self.background = background;
        }
    }
}

/// `ThemeTrieElement`.
#[derive(Debug)]
struct ThemeTrieElement {
    main_rule: ThemeTrieElementRule,
    rules_with_parent_scopes: Vec<ThemeTrieElementRule>,
    children: HashMap<String, ThemeTrieElement>,
}

fn split_head(scope: &str) -> (&str, &str) {
    match scope.find('.') {
        None => (scope, ""),
        Some(dot) => (&scope[..dot], &scope[dot + 1..]),
    }
}

impl ThemeTrieElement {
    fn cmp_by_specificity(a: &ThemeTrieElementRule, b: &ThemeTrieElementRule) -> Ordering {
        if a.scope_depth != b.scope_depth {
            return b.scope_depth.cmp(&a.scope_depth);
        }
        let mut ai = 0;
        let mut bi = 0;
        loop {
            if a.parent_scopes.get(ai).is_some_and(|s| s == ">") {
                ai += 1;
            }
            if b.parent_scopes.get(bi).is_some_and(|s| s == ">") {
                bi += 1;
            }
            if ai >= a.parent_scopes.len() || bi >= b.parent_scopes.len() {
                break;
            }
            let b_len = b.parent_scopes[bi].encode_utf16().count();
            let a_len = a.parent_scopes[ai].encode_utf16().count();
            if b_len != a_len {
                return b_len.cmp(&a_len);
            }
            ai += 1;
            bi += 1;
        }
        b.parent_scopes.len().cmp(&a.parent_scopes.len())
    }

    fn match_scope(&self, scope: &str) -> Vec<ThemeTrieElementRule> {
        if !scope.is_empty() {
            let (head, tail) = split_head(scope);
            if let Some(child) = self.children.get(head) {
                return child.match_scope(tail);
            }
        }
        let mut rules = self.rules_with_parent_scopes.clone();
        rules.push(self.main_rule.clone());
        rules.sort_by(Self::cmp_by_specificity);
        rules
    }

    fn insert(
        &mut self,
        scope_depth: usize,
        scope: &str,
        parent_scopes: Option<&[String]>,
        font_style: i32,
        foreground: u32,
        background: u32,
    ) {
        if scope.is_empty() {
            self.do_insert_here(
                scope_depth,
                parent_scopes,
                font_style,
                foreground,
                background,
            );
            return;
        }
        let (head, tail) = split_head(scope);
        if !self.children.contains_key(head) {
            let child = ThemeTrieElement {
                main_rule: self.main_rule.clone(),
                rules_with_parent_scopes: self.rules_with_parent_scopes.clone(),
                children: HashMap::new(),
            };
            self.children.insert(head.to_owned(), child);
        }
        self.children.get_mut(head).expect("inserted above").insert(
            scope_depth + 1,
            tail,
            parent_scopes,
            font_style,
            foreground,
            background,
        );
    }

    fn do_insert_here(
        &mut self,
        scope_depth: usize,
        parent_scopes: Option<&[String]>,
        mut font_style: i32,
        mut foreground: u32,
        mut background: u32,
    ) {
        let Some(parent_scopes) = parent_scopes else {
            self.main_rule
                .accept_overwrite(scope_depth, font_style, foreground, background);
            return;
        };
        for rule in &mut self.rules_with_parent_scopes {
            if str_arr_cmp(Some(&rule.parent_scopes), Some(parent_scopes)) == Ordering::Equal {
                rule.accept_overwrite(scope_depth, font_style, foreground, background);
                return;
            }
        }
        if font_style == font_style::NOT_SET {
            font_style = self.main_rule.font_style;
        }
        if foreground == 0 {
            foreground = self.main_rule.foreground;
        }
        if background == 0 {
            background = self.main_rule.background;
        }
        self.rules_with_parent_scopes.push(ThemeTrieElementRule {
            scope_depth,
            parent_scopes: parent_scopes.to_vec(),
            font_style,
            foreground,
            background,
        });
    }
}

/// `Theme`.
#[derive(Debug)]
pub struct Theme {
    color_map: ColorMap,
    defaults: StyleAttributes,
    root: ThemeTrieElement,
    cache: RefCell<HashMap<String, Rc<Vec<ThemeTrieElementRule>>>>,
}

impl Theme {
    /// `Theme.createFromRawTheme`, from a theme's `settings` array.
    pub fn from_settings(settings: &[Value]) -> Self {
        let mut rules = parse_theme(settings);
        rules.sort_by(|a, b| {
            js_strcmp(&a.scope, &b.scope)
                .then_with(|| str_arr_cmp(a.parent_scopes.as_deref(), b.parent_scopes.as_deref()))
                .then_with(|| a.index.cmp(&b.index))
        });
        let mut default_font_style = font_style::NONE;
        let mut default_foreground = "#000000".to_owned();
        let mut default_background = "#ffffff".to_owned();
        let mut start = 0;
        while start < rules.len() && rules[start].scope.is_empty() {
            let incoming = &rules[start];
            if incoming.font_style != font_style::NOT_SET {
                default_font_style = incoming.font_style;
            }
            if let Some(foreground) = &incoming.foreground {
                default_foreground = foreground.clone();
            }
            if let Some(background) = &incoming.background {
                default_background = background.clone();
            }
            start += 1;
        }
        let mut color_map = ColorMap::new();
        let defaults = StyleAttributes {
            font_style: default_font_style,
            foreground_id: color_map.id(Some(&default_foreground)),
            background_id: color_map.id(Some(&default_background)),
        };
        let mut root = ThemeTrieElement {
            main_rule: ThemeTrieElementRule {
                scope_depth: 0,
                parent_scopes: Vec::new(),
                font_style: font_style::NOT_SET,
                foreground: 0,
                background: 0,
            },
            rules_with_parent_scopes: Vec::new(),
            children: HashMap::new(),
        };
        for rule in &rules[start..] {
            let foreground = color_map.id(rule.foreground.as_deref());
            let background = color_map.id(rule.background.as_deref());
            root.insert(
                0,
                &rule.scope,
                rule.parent_scopes.as_deref(),
                rule.font_style,
                foreground,
                background,
            );
        }
        Self {
            color_map,
            defaults,
            root,
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// `getColorMap`: index 0 is unused.
    pub fn color_map(&self) -> &[String] {
        &self.color_map.id2color
    }

    pub fn defaults(&self) -> StyleAttributes {
        self.defaults
    }

    /// `Theme.match`.
    pub fn match_path(&self, scope_path: Option<&Rc<ScopeStack>>) -> Option<StyleAttributes> {
        let Some(scope_path) = scope_path else {
            return Some(self.defaults);
        };
        let rules = self
            .cache
            .borrow_mut()
            .entry(scope_path.scope_name.clone())
            .or_insert_with(|| Rc::new(self.root.match_scope(&scope_path.scope_name)))
            .clone();
        let effective = rules.iter().find(|rule| {
            scope_path_matches_parent_scopes(scope_path.parent.as_ref(), &rule.parent_scopes)
        })?;
        Some(StyleAttributes {
            font_style: effective.font_style,
            foreground_id: effective.foreground,
            background_id: effective.background,
        })
    }
}
