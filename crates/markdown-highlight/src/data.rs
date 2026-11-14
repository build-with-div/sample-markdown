//! The grammars and themes in `data/`, in Shiki's load order (listed in
//! `data/manifest.json`, which `tests/shiki.rs` checks this against).

pub const LANGS: &[(&str, &str)] = &[
    ("rust", include_str!("../data/langs/rust.json")),
    (
        "shellscript",
        include_str!("../data/langs/shellscript.json"),
    ),
    ("javascript", include_str!("../data/langs/javascript.json")),
    ("typescript", include_str!("../data/langs/typescript.json")),
    ("json", include_str!("../data/langs/json.json")),
    ("python", include_str!("../data/langs/python.json")),
    ("toml", include_str!("../data/langs/toml.json")),
    ("yaml", include_str!("../data/langs/yaml.json")),
    ("css", include_str!("../data/langs/css.json")),
    ("diff", include_str!("../data/langs/diff.json")),
    ("html", include_str!("../data/langs/html.json")),
];

pub const THEMES: &[(&str, &str)] = &[
    (
        "github-light",
        include_str!("../data/themes/github-light.json"),
    ),
    (
        "github-dark",
        include_str!("../data/themes/github-dark.json"),
    ),
];
