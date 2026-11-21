//! Reading `.ftl` files at build time and in the catalogue tests (not part of
//! the library: included by `build.rs` and `tests/catalogue.rs` with
//! `#[path]`).

#![allow(dead_code)]

use fluent_syntax::ast;
use std::path::{Path, PathBuf};

/// One message of a catalogue file.
pub struct Msg {
    pub id: String,
    /// Variables in order of first use (`$count`, `$name`).
    pub vars: Vec<String>,
    /// The English text as a doc comment (placeables as written).
    pub text: String,
    /// Literal text of the value, without placeables (for search tables).
    pub literal: String,
    /// Identifier keys of select variants (`[one]`, `[librarian]`; number keys like `[0]` left out).
    pub variant_keys: Vec<String>,
    pub has_attributes: bool,
    pub has_value: bool,
    pub file: String,
}

/// Every language directory under `locales/` with its `.ftl` files, English first.
pub fn languages(root: &Path) -> Vec<(String, Vec<PathBuf>)> {
    let mut langs: Vec<(String, Vec<PathBuf>)> = std::fs::read_dir(root)
        .expect("locales/ directory")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| {
            let mut files: Vec<PathBuf> = std::fs::read_dir(e.path())
                .expect("language directory")
                .filter_map(Result::ok)
                .map(|f| f.path())
                .filter(|p| p.extension().is_some_and(|x| x == "ftl"))
                .collect();
            files.sort();
            (e.file_name().to_string_lossy().into_owned(), files)
        })
        .filter(|(_, files)| !files.is_empty())
        .collect();
    langs.sort_by(|a, b| (a.0 != "en", &a.0).cmp(&(b.0 != "en", &b.0)));
    langs
}

/// Parses one file; a syntax error is an `Err` with a readable message.
pub fn messages(path: &Path) -> Result<Vec<Msg>, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let res = fluent_syntax::parser::parse(src.as_str())
        .map_err(|(_, errs)| format!("{}: {:?}", path.display(), errs.iter().map(|e| &e.kind).collect::<Vec<_>>()))?;
    let file = path.file_name().unwrap().to_string_lossy().into_owned();
    let mut out = Vec::new();
    for entry in &res.body {
        match entry {
            ast::Entry::Message(m) => {
                let mut vars = Vec::new();
                let mut text = String::new();
                let mut literal = String::new();
                let mut variant_keys = Vec::new();
                if let Some(p) = &m.value {
                    pattern(p, &mut vars, &mut text, &mut literal);
                    keys_of(p, &mut variant_keys);
                }
                out.push(Msg {
                    id: m.id.name.to_owned(),
                    vars,
                    text: text.trim().to_owned(),
                    literal,
                    variant_keys,
                    has_attributes: !m.attributes.is_empty(),
                    has_value: m.value.is_some(),
                    file: file.clone(),
                });
            }
            ast::Entry::Junk { content } => return Err(format!("{}: cannot parse `{content}`", path.display())),
            _ => {}
        }
    }
    Ok(out)
}

fn pattern(p: &ast::Pattern<&str>, vars: &mut Vec<String>, text: &mut String, literal: &mut String) {
    for el in &p.elements {
        match el {
            ast::PatternElement::TextElement { value } => {
                text.push_str(value);
                literal.push_str(value);
            }
            ast::PatternElement::Placeable { expression } => {
                text.push('{');
                expr(expression, vars, text, literal);
                text.push('}');
            }
        }
    }
}

fn expr(e: &ast::Expression<&str>, vars: &mut Vec<String>, text: &mut String, literal: &mut String) {
    match e {
        ast::Expression::Inline(i) => inline(i, vars, text, literal),
        ast::Expression::Select { selector, variants } => {
            inline(selector, vars, text, literal);
            text.push_str(" -> ");
            // Variables and literal words of every variant count; the text
            // shows the default variant.
            for v in variants {
                let mut t = String::new();
                pattern(&v.value, vars, &mut t, literal);
                literal.push(' ');
                if v.default {
                    text.push_str(t.trim());
                }
            }
        }
    }
}

fn keys_of(p: &ast::Pattern<&str>, keys: &mut Vec<String>) {
    for el in &p.elements {
        if let ast::PatternElement::Placeable { expression: ast::Expression::Select { variants, .. } } = el {
            for v in variants {
                if let ast::VariantKey::Identifier { name } = v.key
                    && !keys.iter().any(|k| k == name)
                {
                    keys.push(name.to_owned());
                }
                keys_of(&v.value, keys);
            }
        }
    }
}

fn inline(i: &ast::InlineExpression<&str>, vars: &mut Vec<String>, text: &mut String, literal: &mut String) {
    match i {
        ast::InlineExpression::VariableReference { id } => {
            if !vars.iter().any(|v| v == id.name) {
                vars.push(id.name.to_owned());
            }
            text.push('$');
            text.push_str(id.name);
        }
        ast::InlineExpression::StringLiteral { value } => {
            text.push('"');
            text.push_str(value);
            text.push('"');
            literal.push_str(value);
        }
        ast::InlineExpression::NumberLiteral { value } => text.push_str(value),
        ast::InlineExpression::FunctionReference { id, arguments } => {
            text.push_str(id.name);
            text.push('(');
            for a in &arguments.positional {
                inline(a, vars, text, literal);
            }
            for n in &arguments.named {
                inline(&n.value, vars, text, literal);
            }
            text.push(')');
        }
        ast::InlineExpression::MessageReference { id, .. } => text.push_str(id.name),
        ast::InlineExpression::TermReference { id, .. } => {
            text.push('-');
            text.push_str(id.name)
        }
        ast::InlineExpression::Placeable { expression } => expr(expression, vars, text, literal),
    }
}

/// Rust function name of a message id: `library-add-piece` ⇒ `library_add_piece`.
pub fn fn_name(id: &str) -> String {
    id.replace('-', "_")
}
