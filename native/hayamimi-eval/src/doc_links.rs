//! Checks that every Markdown link of the repository points at a file that
//! exists.
//!
//! Two kinds of link are resolved:
//!
//! - a relative link (`../results/benchmarks.md`) against the directory of the
//!   file that holds it;
//! - a link to the repository's own `blob/main/` page on GitHub (a published
//!   package cannot hold a relative link out of itself) against the repository
//!   root, so a link to a file the tree no longer has fails too.
//!
//! Any other address (`https://`, `mailto:`, a bare `#anchor`) is not checked:
//! this is about paths inside the tree, not about the network. An anchor or a
//! query on a link is cut off and not verified.

use std::path::{Component, Path, PathBuf};

use crate::error::EvalError;

/// Directories that hold generated or vendored Markdown nobody edits, or data
/// that `git` does not carry (`target`, `models`, `testdata`).
const SKIP_DIRS: [&str; 8] = [
    ".dart_tool",
    "build",
    "node_modules",
    ".git",
    ".claude",
    "target",
    "models",
    "testdata",
];

/// The start of a link to a page of this repository on GitHub, after the owner.
const SELF_PAGE: &str = "hayamimi/blob/main/";

/// One link whose target does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broken {
    /// The Markdown file, relative to the repository root, with `/` separators.
    pub file: String,
    /// The line of the link, from 1.
    pub line: usize,
    /// The target as written.
    pub target: String,
}

/// The link starting at the `[` at byte `open`: its target and the byte after
/// the closing parenthesis. A link is `[text](target)` or `[text](target "title")`.
fn link_at(line: &str, open: usize) -> Option<(&str, usize)> {
    let close = open + 1 + line[open + 1..].find(']')?;
    let body = line[close + 1..].strip_prefix('(')?;
    let start = close + 2;
    let length = body.find(|c: char| c == ')' || c.is_whitespace())?;
    if length == 0 {
        return None;
    }
    let target = &body[..length];
    if body[length..].starts_with(')') {
        return Some((target, start + length + 1));
    }
    let closing = body[length..].find(')')?;
    Some((target, start + length + closing + 1))
}

/// The targets of the links on one line, left to right.
fn targets_in(line: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = line[from..].find('[') {
        let open = from + at;
        match link_at(line, open) {
            Some((target, end)) => {
                found.push(target);
                from = end;
            }
            None => from = open + 1,
        }
    }
    found
}

/// The target without the `<...>` that Markdown allows around it.
fn clean(target: &str) -> &str {
    let target = target.trim();
    match target
        .strip_prefix('<')
        .and_then(|inner| inner.strip_suffix('>'))
    {
        Some(inner) => inner,
        None => target,
    }
}

/// Whether the text starts with a URL scheme (`https:`, `mailto:`).
fn has_scheme(target: &str) -> bool {
    let mut chars = target.chars();
    if !chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
    {
        return false;
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')) {
            return false;
        }
    }
    false
}

/// The path inside the repository that a link to this repository's GitHub page names.
fn self_page_path(target: &str) -> Option<&str> {
    let after_host = target.strip_prefix("https://github.com/")?;
    let (_owner, page) = after_host.split_once('/')?;
    page.strip_prefix(SELF_PAGE)
}

fn hex_value(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}

/// The value of the `%XX` escape that starts at `at`, if there is one.
fn escape_at(bytes: &[u8], at: usize) -> Option<u8> {
    if bytes[at] != b'%' {
        return None;
    }
    let high = hex_value(*bytes.get(at + 1)?)?;
    let low = hex_value(*bytes.get(at + 2)?)?;
    Some(high * 16 + low)
}

/// The path with `%XX` escapes decoded as UTF-8 bytes; a stray `%` stays.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match escape_at(bytes, at) {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `path` with `.` and `..` resolved by reading it, without touching the disk.
fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The file a link names, or `None` for a link that is not checked.
fn resolve(file_dir: &Path, root: &Path, target: &str) -> Option<PathBuf> {
    let target = clean(target);
    if target.is_empty() || target.starts_with('#') {
        return None;
    }
    let (base, rest) = if let Some(rest) = self_page_path(target) {
        (root, rest)
    } else if has_scheme(target) || target.starts_with("//") {
        return None;
    } else if target.starts_with('/') {
        (root, target.trim_start_matches('/'))
    } else {
        (file_dir, target)
    };
    let rest = rest.split('#').next()?.split('?').next()?;
    if rest.is_empty() {
        return None;
    }
    Some(normalized(&base.join(percent_decoded(rest))))
}

