//! Raw grammars (`src/rawGrammar.ts`) read from JSON, with the JavaScript
//! truthiness and key order that `src/rule.ts` and `src/grammar` rely on.
//!
//! `initGrammar` deep-clones the grammar for each `Grammar` that uses it,
//! because `RuleFactory` caches rule ids on the raw rules. Parsing the JSON
//! again is that clone: every [`RawRule`] belongs to one grammar.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use serde_json::{Map, Value};

use super::utils::is_js_whitespace;

/// A `repository`: rule names to rules. `$self` and `$base` are entries.
pub type Repository = HashMap<String, Rc<RawRule>>;

/// `captures`, `beginCaptures`, …: keys in JavaScript's `for…in` order.
pub type RawCaptures = Rc<Vec<(String, Rc<RawRule>)>>;

/// `IRawRule`. Strings tested for truthiness are `None` when empty.
#[derive(Debug, Default)]
pub struct RawRule {
    /// The id `RuleFactory.getCompiledRuleId` stores on the rule (`desc.id`).
    pub id: Cell<Option<usize>>,
    pub include: Option<String>,
    pub name: Option<String>,
    pub content_name: Option<String>,
    pub match_: Option<String>,
    pub captures: Option<RawCaptures>,
    /// Present, even when empty: the factory tests `typeof begin`.
    pub begin: Option<String>,
    pub begin_captures: Option<RawCaptures>,
    pub end: Option<String>,
    pub end_captures: Option<RawCaptures>,
    pub while_: Option<String>,
    pub while_captures: Option<RawCaptures>,
    pub patterns: Option<Rc<Vec<Rc<RawRule>>>>,
    pub repository: Option<Rc<Repository>>,
    pub apply_end_pattern_last: bool,
}

/// A grammar after `initGrammar`.
#[derive(Debug)]
pub struct RawGrammar {
    pub scope_name: String,
    /// The grammar object itself used as a rule, as an injection grammar is
    /// by `collectInjections`: its `name` is the grammar's `name` field.
    pub as_rule: Rc<RawRule>,
    pub repository: Rc<Repository>,
    pub injections: Vec<(String, Rc<RawRule>)>,
    pub injection_selector: Option<String>,
}

fn truthy_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// An array index key (`"0"`, `"12"`), which `for…in` visits first, in
/// numeric order.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    key.parse::<u32>().ok().filter(|n| *n != u32::MAX)
}

/// The keys of a JSON object in JavaScript's property order.
pub fn js_keys(map: &Map<String, Value>) -> Vec<&String> {
    let mut indices: Vec<(u32, &String)> = map
        .keys()
        .filter_map(|k| array_index(k).map(|n| (n, k)))
        .collect();
    indices.sort_by_key(|(n, _)| *n);
    let mut keys: Vec<&String> = indices.into_iter().map(|(_, k)| k).collect();
    keys.extend(map.keys().filter(|k| array_index(k).is_none()));
    keys
}

/// `parseInt(text, 10)`; `None` for NaN.
pub fn parse_int(text: &str) -> Option<i64> {
    let text = text.trim_start_matches(is_js_whitespace);
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let digits: String = digits.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let value = digits.parse::<i64>().unwrap_or(i64::MAX);
    Some(if negative { -value } else { value })
}

fn parse_captures(value: Option<&Value>) -> Option<RawCaptures> {
    match value {
        Some(Value::Object(map)) => Some(Rc::new(
            js_keys(map)
                .into_iter()
                .map(|key| (key.clone(), parse_rule(&map[key])))
                .collect(),
        )),
        // Any other truthy value is an object without numeric keys.
        Some(value) if super::utils::truthy(value) => Some(Rc::new(Vec::new())),
        _ => None,
    }
}

fn parse_patterns(value: Option<&Value>) -> Option<Rc<Vec<Rc<RawRule>>>> {
    match value {
        Some(Value::Array(items)) => Some(Rc::new(items.iter().map(parse_rule).collect())),
        _ => None,
    }
}

fn parse_repository(value: Option<&Value>) -> Option<Repository> {
    match value {
        Some(Value::Object(map)) => Some(
            map.iter()
                .map(|(key, rule)| (key.clone(), parse_rule(rule)))
                .collect(),
        ),
        _ => None,
    }
}

pub fn parse_rule(value: &Value) -> Rc<RawRule> {
    let get = |key: &str| value.get(key);
    Rc::new(RawRule {
        id: Cell::new(None),
        include: truthy_string(get("include")),
        name: truthy_string(get("name")),
        content_name: truthy_string(get("contentName")),
        match_: truthy_string(get("match")),
        captures: parse_captures(get("captures")),
        begin: match get("begin") {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        },
        begin_captures: parse_captures(get("beginCaptures")),
        end: truthy_string(get("end")),
        end_captures: parse_captures(get("endCaptures")),
        while_: truthy_string(get("while")),
        while_captures: parse_captures(get("whileCaptures")),
        patterns: parse_patterns(get("patterns")),
        repository: parse_repository(get("repository")).map(Rc::new),
        apply_end_pattern_last: get("applyEndPatternLast").is_some_and(super::utils::truthy),
    })
}

/// `initGrammar(grammar, base)`.
pub fn init_grammar(json: &Value, base: Option<Rc<RawRule>>) -> Rc<RawGrammar> {
    let scope_name = json
        .get("scopeName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let patterns = parse_patterns(json.get("patterns"));
    let mut repository = parse_repository(json.get("repository")).unwrap_or_default();
    let self_rule = Rc::new(RawRule {
        patterns: patterns.clone(),
        name: Some(scope_name.clone()).filter(|s| !s.is_empty()),
        ..RawRule::default()
    });
    repository.insert("$self".into(), self_rule.clone());
    repository.insert("$base".into(), base.unwrap_or(self_rule));
    let repository = Rc::new(repository);
    let injections = match json.get("injections") {
        Some(Value::Object(map)) => js_keys(map)
            .into_iter()
            .map(|key| (key.clone(), parse_rule(&map[key])))
            .collect(),
        _ => Vec::new(),
    };
    let as_rule = Rc::new(RawRule {
        name: truthy_string(json.get("name")),
        content_name: truthy_string(json.get("contentName")),
        patterns,
        repository: Some(repository.clone()),
        ..RawRule::default()
    });
    Rc::new(RawGrammar {
        scope_name,
        as_rule,
        repository,
        injections,
        injection_selector: truthy_string(json.get("injectionSelector")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_javascript_property_order() {
        let value: Value = serde_json::from_str(r#"{"b":1,"10":2,"a":3,"2":4,"01":5}"#).unwrap();
        let keys: Vec<&str> = js_keys(value.as_object().unwrap())
            .into_iter()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["2", "10", "b", "a", "01"]);
        assert_eq!(parse_int(" 12x"), Some(12));
        assert_eq!(parse_int("x"), None);
    }
}
