//! Quellcode-Inventar. Liest keine Prozessumgebung und keine Betriebsdateien.
//! Auch nichtliterale Argumente und Import-Aliasse werden als Leser erfasst.

use quote::ToTokens;
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use syn::{
    spanned::Spanned,
    visit::{self, Visit},
};

#[derive(Serialize)]
struct Reader {
    file: String,
    line: usize,
    function: String,
    operation: String,
    key: Option<String>,
    dynamic: bool,
}

#[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct Candidate {
    file: String,
    line: usize,
    function: String,
    key: String,
}

#[derive(Default, Serialize)]
struct Report {
    readers: Vec<Reader>,
    key_candidates: BTreeSet<Candidate>,
}

fn test_only(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        let name = attribute
            .path()
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        name == "test"
            || (name == "cfg" && attribute.meta.to_token_stream().to_string() == "cfg (test)")
    })
}

fn candidate_key(value: &str) -> bool {
    (value.contains('_') || matches!(value, "PORT" | "HOME" | "PATH" | "MINMAX"))
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
        && value.bytes().next().is_some_and(|c| c.is_ascii_uppercase())
}

struct Scanner<'a> {
    file: String,
    function: String,
    aliases: BTreeSet<String>,
    env_modules: BTreeSet<String>,
    report: &'a mut Report,
}

