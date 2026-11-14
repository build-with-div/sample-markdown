//! `SyncRegistry` (`src/registry.ts`) as Shiki's primitive fills it: every
//! language is known to the resolver before any grammar loads, so a
//! scope's injections are the resolver's `getInjections` over all of them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use serde_json::Value;

use super::theme::Theme;

pub struct SyncRegistry {
    raw_grammars: HashMap<String, Rc<Value>>,
    injections: HashMap<String, Vec<String>>,
    theme: RefCell<Rc<Theme>>,
}

impl SyncRegistry {
    /// `languages`: (scope name, grammar, `injectTo`) in load order.
    pub fn new(languages: &[(String, Rc<Value>, Vec<String>)], theme: Rc<Theme>) -> Self {
        // `Resolver._injections`: target scope → injecting scopes, in the
        // order the languages were added.
        let mut by_target: HashMap<&str, Vec<String>> = HashMap::new();
        for (scope_name, _, inject_to) in languages {
            for target in inject_to {
                let list = by_target.entry(target.as_str()).or_default();
                if !list.contains(scope_name) {
                    list.push(scope_name.clone());
                }
            }
        }
        // `Resolver.getInjections`: the injections of every prefix of the
        // scope, shortest first.
        let injections = languages
            .iter()
            .map(|(scope_name, _, _)| {
                let parts: Vec<&str> = scope_name.split('.').collect();
                let list = (1..=parts.len())
                    .flat_map(|i| {
                        by_target
                            .get(parts[..i].join(".").as_str())
                            .cloned()
                            .unwrap_or_default()
                    })
                    .collect();
                (scope_name.clone(), list)
            })
            .collect();
        Self {
            raw_grammars: languages
                .iter()
                .map(|(scope_name, grammar, _)| (scope_name.clone(), grammar.clone()))
                .collect(),
            injections,
            theme: RefCell::new(theme),
        }
    }

    pub fn lookup(&self, scope_name: &str) -> Option<Rc<Value>> {
        self.raw_grammars.get(scope_name).cloned()
    }

    pub fn injections(&self, scope_name: &str) -> Option<Vec<String>> {
        self.injections.get(scope_name).cloned()
    }

    pub fn theme(&self) -> Rc<Theme> {
        self.theme.borrow().clone()
    }

    pub fn set_theme(&self, theme: Rc<Theme>) {
        *self.theme.borrow_mut() = theme;
    }
}
