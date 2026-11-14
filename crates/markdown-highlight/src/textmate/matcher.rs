//! `src/matcher.ts`: scope selectors (`source.js - comment, L:text`).

/// A parsed selector expression.
#[derive(Debug)]
pub enum Matcher {
    /// `-x`: false when the operand is missing.
    Not(Option<Box<Matcher>>),
    And(Vec<Matcher>),
    Or(Vec<Matcher>),
    Names(Vec<String>),
}

impl Matcher {
    pub fn matches<T: ?Sized>(
        &self,
        input: &T,
        matches_name: &dyn Fn(&[String], &T) -> bool,
    ) -> bool {
        match self {
            Self::Not(operand) => operand
                .as_ref()
                .is_some_and(|operand| !operand.matches(input, matches_name)),
            Self::And(items) => items.iter().all(|m| m.matches(input, matches_name)),
            Self::Or(items) => items.iter().any(|m| m.matches(input, matches_name)),
            Self::Names(names) => matches_name(names, input),
        }
    }
}

/// `MatcherWithPriority`.
#[derive(Debug)]
pub struct MatcherWithPriority {
    pub matcher: Matcher,
    /// -1 for `L:`, 1 for `R:`, else 0.
    pub priority: i32,
}

/// `newTokenizer`: `/([LR]:|[\w\.:][\w\.:\-]*|[\,\|\-\(\)])/g`.
fn tokens(input: &str) -> Vec<String> {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == ':';
    let chars: Vec<char> = input.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (c == 'L' || c == 'R') && chars.get(i + 1) == Some(&':') {
            result.push(format!("{c}:"));
            i += 2;
        } else if word(c) {
            let start = i;
            i += 1;
            while i < chars.len() && (word(chars[i]) || chars[i] == '-') {
                i += 1;
            }
            result.push(chars[start..i].iter().collect());
        } else if ",|-()".contains(c) {
            result.push(c.to_string());
            i += 1;
        } else {
            i += 1;
        }
    }
    result
}

/// `isIdentifier`: contains a `[\w\.:]`.
fn is_identifier(token: Option<&str>) -> bool {
    token.is_some_and(|t| {
        t.chars()
            .any(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == ':')
    })
}

struct Parser {
    tokens: Vec<String>,
    position: usize,
}

impl Parser {
    fn token(&self) -> Option<&str> {
        self.tokens.get(self.position).map(String::as_str)
    }

    fn next(&mut self) {
        self.position += 1;
    }

    fn operand(&mut self) -> Option<Matcher> {
        if self.token() == Some("-") {
            self.next();
            let negated = self.operand();
            return Some(Matcher::Not(negated.map(Box::new)));
        }
        if self.token() == Some("(") {
            self.next();
            let inner = self.inner_expression();
            if self.token() == Some(")") {
                self.next();
            }
            return Some(inner);
        }
        if is_identifier(self.token()) {
            let mut identifiers = Vec::new();
            loop {
                identifiers.push(self.token().unwrap_or_default().to_owned());
                self.next();
                if !is_identifier(self.token()) {
                    break;
                }
            }
            return Some(Matcher::Names(identifiers));
        }
        None
    }

    fn conjunction(&mut self) -> Matcher {
        let mut matchers = Vec::new();
        while let Some(matcher) = self.operand() {
            matchers.push(matcher);
        }
        Matcher::And(matchers)
    }

    fn inner_expression(&mut self) -> Matcher {
        let mut matchers = Vec::new();
        loop {
            matchers.push(self.conjunction());
            if matches!(self.token(), Some("|") | Some(",")) {
                while matches!(self.token(), Some("|") | Some(",")) {
                    self.next();
                }
            } else {
                break;
            }
        }
        Matcher::Or(matchers)
    }
}

/// `createMatchers`.
pub fn create_matchers(selector: &str) -> Vec<MatcherWithPriority> {
    let mut parser = Parser {
        tokens: tokens(selector),
        position: 0,
    };
    let mut results = Vec::new();
    while let Some(token) = parser.token() {
        let mut priority = 0;
        let chars: Vec<char> = token.chars().collect();
        if chars.len() == 2 && chars[1] == ':' {
            priority = match chars[0] {
                'R' => 1,
                'L' => -1,
                _ => 0,
            };
            parser.next();
        }
        let matcher = parser.conjunction();
        results.push(MatcherWithPriority { matcher, priority });
        if parser.token() != Some(",") {
            break;
        }
        parser.next();
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names_match(identifiers: &[String], scopes: &[&str]) -> bool {
        // The grammar's `nameMatcher`: each identifier matches a later scope.
        let mut last = 0;
        identifiers.iter().all(|identifier| {
            for (i, scope) in scopes.iter().enumerate().skip(last) {
                if *scope == identifier || scope.starts_with(&format!("{identifier}.")) {
                    last = i + 1;
                    return true;
                }
            }
            false
        })
    }

    #[test]
    fn parses_priorities_negation_and_groups() {
        let matchers = create_matchers("L:source.js - (comment | string), R:text");
        assert_eq!(matchers.len(), 2);
        assert_eq!(matchers[0].priority, -1);
        assert_eq!(matchers[1].priority, 1);
        let f = |names: &[String], scopes: &[&str]| names_match(names, scopes);
        assert!(matchers[0]
            .matcher
            .matches(&["source.js", "meta.x"][..], &f));
        assert!(!matchers[0]
            .matcher
            .matches(&["source.js", "comment.line"][..], &f));
    }
}
