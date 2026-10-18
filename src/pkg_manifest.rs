//! Zero-dependency reader for `aoxn.json` package manifests.
//!
//! The compiler library (`src/`) has a strict zero-external-crate policy
//! (see SECURITY.md), so this module hand-rolls a minimal JSON value parser
//! instead of pulling in serde_json. It extracts only the entry-resolution
//! fields a bare `import "pkg"` needs: `main`, `exports`, and `types`. The
//! full manifest (dependencies, workspace, registries, ...) is owned by the
//! `aoxn-pkg` crate, which uses serde; the compiler never needs that surface.
//!
//! Resolution order for a bare package import with subpath `s` (None or "."
//! means the package root):
//! 1. `exports[s]` / `exports["."]` if `exports` is present
//! 2. `main` when `s` is None/"."
//! 3. fall through to the legacy `aox_modules/<name>` directory probe in
//!    `resolve_import` (this function returns `None` then).
//!
//! `exports` values may be a plain path string or a conditional object
//! (`{ "default": "...", "types": "..." }`); the `"default"` entry wins,
//! then `"types"`, mirroring the npm conditional-exports convention closely
//! enough for Aoxn's needs.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// One parsed JSON value. Only the shapes manifest entry resolution needs
/// are modeled richly (Object, String); the rest are accepted but opaque.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Num/Array payloads are accepted by the parser, not read out.
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    fn as_str(&self) -> Option<&str> {
        if let Json::Str(s) = self { Some(s) } else { None }
    }
    fn as_object(&self) -> Option<&BTreeMap<String, Json>> {
        if let Json::Object(m) = self { Some(m) } else { None }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Parser { b: s.as_bytes(), i: 0 }
    }

    fn ws(&mut self) {
        while self.i < self.b.len() {
            match self.b[self.i] {
                b' ' | b'\t' | b'\n' | b'\r' => self.i += 1,
                _ => break,
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn parse_value(&mut self) -> Option<Json> {
        self.ws();
        match self.peek()? {
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b'"' => self.parse_string().map(Json::Str),
            b't' | b'f' => self.parse_bool(),
            b'n' => self.parse_null(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => None,
        }
    }

    fn parse_object(&mut self) -> Option<Json> {
        // assumes peek() == '{'
        self.i += 1;
        let mut map = BTreeMap::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Some(Json::Object(map));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return None;
            }
            let key = self.parse_string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return None;
            }
            self.i += 1;
            let val = self.parse_value()?;
            map.insert(key, val);
            self.ws();
            match self.peek()? {
                b',' => {
                    self.i += 1;
                    continue;
                }
                b'}' => {
                    self.i += 1;
                    return Some(Json::Object(map));
                }
                _ => return None,
            }
        }
    }

    fn parse_array(&mut self) -> Option<Json> {
        // assumes peek() == '['
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Some(Json::Array(items));
        }
        loop {
            let v = self.parse_value()?;
            items.push(v);
            self.ws();
            match self.peek()? {
                b',' => {
                    self.i += 1;
                    continue;
                }
                b']' => {
                    self.i += 1;
                    return Some(Json::Array(items));
                }
                _ => return None,
            }
        }
    }

    fn parse_string(&mut self) -> Option<String> {
        // assumes peek() == '"'. Collect raw bytes (the input is a valid Rust
        // &str, so any multi-byte UTF-8 sequence is intact byte-for-byte) and
        // decode once at the end; escapes push their UTF-8 encoding directly.
        self.i += 1;
        let mut buf: Vec<u8> = Vec::new();
        while self.i < self.b.len() {
            let c = self.b[self.i];
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(buf).ok(),
                b'\\' => {
                    let e = *self.b.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'"' => buf.push(b'"'),
                        b'\\' => buf.push(b'\\'),
                        b'/' => buf.push(b'/'),
                        b'n' => buf.push(b'\n'),
                        b't' => buf.push(b'\t'),
                        b'r' => buf.push(b'\r'),
                        b'b' => buf.push(0x08),
                        b'f' => buf.push(0x0C),
                        b'u' => {
                            let h = std::str::from_utf8(self.b.get(self.i..self.i + 4)?).ok()?;
                            let cp = u32::from_str_radix(h, 16).ok()?;
                            self.i += 4;
                            if let Some(ch) = char::from_u32(cp) {
                                let mut enc = [0u8; 4];
                                buf.extend_from_slice(ch.encode_utf8(&mut enc).as_bytes());
                            }
                        }
                        _ => return None,
                    }
                }
                _ => buf.push(c),
            }
        }
        None
    }

    fn parse_bool(&mut self) -> Option<Json> {
        if self.b[self.i..].starts_with(b"true") {
            self.i += 4;
            Some(Json::Bool(true))
        } else if self.b[self.i..].starts_with(b"false") {
            self.i += 5;
            Some(Json::Bool(false))
        } else {
            None
        }
    }

    fn parse_null(&mut self) -> Option<Json> {
        if self.b[self.i..].starts_with(b"null") {
            self.i += 4;
            Some(Json::Null)
        } else {
            None
        }
    }

    fn parse_number(&mut self) -> Option<Json> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == b'.' || c == b'e' || c == b'E' || c == b'+' || c == b'-' {
                self.i += 1;
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(self.b.get(start..self.i)?).ok()?;
        s.parse::<f64>().ok().map(Json::Num)
    }
}

