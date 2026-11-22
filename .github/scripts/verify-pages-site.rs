// SPDX-License-Identifier: MPL-2.0
//
// Verify a rendered static site without network access or third-party crates.
//
// The checker validates the built artifact rather than its source fragment.
// It covers the failure modes that matter for GitHub Pages: missing entry
// points, broken repository-base paths, missing local assets and fragments,
// generic GitHub 404 output, and a compact set of document and accessibility
// invariants.
//
// Build and run (std only, no Cargo project needed):
//   rustc -O --edition 2021 -o verify-pages-site .github/scripts/verify-pages-site.rs
//   ./verify-pages-site --self-test
//   ./verify-pages-site _site --base-path /repo/ --expected-marker "text"
//
// This is a port of verify-pages-site.py. Entity decoding covers the numeric
// forms and the common named ones (amp, lt, gt, quot, apos, nbsp), which is
// all a static Pages build emits.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

const EXTERNAL_SCHEMES: [&str; 7] = ["blob", "data", "http", "https", "javascript", "mailto", "tel"];
const GITHUB_404_MARKERS: [&str; 2] = ["There isn't a GitHub Pages site here", "File not found"];

type Attrs = HashMap<String, String>;

#[derive(Clone)]
struct Reference {
    value: String,
    line: usize,
    attribute: &'static str,
}

struct Interactive {
    tag: String,
    line: usize,
    attrs: Attrs,
    text: Vec<String>,
}

#[derive(Default)]
struct Doc {
    references: Vec<Reference>,
    ids: HashMap<String, usize>,
    duplicate_ids: Vec<(String, usize)>,
    html_lang: String,
    has_charset: bool,
    has_viewport: bool,
    main_count: usize,
    h1_count: usize,
    title: String,
    labels_for: HashSet<String>,
    images: Vec<(usize, Attrs)>,
    canvases: Vec<(usize, Attrs)>,
    form_controls: Vec<(String, usize, Attrs)>,
    interactive: Vec<Interactive>,
    stack: Vec<usize>,
    in_title: bool,
    in_style: bool,
}