/// A directory that holds `pyvenv.cfg` is a Python virtual environment,
/// whatever it is called; its packages carry Markdown of their own.
fn is_venv_root(directory: &Path) -> bool {
    directory.join("pyvenv.cfg").is_file()
}

fn is_markdown(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".md")
}

/// The Markdown files under `directory`: its own files first, sorted, then
/// each sub directory in sorted order.
fn markdown_files(directory: &Path, found: &mut Vec<PathBuf>) -> Result<(), EvalError> {
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let entries = std::fs::read_dir(directory).map_err(|e| EvalError::io(directory, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| EvalError::io(directory, e))?;
        let file_type = entry.file_type().map_err(|e| EvalError::io(directory, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if file_type.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) && !is_venv_root(&entry.path()) {
                directories.push(entry.path());
            }
        } else if is_markdown(&name) {
            files.push(entry.path());
        }
    }
    files.sort();
    directories.sort();
    found.extend(files);
    for sub in directories {
        markdown_files(&sub, found)?;
    }
    Ok(())
}

/// The path of `file` under `root`, with `/` separators; a path outside `root` is kept whole.
fn relative_name(root: &Path, file: &Path) -> String {
    let relative = match file.strip_prefix(root) {
        Ok(inside) => inside,
        Err(_) => file,
    };
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The broken links of one Markdown file.
fn broken_in(root: &Path, file: &Path) -> Result<Vec<Broken>, EvalError> {
    let text = std::fs::read_to_string(file).map_err(|e| EvalError::io(file, e))?;
    let Some(file_dir) = file.parent() else {
        return Err(EvalError::Invalid(format!(
            "{} has no parent directory",
            file.display()
        )));
    };
    let mut broken = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for target in targets_in(line) {
            let Some(resolved) = resolve(file_dir, root, target) else {
                continue;
            };
            if !resolved.exists() {
                broken.push(Broken {
                    file: relative_name(root, file),
                    line: index + 1,
                    target: clean(target).to_owned(),
                });
            }
        }
    }
    Ok(broken)
}

/// Every broken link in the Markdown files under `scan`. `root` is the
/// repository root that absolute and `blob/main/` links are resolved against.
pub fn check(root: &Path, scan: &Path) -> Result<Vec<Broken>, EvalError> {
    let mut files = Vec::new();
    markdown_files(scan, &mut files)?;
    let mut broken = Vec::new();
    for file in &files {
        broken.extend(broken_in(root, file)?);
    }
    Ok(broken)
}