/// Parse a JSON document. Returns `None` on any malformed input; manifest
/// corruption surfaces to the user as a fall-back to the directory probe
/// rather than a hard error, so we keep this lenient.
fn parse(s: &str) -> Option<Json> {
    // Strip a UTF-8 BOM if present (PowerShell writes one; serde_json rejects
    // it, and we want to be friendlier than serde_json here).
    let s = s.strip_prefix('\u{FEFF}').unwrap_or(s);
    let mut p = Parser::new(s);
    let v = p.parse_value()?;
    p.ws();
    // trailing garbage → reject
    if p.i != p.b.len() {
        return None;
    }
    Some(v)
}

/// Resolve a path-valued `exports` entry that may be a plain string or a
/// conditional object (`{ "default": "...", "types": "..." }`).
fn exports_target(val: &Json) -> Option<&str> {
    if let Some(s) = val.as_str() {
        return Some(s);
    }
    if let Some(obj) = val.as_object() {
        if let Some(Json::Str(s)) = obj.get("default") {
            return Some(s);
        }
        if let Some(Json::Str(s)) = obj.get("types") {
            return Some(s);
        }
    }
    None
}

// The loader asks this question once per IMPORT, and every module of a package
// asks it about the same `aoxn.json`. Without the memo below, the manifest was
// read from disk and JSON-parsed once per import edge -- for a package with N
// modules that is N reads and N parses of one file. The cache is a
// `thread_local` because resolution is single-threaded and this is a
// build-time answer: the tree cannot change under a build already walking it.
thread_local! {
    static ENTRY_CACHE: std::cell::RefCell<HashMap<(PathBuf, Option<String>), Option<PathBuf>>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Resolve the entry source file for a package rooted at `pkg_dir`, given a
/// subpath (None or `"."` = the package root; `"./sub"` or `"sub"` = that
/// subpath). Returns the resolved file path only when it exists on disk.
///
/// Memoized per process; see the note on `ENTRY_CACHE`.
pub fn resolve_pkg_entry(pkg_dir: &Path, subpath: Option<&str>) -> Option<PathBuf> {
    let key = (pkg_dir.to_path_buf(), subpath.map(|s| s.to_string()));
    ENTRY_CACHE.with(|c| {
        if let Some(hit) = c.borrow().get(&key) {
            return hit.clone();
        }
        let fresh = resolve_pkg_entry_uncached(pkg_dir, subpath);
        c.borrow_mut().insert(key, fresh.clone());
        fresh
    })
}

fn resolve_pkg_entry_uncached(pkg_dir: &Path, subpath: Option<&str>) -> Option<PathBuf> {

    let manifest_path = pkg_dir.join("aoxn.json");
    let text = std::fs::read_to_string(&manifest_path).ok()?;
    let root = parse(&text)?.as_object()?.clone();

    let sub = match subpath {
        None | Some(".") | Some("") => ".",
        Some(s) => {
            // normalize "sub" and "./sub" to "./sub"
            if s.starts_with("./") {
                s
            } else {
                // build "./sub" from a bare "sub"
                // (cheap; the exports keys we publish use the "./" form)
                return exports_lookup(pkg_dir, &root, &format!("./{s}"));
            }
        }
    };
    exports_lookup(pkg_dir, &root, sub)
}

fn exports_lookup(pkg_dir: &Path, root: &BTreeMap<String, Json>, sub: &str) -> Option<PathBuf> {
    // 1. conditional/plain exports map
    if let Some(Json::Object(exports)) = root.get("exports") {
        if let Some(val) = exports.get(sub) {
            if let Some(rel) = exports_target(val) {
                let cand = pkg_dir.join(rel);
                if cand.exists() {
                    return Some(cand);
                }
            }
            // exports present but this subpath missing or file absent → stop.
            // Falling back to `main` would contradict the npm semantics
            // (exports, when present, gates all entries), so return None.
            return None;
        }
        // exports present but no matching key → no entry (subpath not exported).
        return None;
    }
    // 2. `main` for the root subpath only.
    if sub == "." {
        if let Some(Json::Str(m)) = root.get("main") {
            let cand = pkg_dir.join(m);
            if cand.exists() {
                return Some(cand);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_pkg(dir: &Path, manifest: &str, files: &[(&str, &str)]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let mut f = std::fs::File::create(dir.join("aoxn.json")).unwrap();
        f.write_all(manifest.as_bytes()).unwrap();
        for (rel, body) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            let mut g = std::fs::File::create(&p).unwrap();
            g.write_all(body.as_bytes()).unwrap();
        }
        dir.to_path_buf()
    }

    #[test]
    fn parses_main_entry() {
        let tmp = std::env::temp_dir().join("aoxn_pm_main");
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg = write_pkg(
            &tmp,
            r#"{"name":"p","version":"0.1.0","main":"src/lib.ax"}"#,
            &[("src/lib.ax", "def answer(): int { return 42 }")],
        );
        let got = resolve_pkg_entry(&pkg, None).unwrap();
        assert!(got.ends_with("src/lib.ax"));
    }

    #[test]
    fn exports_subpath_string_target() {
        let tmp = std::env::temp_dir().join("aoxn_pm_exp");
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg = write_pkg(
            &tmp,
            r#"{"name":"p","exports":{"./sub":"./src/sub.ax"}}"#,
            &[("src/sub.ax", "def x(): int { return 1 }")],
        );
        let got = resolve_pkg_entry(&pkg, Some("./sub")).unwrap();
        assert!(got.ends_with("src/sub.ax"));
    }

    #[test]
    fn exports_conditional_object_uses_default() {
        let tmp = std::env::temp_dir().join("aoxn_pm_cond");
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg = write_pkg(
            &tmp,
            r#"{"name":"p","exports":{".":{"types":"./types.ax","default":"./src/lib.ax"}}}"#,
            &[("src/lib.ax", "def y(): int { return 9 }")],
        );
        let got = resolve_pkg_entry(&pkg, None).unwrap();
        assert!(got.ends_with("src/lib.ax"));
    }

    #[test]
    fn exports_present_but_missing_subpath_returns_none() {
        let tmp = std::env::temp_dir().join("aoxn_pm_miss");
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg = write_pkg(
            &tmp,
            r#"{"name":"p","exports":{".":"./src/lib.ax"}}"#,
            &[("src/lib.ax", "def z(): int { return 0 }")],
        );
        // subpath not in exports → None (no fallback to main)
        assert!(resolve_pkg_entry(&pkg, Some("./nope")).is_none());
    }

    #[test]
    fn no_manifest_returns_none() {
        let tmp = std::env::temp_dir().join("aoxn_pm_none");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(resolve_pkg_entry(&tmp, None).is_none());
    }

    #[test]
    fn bom_tolerated() {
        let tmp = std::env::temp_dir().join("aoxn_pm_bom");
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg = write_pkg(
            &tmp,
            "\u{FEFF}{\"name\":\"p\",\"main\":\"src/lib.ax\"}",
            &[("src/lib.ax", "def q(): int { return 7 }")],
        );
        let got = resolve_pkg_entry(&pkg, None).unwrap();
        assert!(got.ends_with("src/lib.ax"));
    }
}