// ---------------------------------------------------------------- helpers

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let body = &rest[1..];
        let mut done = false;
        if let Some(num) = body.strip_prefix('#') {
            let (digits, radix) = match num.strip_prefix(['x', 'X']) {
                Some(h) => (h, 16),
                None => (num, 10),
            };
            let n = digits.chars().take_while(|c| c.is_digit(radix)).count();
            if n > 0 {
                if let Some(c) = u32::from_str_radix(&digits[..n], radix).ok().and_then(char::from_u32) {
                    out.push(c);
                    let used = 1 + (num.len() - digits.len()) + 1 + n;
                    rest = &rest[used..];
                    if rest.starts_with(';') {
                        rest = &rest[1..];
                    }
                    done = true;
                }
            }
        } else {
            for (name, ch) in [("amp", '&'), ("lt", '<'), ("gt", '>'), ("quot", '"'), ("apos", '\''), ("nbsp", '\u{a0}')] {
                if let Some(after) = body.strip_prefix(name) {
                    if let Some(after) = after.strip_prefix(';') {
                        out.push(ch);
                        rest = after;
                        done = true;
                        break;
                    }
                    if matches!(name, "amp" | "lt" | "gt" | "quot") {
                        out.push(ch);
                        rest = after;
                        done = true;
                        break;
                    }
                }
            }
        }
        if !done {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn unquote(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = h {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ----------------------------------------------------------- CSS url(...)

fn css_urls(text: &str) -> Vec<String> {
    let lower: Vec<char> = text.to_lowercase().chars().collect();
    let chars: Vec<char> = text.chars().collect();
    // to_lowercase can change length for exotic characters; fall back safely.
    let lower = if lower.len() == chars.len() { lower } else { chars.clone() };
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= chars.len() {
        if lower[i..i + 4] != ['u', 'r', 'l', '('] {
            i += 1;
            continue;
        }
        let mut j = i + 4;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        let mut matched: Option<(String, usize)> = None;
        if j < chars.len() && (chars[j] == '\'' || chars[j] == '"') {
            let q = chars[j];
            // quoted: smallest content (no newline) then q, optional blanks, ')'
            let mut k = j + 1;
            while k < chars.len() && chars[k] != '\n' {
                if chars[k] == q {
                    let mut m = k + 1;
                    while m < chars.len() && chars[m].is_whitespace() {
                        m += 1;
                    }
                    if m < chars.len() && chars[m] == ')' {
                        matched = Some((chars[j + 1..k].iter().collect(), m + 1));
                        break;
                    }
                }
                k += 1;
            }
        }
        if matched.is_none() {
            // unquoted: content up to the first ')' on the same line
            let mut k = j;
            while k < chars.len() && chars[k] != '\n' {
                if chars[k] == ')' {
                    matched = Some((chars[j..k].iter().collect(), k + 1));
                    break;
                }
                k += 1;
            }
        }
        match matched {
            Some((value, end)) => {
                let v = value.trim().to_string();
                if !v.is_empty() {
                    out.push(v);
                }
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

// ------------------------------------------------------------ HTML parser

impl Doc {
    fn collect_css(&mut self, text: &str, line: usize) {
        for v in css_urls(text) {
            self.references.push(Reference { value: v, line, attribute: "css-url" });
        }
    }

    fn start(&mut self, tag: &str, attrs: Attrs, line: usize) {
        if let Some(id) = attrs.get("id").filter(|v| !v.is_empty()) {
            if self.ids.contains_key(id) {
                self.duplicate_ids.push((id.clone(), line));
            } else {
                self.ids.insert(id.clone(), line);
            }
        }
        let get = |k: &str| attrs.get(k).map(String::as_str).unwrap_or("");
        match tag {
            "html" => self.html_lang = get("lang").trim().to_string(),
            "meta" => {
                self.has_charset = self.has_charset || !get("charset").trim().is_empty();
                self.has_viewport = self.has_viewport
                    || (get("name").to_lowercase() == "viewport" && !get("content").trim().is_empty());
            }
            "main" => self.main_count += 1,
            "h1" => self.h1_count += 1,
            "title" => self.in_title = true,
            "style" => self.in_style = true,
            "label" if !get("for").is_empty() => {
                self.labels_for.insert(get("for").to_string());
            }
            "img" => self.images.push((line, attrs.clone())),
            "canvas" => self.canvases.push((line, attrs.clone())),
            "input" | "select" | "textarea" => self.form_controls.push((tag.to_string(), line, attrs.clone())),
            _ => {}
        }
        if tag == "a" || tag == "button" {
            self.interactive.push(Interactive { tag: tag.to_string(), line, attrs: attrs.clone(), text: Vec::new() });
            self.stack.push(self.interactive.len() - 1);
        }
        for attribute in ["href", "src", "poster", "action"] {
            if let Some(v) = attrs.get(attribute) {
                if !v.trim().is_empty() {
                    self.references.push(Reference { value: v.trim().to_string(), line, attribute });
                }
            }
        }
        if let Some(srcset) = attrs.get("srcset").filter(|v| !v.is_empty()) {
            for candidate in srcset.split(',') {
                if let Some(first) = candidate.split_whitespace().next() {
                    self.references.push(Reference { value: first.to_string(), line, attribute: "srcset" });
                }
            }
        }
        if let Some(style) = attrs.get("style").filter(|v| !v.is_empty()) {
            let style = style.clone();
            self.collect_css(&style, line);
        }
    }

    fn end(&mut self, tag: &str) {
        match tag {
            "title" => self.in_title = false,
            "style" => self.in_style = false,
            "a" | "button" => {
                self.stack.pop();
            }
            _ => {}
        }
    }

    fn data(&mut self, text: &str, line: usize) {
        if self.in_title {
            self.title.push_str(text);
        }
        if self.in_style {
            self.collect_css(text, line);
        }
        for &idx in &self.stack {
            self.interactive[idx].text.push(text.to_string());
        }
    }
}

fn find_ci(hay: &str, needle: &str, from: usize) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (from..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

fn parse_html(text: &str) -> Doc {
    let mut doc = Doc::default();
    let b = text.as_bytes();
    let mut i = 0;
    let mut line = 1usize;
    let mut cdata: Option<String> = None;

    // Move to byte offset `to`, counting newlines.
    macro_rules! advance {
        ($to:expr) => {{
            let to = $to;
            line += b[i..to].iter().filter(|&&c| c == b'\n').count();
            i = to;
        }};
    }

    while i < b.len() {
        if let Some(tag) = cdata.clone() {
            let close = format!("</{tag}");
            let j = find_ci(text, &close, i).unwrap_or(b.len());
            if j > i {
                let chunk = &text[i..j];
                doc.data(chunk, line);
                advance!(j);
            }
            cdata = None;
            if i >= b.len() {
                break;
            }
        }
        let j = text[i..].find('<').map(|p| p + i).unwrap_or(b.len());
        if j > i {
            let chunk = unescape(&text[i..j]);
            doc.data(&chunk, line);
            advance!(j);
        }
        if i >= b.len() {
            break;
        }
        let rest = &text[i..];
        if rest.starts_with("<!--") {
            match text[i + 4..].find("-->") {
                Some(p) => advance!(i + 4 + p + 3),
                None => advance!(b.len()),
            }
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            match rest.find('>') {
                Some(p) => advance!(i + p + 1),
                None => advance!(b.len()),
            }
        } else if rest.starts_with("</") {
            let name: String = rest[2..].chars().take_while(|c| !c.is_whitespace() && *c != '/' && *c != '>').collect();
            if rest[2..].starts_with(|c: char| c.is_ascii_alphabetic()) {
                doc.end(&name.to_lowercase());
            }
            match rest.find('>') {
                Some(p) => advance!(i + p + 1),
                None => advance!(b.len()),
            }
        } else if rest[1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            let tag_line = line;
            let name: String = rest[1..].chars().take_while(|c| !c.is_whitespace() && *c != '/' && *c != '>').collect();
            let mut p = i + 1 + name.len();
            let mut attrs = Attrs::new();
            let mut self_closing = false;
            loop {
                while p < b.len() && (b[p] as char).is_ascii_whitespace() {
                    p += 1;
                }
                if p >= b.len() {
                    break;
                }
                if b[p] == b'>' {
                    p += 1;
                    break;
                }
                if b[p] == b'/' {
                    if p + 1 < b.len() && b[p + 1] == b'>' {
                        self_closing = true;
                        p += 2;
                        break;
                    }
                    p += 1;
                    continue;
                }
                let start = p;
                p += 1;
                while p < b.len() && !(b[p] as char).is_ascii_whitespace() && !matches!(b[p], b'/' | b'=' | b'>') {
                    p += 1;
                }
                // multi-byte names: p is on an ASCII delimiter or a char boundary
                let aname = text[start..p].to_lowercase();
                while p < b.len() && (b[p] as char).is_ascii_whitespace() {
                    p += 1;
                }
                let mut value = String::new();
                if p < b.len() && b[p] == b'=' {
                    p += 1;
                    while p < b.len() && (b[p] as char).is_ascii_whitespace() {
                        p += 1;
                    }
                    if p < b.len() && (b[p] == b'"' || b[p] == b'\'') {
                        let q = b[p];
                        let vs = p + 1;
                        let ve = b[vs..].iter().position(|&c| c == q).map(|x| x + vs).unwrap_or(b.len());
                        value = unescape(&text[vs..ve]);
                        p = (ve + 1).min(b.len());
                    } else {
                        let vs = p;
                        while p < b.len() && !(b[p] as char).is_ascii_whitespace() && b[p] != b'>' {
                            p += 1;
                        }
                        value = unescape(&text[vs..p]);
                    }
                }
                attrs.insert(aname, value);
            }
            let lname = name.to_lowercase();
            doc.start(&lname, attrs, tag_line);
            if !self_closing && (lname == "script" || lname == "style") {
                cdata = Some(lname);
            }
            advance!(p.min(b.len()));
        } else {
            doc.data("<", line);
            advance!(i + 1);
        }
    }
    doc
}

fn parse_document(path: &Path, errors: &mut Vec<String>) -> (Doc, String) {
    match fs::read(path).map_err(|e| e.to_string()).and_then(|raw| String::from_utf8(raw).map_err(|e| e.to_string())) {
        Ok(text) => (parse_html(&text), text),
        Err(e) => {
            errors.push(format!("{}: cannot read as UTF-8 HTML: {e}", path.display()));
            (Doc::default(), String::new())
        }
    }
}

// ------------------------------------------------------------- the checks

fn normalize_base_path(value: &str) -> String {
    let base = format!("/{}", value.trim_matches('/'));
    if base == "/" {
        base
    } else {
        base + "/"
    }
}

fn has_accessible_name(attrs: &Attrs) -> bool {
    ["aria-label", "aria-labelledby", "title"]
        .iter()
        .any(|k| attrs.get(*k).is_some_and(|v| !v.trim().is_empty()))
}

fn document_errors(path: &Path, d: &Doc) -> Vec<String> {
    let mut e = Vec::new();
    let p = path.display().to_string();
    if d.html_lang.is_empty() {
        e.push(format!("{p}: <html> must declare a non-empty lang attribute"));
    }
    if !d.has_charset {
        e.push(format!("{p}: document must declare a character encoding"));
    }
    if !d.has_viewport {
        e.push(format!("{p}: document must declare a viewport"));
    }
    if d.title.trim().is_empty() {
        e.push(format!("{p}: document must have a non-empty <title>"));
    }
    if d.main_count != 1 {
        e.push(format!("{p}: expected exactly one <main>, found {}", d.main_count));
    }
    if d.h1_count != 1 {
        e.push(format!("{p}: expected exactly one <h1>, found {}", d.h1_count));
    }
    for (id, line) in &d.duplicate_ids {
        e.push(format!("{p}:{line}: duplicate id #{id}"));
    }
    for (line, attrs) in &d.images {
        if !attrs.contains_key("alt") {
            e.push(format!("{p}:{line}: <img> must provide alt text (empty is valid for decorative images)"));
        }
    }
    for (line, attrs) in &d.canvases {
        if !has_accessible_name(attrs) {
            e.push(format!("{p}:{line}: <canvas> must have an accessible name"));
        }
    }
    for (tag, line, attrs) in &d.form_controls {
        if tag == "input" && attrs.get("type").map(|t| t.to_lowercase()).as_deref() == Some("hidden") {
            continue;
        }
        let id = attrs.get("id").map(String::as_str).unwrap_or("");
        if !has_accessible_name(attrs) && !d.labels_for.contains(id) {
            e.push(format!("{p}:{line}: <{tag}> must have an accessible name or associated label"));
        }
    }
    for el in &d.interactive {
        if el.tag == "a" && el.attrs.get("href").map_or(true, |h| h.trim().is_empty()) {
            e.push(format!("{p}:{}: <a> must have a non-empty href", el.line));
        }
        if el.text.join(" ").trim().is_empty() && !has_accessible_name(&el.attrs) {
            e.push(format!("{p}:{}: <{}> must have an accessible name", el.line, el.tag));
        }
    }
    let mut labelled: Vec<(String, usize)> = Vec::new();
    for (_, line, attrs) in &d.form_controls {
        for v in attrs.get("aria-labelledby").map(String::as_str).unwrap_or("").split_whitespace() {
            labelled.push((v.to_string(), *line));
        }
    }
    for (line, attrs) in &d.canvases {
        for v in attrs.get("aria-labelledby").map(String::as_str).unwrap_or("").split_whitespace() {
            labelled.push((v.to_string(), *line));
        }
    }
    for (v, line) in labelled {
        if !d.ids.contains_key(&v) {
            e.push(format!("{p}:{line}: aria-labelledby references missing id #{v}"));
        }
    }
    e
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn resolve(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(path) };
    let norm = normalize(&abs);
    fs::canonicalize(&norm).unwrap_or(norm)
}

struct Target {
    path: Option<PathBuf>,
    fragment: String,
    problem: Option<String>,
}

fn resolve_local_target(root: &Path, document: &Path, reference: &Reference, base: &str) -> Target {
    let none = |problem: Option<String>| Target { path: None, fragment: String::new(), problem };
    let value = reference.value.trim();
    if value.starts_with("//") {
        return none(None);
    }
    // urlsplit drops tabs and newlines anywhere in the value.
    let cleaned: String = value.chars().filter(|c| !matches!(c, '\t' | '\r' | '\n')).collect();
    let (scheme, after) = match cleaned.find(':') {
        Some(p)
            if p > 0
                && cleaned.as_bytes()[0].is_ascii_alphabetic()
                && cleaned[..p].chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) =>
        {
            (cleaned[..p].to_lowercase(), &cleaned[p + 1..])
        }
        _ => (String::new(), cleaned.as_str()),
    };
    if EXTERNAL_SCHEMES.contains(&scheme.as_str()) {
        return none(None);
    }
    if !scheme.is_empty() {
        return none(Some(format!("unsupported URL scheme in {}", py_repr(value))));
    }
    let (before_frag, fragment) = match after.split_once('#') {
        Some((a, f)) => (a, f),
        None => (after, ""),
    };
    let path_part = before_frag.split('?').next().unwrap_or("");
    let decoded = unquote(path_part);
    let mut target: PathBuf;
    if decoded.starts_with('/') {
        if base != "/" && !decoded.starts_with(base) {
            return none(Some(format!(
                "root-relative path {} escapes repository base {}",
                py_repr(&decoded),
                py_repr(base)
            )));
        }
        let relative: String = if base != "/" { decoded.chars().skip(base.chars().count()).collect() } else { decoded[1..].to_string() };
        target = root.join(relative);
    } else if !decoded.is_empty() {
        target = document.parent().unwrap_or(root).join(&decoded);
    } else {
        target = document.to_path_buf();
    }
    target = resolve(&target);
    if !target.starts_with(root) {
        return none(Some(format!("path {} escapes the rendered site", py_repr(value))));
    }
    if decoded.ends_with('/') || target.is_dir() {
        target = target.join("index.html");
    }
    Target { path: Some(target), fragment: unquote(fragment), problem: None }
}

fn collect_html(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                collect_html(&p, out);
            } else if p.is_file() && p.extension().is_some_and(|e| e == "html") {
                out.push(p);
            }
        }
    }
}

fn verify_site(site: &Path, base_path: &str, expected: &[String]) -> Vec<String> {
    let mut errors = Vec::new();
    let root = resolve(site);
    let index = root.join("index.html");
    if !root.is_dir() {
        return vec![format!("{}: rendered site directory does not exist", root.display())];
    }
    if !index.is_file() || fs::metadata(&index).map(|m| m.len() == 0).unwrap_or(true) {
        return vec![format!("{}: non-empty Pages entry point is required", index.display())];
    }
    let mut html_files = Vec::new();
    collect_html(&root, &mut html_files);
    html_files.sort_by(|a, b| a.components().cmp(b.components()));
    if html_files.is_empty() {
        return vec![format!("{}: rendered site contains no HTML documents", root.display())];
    }

    // Insertion-ordered map of resolved path -> (parsed, text).
    let mut order: Vec<PathBuf> = Vec::new();
    let mut docs: HashMap<PathBuf, (Doc, String)> = HashMap::new();
    for path in &html_files {
        let (parsed, text) = parse_document(path, &mut errors);
        let rel = path.strip_prefix(&root).unwrap_or(path);
        errors.extend(document_errors(rel, &parsed));
        let key = resolve(path);
        if !docs.contains_key(&key) {
            order.push(key.clone());
        }
        docs.insert(key, (parsed, text));
    }

    let index_text = docs.get(&resolve(&index)).map(|d| d.1.clone()).unwrap_or_default();
    for marker in GITHUB_404_MARKERS {
        if index_text.to_lowercase().contains(&marker.to_lowercase()) {
            errors.push(format!("index.html: contains generic error marker {}", py_repr(marker)));
        }
    }
    for marker in expected {
        if !index_text.contains(marker.as_str()) {
            errors.push(format!("index.html: expected content marker not found: {}", py_repr(marker)));
        }
    }

    let base = normalize_base_path(base_path);
    let mut n = 0;
    while n < order.len() {
        let document = order[n].clone();
        n += 1;
        let display = document.strip_prefix(&root).unwrap_or(&document).display().to_string();
        let refs = docs[&document].0.references.clone();
        for reference in refs {
            let t = resolve_local_target(&root, &document, &reference, &base);
            if let Some(problem) = t.problem {
                errors.push(format!("{display}:{}: {problem}", reference.line));
                continue;
            }
            let Some(target) = t.path else { continue };
            if !target.is_file() {
                errors.push(format!("{display}:{}: {} target not found: {}", reference.line, reference.attribute, py_repr(&reference.value)));
                continue;
            }
            if fs::metadata(&target).map(|m| m.len() == 0).unwrap_or(false) {
                errors.push(format!("{display}:{}: {} target is empty: {}", reference.line, reference.attribute, py_repr(&reference.value)));
                continue;
            }
            let ext = target.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !t.fragment.is_empty() && (ext == "htm" || ext == "html") {
                let key = resolve(&target);
                if !docs.contains_key(&key) {
                    let parsed = parse_document(&target, &mut errors);
                    docs.insert(key.clone(), parsed);
                    order.push(key.clone());
                }
                if !docs[&key].0.ids.contains_key(&t.fragment) {
                    errors.push(format!("{display}:{}: fragment target not found: {}", reference.line, py_repr(&reference.value)));
                }
            }
        }
    }
    errors
}

// -------------------------------------------------------------- self test

fn write(path: &Path, text: &str) {
    fs::write(path, text).expect("write fixture");
}

fn self_test() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("pages-verifier-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let result = (|| {
        write(&root.join("asset.txt"), "asset\n");
        write(
            &root.join("about.html"),
            "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>About</title></head>\n<body><main><h1 id=\"about\">About</h1></main></body></html>\n",
        );
        write(
            &root.join("index.html"),
            "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>Pilot</title></head>\n<body><main><h1>Pilot marker</h1><a href=\"about.html#about\">About</a><a href=\"asset.txt\">Asset</a></main></body></html>\n",
        );
        let ok = verify_site(&root, "/pilot/", &["Pilot marker".to_string()]);
        if !ok.is_empty() {
            return Err(format!("known-good fixture failed: {ok:?}"));
        }
        write(
            &root.join("index.html"),
            "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>Broken</title></head>\n<body><main><h1>Broken</h1><a href=\"missing.png\">Missing</a></main></body></html>\n",
        );
        let broken = verify_site(&root, "/pilot/", &[]);
        if !broken.iter().any(|e| e.contains("target not found")) {
            return Err("broken-link positive control was not detected".into());
        }
        write(
            &root.join("index.html"),
            "<!doctype html><html><head><title>Broken accessibility</title></head>\n<body><h1>Broken</h1><img src=\"asset.txt\"></body></html>\n",
        );
        let acc = verify_site(&root, "/pilot/", &[]);
        for item in ["lang attribute", "character encoding", "viewport", "exactly one <main>", "provide alt text"] {
            if !acc.iter().any(|e| e.contains(item)) {
                return Err("accessibility positive control was not fully detected".into());
            }
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&root);
    result
}

// ------------------------------------------------------------------- main

fn usage() -> &'static str {
    "usage: verify-pages-site [-h] [--base-path BASE_PATH] [--expected-marker EXPECTED_MARKER] [--self-test] [site]"
}

fn main() -> ExitCode {
    let mut site: Option<String> = None;
    let mut base_path = "/".to_string();
    let mut markers: Vec<String> = Vec::new();
    let mut run_self_test = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        match flag.as_str() {
            "-h" | "--help" => {
                println!("{}\n\nVerify a rendered static site without network access or third-party packages.", usage());
                return ExitCode::SUCCESS;
            }
            "--self-test" => run_self_test = true,
            "--base-path" | "--expected-marker" => {
                let Some(v) = inline.or_else(|| args.next()) else {
                    eprintln!("{}\nverify-pages-site: error: argument {flag}: expected one argument", usage());
                    return ExitCode::from(2);
                };
                if flag == "--base-path" {
                    base_path = v;
                } else {
                    markers.push(v);
                }
            }
            _ if flag.starts_with("--") => {
                eprintln!("{}\nverify-pages-site: error: unrecognized arguments: {a}", usage());
                return ExitCode::from(2);
            }
            _ if site.is_none() => site = Some(a),
            _ => {
                eprintln!("{}\nverify-pages-site: error: unrecognized arguments: {a}", usage());
                return ExitCode::from(2);
            }
        }
    }

    if run_self_test {
        return match self_test() {
            Ok(()) => {
                println!("pages verifier self-test passed (valid fixture + broken-link/accessibility controls)");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("self-test failed: {e}");
                ExitCode::from(1)
            }
        };
    }
    let Some(site) = site else {
        eprintln!("{}\nverify-pages-site: error: site is required unless --self-test is used", usage());
        return ExitCode::from(2);
    };

    let errors = verify_site(Path::new(&site), &base_path, &markers);
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("ERROR: {e}");
        }
        eprintln!("Pages artifact verification failed with {} error(s)", errors.len());
        return ExitCode::from(1);
    }
    println!("Pages artifact verified: {site} (base path {})", normalize_base_path(&base_path));
    ExitCode::SUCCESS
}