/// The text the command prints: one line per broken link, then the verdict.
pub fn report(broken: &[Broken]) -> String {
    let mut text = String::new();
    for one in broken {
        text.push_str(&format!(
            "{}:{}: broken link -> {}\n",
            one.file, one.line, one.target
        ));
    }
    if broken.is_empty() {
        text.push_str("All Markdown links resolve.\n");
    } else {
        text.push_str(&format!("\n{} broken link(s).\n", broken.len()));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::scratch;

    /// A directory tree of the given files, each with its text.
    fn tree(name: &str, files: &[(&str, &str)]) -> Result<PathBuf, EvalError> {
        let root = scratch(name);
        if root.exists() {
            std::fs::remove_dir_all(&root).map_err(|e| EvalError::io(&root, e))?;
        }
        for (path, text) in files {
            let full = root.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).map_err(|e| EvalError::io(parent, e))?;
            }
            std::fs::write(&full, text).map_err(|e| EvalError::io(&full, e))?;
        }
        Ok(root)
    }

    /// The repository this crate sits in has no broken Markdown link.
    #[test]
    fn no_broken_markdown_links() -> Result<(), EvalError> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let broken = check(&root, &root)?;
        assert!(broken.is_empty(), "{}", report(&broken));
        Ok(())
    }

    #[test]
    fn a_line_without_a_link_has_no_target() {
        assert!(targets_in("").is_empty());
        assert!(targets_in("a [b] (c) and [d]").is_empty());
        assert!(targets_in("[a](").is_empty());
        assert!(targets_in("[a]()").is_empty());
    }

    #[test]
    fn every_link_of_a_line_is_found_in_order() {
        assert_eq!(
            targets_in("see [a](x.md) and [b](y/z.md \"title\") and ![i](p.png)."),
            ["x.md", "y/z.md", "p.png"]
        );
    }

    #[test]
    fn a_title_needs_its_closing_parenthesis() {
        assert!(targets_in("[a](x.md \"title\"").is_empty());
    }

    #[test]
    fn an_open_bracket_inside_the_text_belongs_to_the_text() {
        assert_eq!(targets_in("[a [b](x.md)"), ["x.md"]);
        assert!(targets_in("[[a]](x.md)").is_empty());
    }

    #[test]
    fn a_target_in_angle_brackets_is_unwrapped() {
        assert_eq!(clean(" <a b.md> "), "a b.md");
        assert_eq!(clean("a.md"), "a.md");
    }

    #[test]
    fn a_scheme_is_told_from_a_path_with_a_colon_later() {
        assert!(has_scheme("https://x"));
        assert!(has_scheme("mailto:a@b"));
        assert!(!has_scheme("docs/a:b.md"));
        assert!(!has_scheme("1http://x"));
        assert!(!has_scheme(":x"));
        assert!(!has_scheme(""));
    }

    #[test]
    fn a_link_to_the_own_github_page_is_a_path_of_the_tree() {
        assert_eq!(
            self_page_path("https://github.com/someone/hayamimi/blob/main/docs/a.md"),
            Some("docs/a.md")
        );
        assert_eq!(
            self_page_path("https://github.com/someone/other/blob/main/docs/a.md"),
            None
        );
        assert_eq!(self_page_path("https://example.com/a"), None);
    }

    #[test]
    fn percent_escapes_decode_as_utf8_and_a_stray_percent_stays() {
        assert_eq!(percent_decoded("a%20b%E3%81%82"), "a b\u{3042}");
        assert_eq!(percent_decoded("100%"), "100%");
        assert_eq!(percent_decoded("%zz%4"), "%zz%4");
    }

    #[test]
    fn dots_are_resolved_without_the_disk() {
        assert_eq!(normalized(Path::new("a/b/../c/./d")), Path::new("a/c/d"));
        assert_eq!(normalized(Path::new("../a")), Path::new("../a"));
    }

    #[test]
    fn links_that_are_not_checked_resolve_to_nothing() {
        let (dir, root) = (Path::new("/r/docs"), Path::new("/r"));
        for target in ["", "#top", "https://x/y", "mailto:a@b", "//cdn/x.js", "#"] {
            assert_eq!(resolve(dir, root, target), None, "{target}");
        }
        assert_eq!(
            resolve(dir, root, "a.md#part"),
            Some(PathBuf::from("/r/docs/a.md"))
        );
        assert_eq!(
            resolve(dir, root, "../README.md?x=1"),
            Some(PathBuf::from("/r/README.md"))
        );
        assert_eq!(
            resolve(dir, root, "/docs/a.md"),
            Some(PathBuf::from("/r/docs/a.md"))
        );
    }

    #[test]
    fn a_broken_relative_link_is_named_with_its_file_and_line() -> Result<(), EvalError> {
        let root = tree(
            "doc-links-broken",
            &[
                ("docs/a.md", "fine [b](b.md)\nbroken [c](gone.md)\n"),
                (
                    "docs/b.md",
                    "[up](../docs/a.md#x) [net](https://example.invalid/x)\n",
                ),
            ],
        )?;
        let broken = check(&root, &root)?;
        assert_eq!(
            broken,
            [Broken {
                file: String::from("docs/a.md"),
                line: 2,
                target: String::from("gone.md"),
            }]
        );
        Ok(())
    }

    #[test]
    fn a_page_of_the_own_repository_that_is_gone_is_broken() -> Result<(), EvalError> {
        let root = tree(
            "doc-links-self",
            &[
                (
                    "README.md",
                    "[a](https://github.com/o/hayamimi/blob/main/docs/a.md) [b](https://github.com/o/hayamimi/blob/main/docs/none.md)\n",
                ),
                ("docs/a.md", "text\n"),
            ],
        )?;
        let broken = check(&root, &root)?;
        assert_eq!(broken.len(), 1);
        assert!(broken[0].target.ends_with("docs/none.md"));
        Ok(())
    }

    #[test]
    fn skipped_directories_and_virtual_environments_are_not_read() -> Result<(), EvalError> {
        let root = tree(
            "doc-links-skip",
            &[
                ("target/x.md", "[a](gone.md)\n"),
                ("models/x.md", "[a](gone.md)\n"),
                ("venv/pyvenv.cfg", "home = x\n"),
                ("venv/pkg/Privacy.md", "[a](gone.md)\n"),
                ("docs/ok.md", "[a](ok.md)\n"),
            ],
        )?;
        assert!(check(&root, &root)?.is_empty());
        Ok(())
    }

    #[test]
    fn the_report_ends_with_the_verdict() {
        assert_eq!(report(&[]), "All Markdown links resolve.\n");
        let one = Broken {
            file: String::from("a.md"),
            line: 3,
            target: String::from("x.md"),
        };
        assert_eq!(
            report(&[one]),
            "a.md:3: broken link -> x.md\n\n1 broken link(s).\n"
        );
    }
}