impl Scanner<'_> {
    fn import(&mut self, tree: &syn::UseTree, prefix: &str) {
        match tree {
            syn::UseTree::Path(path) => {
                self.import(&path.tree, &format!("{prefix}{}::", path.ident))
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.import(item, prefix);
                }
            }
            syn::UseTree::Name(name) => {
                if prefix.ends_with("env::") {
                    self.aliases.insert(name.ident.to_string());
                }
            }
            syn::UseTree::Rename(rename) => {
                if prefix.ends_with("env::") {
                    self.aliases.insert(rename.rename.to_string());
                }
                if prefix == "std::" && rename.ident == "env" {
                    self.env_modules.insert(rename.rename.to_string());
                }
            }
            syn::UseTree::Glob(_) => {
                if prefix.ends_with("env::") {
                    self.aliases
                        .extend(["var", "var_os", "vars", "vars_os"].map(str::to_string));
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attributes: &[syn::Attribute] = match item {
            syn::Item::Fn(v) => &v.attrs,
            syn::Item::Mod(v) => &v.attrs,
            syn::Item::Impl(v) => &v.attrs,
            syn::Item::Const(v) => &v.attrs,
            syn::Item::Static(v) => &v.attrs,
            syn::Item::Use(v) => &v.attrs,
            syn::Item::Macro(v) => &v.attrs,
            _ => &[],
        };
        if test_only(attributes) {
            return;
        }
        let previous = self.function.clone();
        if let syn::Item::Fn(function) = item {
            self.function = function.sig.ident.to_string();
        }
        visit::visit_item(self, item);
        self.function = previous;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if test_only(&item.attrs) {
            return;
        }
        let previous = std::mem::replace(&mut self.function, item.sig.ident.to_string());
        visit::visit_impl_item_fn(self, item);
        self.function = previous;
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.import(&item.tree, "");
    }

    fn visit_expr_call(&mut self, expression: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*expression.func {
            let names: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            let name = names.last().map(String::as_str).unwrap_or("");
            let env =
                names.iter().any(|s| self.env_modules.contains(s)) || self.aliases.contains(name);
            let read = matches!(name, "var" | "var_os" | "vars" | "vars_os")
                || self.aliases.contains(name);
            if env && read {
                let key = expression.args.first().and_then(|argument| match argument {
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(value),
                        ..
                    }) => Some(value.value()),
                    _ => None,
                });
                self.report.readers.push(Reader {
                    file: self.file.clone(),
                    line: expression.span().start().line,
                    function: self.function.clone(),
                    operation: names.join("::"),
                    dynamic: key.is_none(),
                    key,
                });
            }
        }
        visit::visit_expr_call(self, expression);
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        let key = literal.value();
        if candidate_key(&key) {
            self.report.key_candidates.insert(Candidate {
                file: self.file.clone(),
                line: literal.span().start().line,
                function: self.function.clone(),
                key,
            });
        }
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        fn tokens(scanner: &mut Scanner<'_>, stream: proc_macro2::TokenStream) {
            for token in stream {
                match token {
                    proc_macro2::TokenTree::Group(group) => tokens(scanner, group.stream()),
                    proc_macro2::TokenTree::Literal(literal) => {
                        if let Ok(value) = syn::parse_str::<syn::LitStr>(&literal.to_string()) {
                            let key = value.value();
                            if candidate_key(&key) {
                                scanner.report.key_candidates.insert(Candidate {
                                    file: scanner.file.clone(),
                                    line: literal.span().start().line,
                                    function: scanner.function.clone(),
                                    key,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        tokens(self, mac.tokens.clone());
        let text = mac.tokens.to_string();
        if ["env :: var", "env :: vars"]
            .iter()
            .any(|needle| text.contains(needle))
        {
            self.report.readers.push(Reader {
                file: self.file.clone(),
                line: mac.span().start().line,
                function: self.function.clone(),
                operation: "macro:manual-review".into(),
                key: None,
                dynamic: true,
            });
        }
    }
}

fn files(directory: &Path, result: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(
            name.as_ref(),
            "target" | "tests" | "test-support" | "vendor" | ".git"
        ) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            files(&entry.path(), result)?;
        } else if entry.path().extension().is_some_and(|ext| ext == "rs")
            && !name.ends_with("_tests.rs")
            && name != "test_support.rs"
            && !name.starts_with("build")
        {
            result.push(entry.path());
        }
    }
    Ok(())
}

fn run() -> Result<(), &'static str> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 2 {
        return Err("Aufruf: tb-config-inventory <Rust-Quellwurzel> <neue JSON-Ausgabedatei>");
    }
    let root = Path::new(&arguments[0])
        .canonicalize()
        .map_err(|_| "Quellwurzel ist nicht lesbar.")?;
    let mut paths = Vec::new();
    for directory in [root.join("bin"), root.join("crates")] {
        files(&directory, &mut paths).map_err(|_| "Quellverzeichnis ist nicht lesbar.")?;
    }
    paths.sort();
    let mut report = Report::default();
    for path in paths {
        let source = fs::read_to_string(&path).map_err(|_| "Quelldatei ist nicht lesbar.")?;
        let syntax = syn::parse_file(&source)
            .map_err(|_| "Rust-Quelle konnte nicht vollständig geparst werden.")?;
        let mut scanner = Scanner {
            file: path
                .strip_prefix(&root)
                .map_err(|_| "Quelldatei außerhalb der Wurzel.")?
                .to_string_lossy()
                .into_owned(),
            function: String::new(),
            aliases: BTreeSet::new(),
            env_modules: BTreeSet::from(["env".to_string()]),
            report: &mut report,
        };
        // Imports vorab auflösen, damit ihre Position im Modul keine Rolle spielt.
        for item in &syntax.items {
            if let syn::Item::Use(import) = item {
                scanner.import(&import.tree, "");
            }
        }
        scanner.visit_file(&syntax);
    }
    let output = fs::OpenOptions::new().write(true).create_new(true).open(&arguments[1])
        .map_err(|_| "Die neue Inventardatei konnte nicht angelegt werden; vorhandene Dateien werden nicht überschrieben.")?;
    serde_json::to_writer_pretty(output, &report)
        .map_err(|_| "Inventar konnte nicht geschrieben werden.")?;
    println!("Quellinventar: {} direkte Leser, {} Schlüsselvorkommen. Kandidaten brauchen fachliche Zuordnung.", report.readers.len(), report.key_candidates.len());
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
