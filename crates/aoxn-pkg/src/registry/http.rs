//! HTTP registry backend: the same `packages/<name>/…` tree as the git and
//! dir backends, served over plain HTTP/1.1.
//!
//! Read-only by design: `install`/`resolve`/`search` work, `publish`/`yank`
//! do not (those still go through the git or dir backend that owns the
//! tree). The client is hand-rolled on `std::net::TcpStream` — the crate's
//! dependency set must stay free of build scripts, which rules out every
//! TLS-capable HTTP crate on the build machine. Consequences:
//! - only `http://` URLs are accepted; `https://` is a hard, early error
//!   (put a local reverse proxy in front of a remote registry instead, or
//!   use the git backend);
//! - requests are sent with `Connection: close`, so the response body is
//!   read to EOF and then (for `Transfer-Encoding: chunked`) de-chunked
//!   from the buffer;
//! - 3xx redirects are followed (absolute or root-relative `Location`).
//!
//! Wire layout (matches a static file server pointed at a dir registry):
//! ```text
//! GET {base}/packages/{name}/index.json      -> PackageIndex JSON
//! GET {base}/packages/{name}/{version}.tar.gz
//! GET {base}/packages/names.json             -> JSON array of all names
//! GET {base}/trust.json                      -> trust index (optional; 404 = none)
//! ```
//! `names.json` replaces the directory listing the git/dir backends use
//! for the typosquat guard; a registry mirror must generate it. `trust.json`
//! is optional — see `docs/trusted-registry.md`.
//!
//! Tarball downloads are verified against the index's transport digest
//! (`tarball_sha256`) before they enter the cache, so a truncated or tampered
//! transfer fails here rather than at extraction. That digest is optional
//! (added in v0.31.0); when the registry does not publish it the wire check
//! is skipped and only the post-extraction manifest-hash check applies.

use std::io::{Read, Write};
use std::time::Duration;

use super::{PackageIndex, PublishOutcome, PublishRequest, Registry};
use crate::cache::{sha256_hex, Cache};
use crate::errors::PkgError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REDIRECTS: usize = 3;

pub struct HttpRegistry {
    /// normalized base URL: `http://host[:port][/path]`, no trailing slash
    base: String,
}

impl HttpRegistry {
    pub fn new(url: &str) -> HttpRegistry {
        HttpRegistry {
            base: url.trim_end_matches('/').to_string(),
        }
    }

    fn get(&self, path: &str) -> Result<Response, PkgError> {
        let mut target = format!("{}{path}", self.base);
        for _ in 0..=MAX_REDIRECTS {
            let resp = http_get(&target)?;
            if matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
                let loc = resp
                    .header("Location")
                    .ok_or_else(|| PkgError::Registry(format!("{target}: redirect without Location")))?;
                target = resolve_location(&target, &loc);
                continue;
            }
            return Ok(resp);
        }
        Err(PkgError::Registry(format!(
            "{target}: more than {MAX_REDIRECTS} redirects"
        )))
    }

    fn index_url(&self, name: &str) -> String {
        format!("{}/packages/{name}/index.json", self.base)
    }
}

impl Registry for HttpRegistry {
    fn url(&self) -> &str {
        &self.base
    }

    fn index(&mut self, name: &str) -> Result<PackageIndex, PkgError> {
        let resp = self.get(&format!("/packages/{name}/index.json"))?;
        match resp.status {
            200 => Ok(serde_json::from_slice(&resp.body)?),
            404 => Err(PkgError::PackageNotFound(name.to_string())),
            s => Err(PkgError::Registry(format!(
                "{}: unexpected HTTP status {s}",
                self.index_url(name)
            ))),
        }
    }

    fn all_names(&mut self) -> Result<Vec<String>, PkgError> {
        let url = format!("{}/packages/names.json", self.base);
        let resp = http_get(&url)?;
        match resp.status {
            200 => Ok(serde_json::from_slice(&resp.body)?),
            404 => Err(PkgError::Registry(format!(
                "registry `{}` has no packages/names.json (a static HTTP \
                 registry must ship the full name list for the typo-squat guard)",
                self.base
            ))),
            s => Err(PkgError::Registry(format!("{url}: unexpected HTTP status {s}"))),
        }
    }

    fn fetch_tarball(
        &mut self,
        cache: &Cache,
        name: &str,
        version: &str,
        checksum: &str,
        tarball_sha256: Option<&str>,
        offline: bool,
    ) -> Result<std::path::PathBuf, PkgError> {
        let dst = cache.tar_path(checksum)?;
        if dst.exists() {
            return Ok(dst);
        }
        if offline {
            return Err(PkgError::Offline(format!(
                "{name}@{version} is not in the cache and `--offline` is set \
                 (HTTP registries keep no local snapshot)"
            )));
        }
        let resp = self.get(&format!("/packages/{name}/{version}.tar.gz"))?;
        match resp.status {
            200 => {}
            404 => {
                return Err(PkgError::Registry(format!(
                    "registry `{}` lacks {name}@{version} tarball",
                    self.base
                )))
            }
            s => {
                return Err(PkgError::Registry(format!(
                    "{}/packages/{name}/{version}.tar.gz: unexpected HTTP status {s}",
                    self.base
                )))
            }
        }
        // Wire-level verification, before the bytes reach the cache. This
        // compares against the *transport* digest published in the index —
        // never against `checksum`, which is a manifest hash over the unpacked
        // tree and a different quantity entirely (comparing the two made every
        // HTTP download fail; fixed in v0.31.0). A registry predating
        // v0.31.0 publishes no transport digest: skip the wire check rather
        // than reject a good download. Integrity is not weakened either way —
        // install always re-verifies the extracted tree against the manifest
        // hash, which is the real supply-chain anchor.
        if let Some(expected) = tarball_sha256 {
            if sha256_hex(&resp.body) != expected {
                return Err(PkgError::Integrity {
                    name: name.to_string(),
                    version: version.to_string(),
                });
            }
        }
        cache.create_dirs()?;
        let tmp = cache.tarballs().join(format!(
            "{}.tmp-{}-{:?}",
            &checksum[..12.min(checksum.len())],
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&tmp, &resp.body)?;
        std::fs::rename(&tmp, &dst)?;
        Ok(dst)
    }

    fn fork(&self) -> Result<Box<dyn Registry>, PkgError> {
        Ok(Box::new(HttpRegistry::new(&self.base)))
    }

    fn trust(&mut self) -> Result<Option<crate::trust::TrustIndex>, PkgError> {
        let resp = self.get("/trust.json")?;
        match resp.status {
            200 => Ok(Some(crate::trust::TrustIndex::from_slice(&resp.body)?)),
            // a registry that ships no trust index simply makes no claims
            404 | 403 => Ok(None),
            s => Err(PkgError::Registry(format!(
                "{}/trust.json: unexpected HTTP status {s}",
                self.base
            ))),
        }
    }

    fn publish(&mut self, req: &PublishRequest, _dry_run: bool) -> Result<PublishOutcome, PkgError> {
        Err(PkgError::Registry(format!(
            "HTTP registry `{}` is read-only; publish {}@{} through the git or \
             dir registry that owns this tree",
            self.base, req.name, req.version
        )))
    }

    fn set_yank(&mut self, _name: &str, _version: &str, _yanked: bool) -> Result<(), PkgError> {
        Err(PkgError::Registry(format!(
            "HTTP registry `{}` is read-only; yank through the git or dir \
             registry that owns this tree",
            self.base
        )))
    }
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    }
}

/// Parse `http://host[:port]/path` — anything else is a hard error. `https`
/// gets its own message because it is a TLS problem, not a typo.
fn parse_url(url: &str) -> Result<(String, u16, String), PkgError> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Err(PkgError::Registry(format!(
            "`{url}`: https registry URLs are not supported (the package \
             manager's HTTP client is deliberately TLS-free to keep the \
             dependency set build-script-free); serve the registry over \
             plain http:// on a trusted network, or use the git backend"
        )));
    }
    // the scheme check is case-insensitive but host and path keep their
    // original case (paths are case-sensitive on most servers)
    let rest = if lower.starts_with("http://") {
        &url[7..]
    } else {
        return Err(PkgError::Registry(format!("`{url}`: not an http:// URL")));
    };
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    if hostport.is_empty() {
        return Err(PkgError::Registry(format!("`{url}`: no host")));
    }
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|_| PkgError::Registry(format!("`{url}`: bad port `{p}`")))?,
        ),
        None => (hostport.to_string(), 80),
    };
    Ok((host, port, path.trim_end_matches('/').to_string()))
}

/// Resolve a `Location` against the URL it came from (absolute URLs and
/// root-relative paths only — enough for registry mirrors).
fn resolve_location(from: &str, loc: &str) -> String {
    if loc.contains("://") {
        loc.to_string()
    } else if let Some(rest) = loc.strip_prefix('/') {
        // scheme + host of `from`, then the absolute path
        let scheme_end = from.find("://").map(|i| i + 3).unwrap_or(0);
        let host_end = from[scheme_end..]
            .find('/')
            .map(|i| scheme_end + i)
            .unwrap_or(from.len());
        format!("{}{}{rest}", &from[..host_end], "/")
    } else {
        // relative path: replace the last path segment
        match from.rfind('/') {
            Some(i) => format!("{}{loc}", &from[..=i]),
            None => format!("/{loc}"),
        }
    }
}

fn http_get(url: &str) -> Result<Response, PkgError> {
    let (host, port, path) = parse_url(url)?;
    let addr = std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port))
        .map_err(|e| PkgError::Registry(format!("cannot resolve `{host}`: {}", crate::errors::io_reason(&e))))?
        .next()
        .ok_or_else(|| PkgError::Registry(format!("cannot resolve `{host}`")))?;
    let mut stream = std::net::TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        .map_err(|e| PkgError::Registry(format!("cannot connect to `{host}:{port}`: {}", crate::errors::io_reason(&e))))?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUser-Agent: aoxn-pkg\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes())?;

    // Connection: close means EOF ends the body; de-chunk afterwards if the
    // response arrived chunked
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;

    let header_end = find_subsequence(&raw, b"\r\n\r\n")
        .ok_or_else(|| PkgError::Registry(format!("{url}: malformed HTTP response (no header terminator)")))?;
    let head = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| PkgError::Registry(format!("{url}: malformed status line `{status_line}`")))?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();

    let mut body = raw[header_end + 4..].to_vec();
    let chunked = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("Transfer-Encoding") && v.to_ascii_lowercase().contains("chunked"));
    if chunked {
        body = dechunk(&body)
            .ok_or_else(|| PkgError::Registry(format!("{url}: malformed chunked body")))?;
    } else if let Some(len) = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("Content-Length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
    {
        body.truncate(len);
    }

    Ok(Response { status, headers, body })
}

/// Decode a chunked body that was read to EOF (terminal chunk and trailers
/// included). Returns None on malformed input.
fn dechunk(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut rest = data;
    loop {
        let line_end = rest.iter().position(|&b| b == b'\n')?;
        let size_str = std::str::from_utf8(&rest[..line_end]).ok()?;
        let size = usize::from_str_radix(size_str.trim().split(';').next()?.trim(), 16).ok()?;
        rest = &rest[line_end + 1..];
        if size == 0 {
            return Some(out);
        }
        if rest.len() < size + 2 {
            return None;
        }
        out.extend_from_slice(&rest[..size]);
        // the chunk is followed by CRLF
        if &rest[size..size + 2] != b"\r\n" {
            return None;
        }
        rest = &rest[size + 2..];
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    struct Server {
        url: String,
        root: PathBuf,
    }

    impl Drop for Server {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// A sequential static-file HTTP server over a temp registry tree. Each
    /// connection serves one GET (the client sends `Connection: close`).
    fn spawn_server() -> Server {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("aoxn-http-test-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let root_for_thread = root.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                serve_one(stream, &root_for_thread);
            }
        });
        Server {
            url: format!("http://127.0.0.1:{port}"),
            root,
        }
    }

    fn serve_one(mut stream: std::net::TcpStream, root: &Path) {
        let mut buf = [0u8; 4096];
        let mut head = Vec::new();
        // request head ends at the first CRLFCRLF
        loop {
            let n = match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            head.extend_from_slice(&buf[..n]);
            if find_subsequence(&head, b"\r\n\r\n").is_some() {
                break;
            }
        }
        let req_line = String::from_utf8_lossy(&head);
        let path = req_line.split_whitespace().nth(1).unwrap_or("/");
        // path traversal guard: only serve inside the root
        if path.contains("..") {
            respond(&mut stream, 403, b"forbidden", false);
            return;
        }
        let rel = path.trim_start_matches('/');
        let file = root.join(rel);
        match std::fs::read(&file) {
            Ok(bytes) => respond(&mut stream, 200, &bytes, false),
            Err(_) => respond(&mut stream, 404, b"not found", false),
        }
    }

    fn respond(stream: &mut std::net::TcpStream, status: u16, body: &[u8], chunked: bool) {
        let head = if chunked {
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_string()
        } else {
            format!("HTTP/1.1 {status} x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
        };
        stream.write_all(head.as_bytes()).unwrap();
        if chunked {
            for chunk in body.chunks(3) {
                stream
                    .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                    .unwrap();
                stream.write_all(chunk).unwrap();
                stream.write_all(b"\r\n").unwrap();
            }
            stream.write_all(b"0\r\n\r\n").unwrap();
        } else {
            stream.write_all(body).unwrap();
        }
    }

    /// Populate a registry tree with one package (`demo@1.0.0`) and return
    /// the manifest hash it publishes under.
    ///
    /// The index carries BOTH digests, the way a real v0.31.0 publish does:
    /// `checksum` is the manifest hash over the unpacked tree (the content
    /// anchor) and `tarball_sha256` is the digest of the tarball bytes (the
    /// transport digest the download check compares against).
    fn add_package(server: &Server) -> String {
        let (manifest_hash, _) = add_package_full(server);
        manifest_hash
    }

    fn add_package_full(server: &Server) -> (String, String) {
        use crate::tarball;
        let pkg = server.root.join("src-pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("aoxn.json"),
            r#"{ "name": "demo", "version": "1.0.0", "main": "lib.ax" }"#,
        )
        .unwrap();
        std::fs::write(pkg.join("lib.ax"), "pub fn hi() -> int { 42 }").unwrap();
        let manifest_hash = tarball::manifest_hash(&pkg).unwrap();
        let pkg_dir = server.root.join("packages").join("demo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        tarball::pack(&pkg, "demo", &pkg_dir.join("1.0.0.tar.gz")).unwrap();
        let tarball_sha256 = sha256_hex(&std::fs::read(pkg_dir.join("1.0.0.tar.gz")).unwrap());
        let index = format!(
            r#"{{ "name": "demo", "versions": {{ "1.0.0": {{ "checksum": "{manifest_hash}", "tarball_sha256": "{tarball_sha256}" }} }} }}"#
        );
        std::fs::write(pkg_dir.join("index.json"), &index).unwrap();
        std::fs::write(
            server.root.join("packages").join("names.json"),
            r#"["demo"]"#,
        )
        .unwrap();
        (manifest_hash, tarball_sha256)
    }

    fn test_cache(server: &Server) -> Cache {
        Cache {
            root: server.root.join("cache"),
        }
    }

    #[test]
    fn index_tarball_and_names_roundtrip() {
        let server = spawn_server();
        let (checksum, tarball_sha256) = add_package_full(&server);
        let mut reg = HttpRegistry::new(&server.url);

        let idx = reg.index("demo").unwrap();
        assert_eq!(idx.name, "demo");
        assert_eq!(idx.get("1.0.0").unwrap().checksum, checksum);
        assert_eq!(
            idx.get("1.0.0").unwrap().tarball_sha256.as_deref(),
            Some(tarball_sha256.as_str())
        );

        let names = reg.all_names().unwrap();
        assert_eq!(names, vec!["demo".to_string()]);

        let cache = test_cache(&server);
        let tar = reg
            .fetch_tarball(&cache, "demo", "1.0.0", &checksum, Some(&tarball_sha256), false)
            .unwrap();
        assert_eq!(sha256_hex(&std::fs::read(&tar).unwrap()), tarball_sha256);
        // second fetch is served from the cache
        let tar2 = reg
            .fetch_tarball(&cache, "demo", "1.0.0", &checksum, Some(&tarball_sha256), false)
            .unwrap();
        assert_eq!(tar, tar2);
    }

    /// The regression this whole fix exists for: the manifest hash and the
    /// tarball digest are different quantities, and comparing the download
    /// against the manifest hash made every real HTTP install fail.
    #[test]
    fn the_manifest_hash_is_not_the_tarball_digest() {
        let server = spawn_server();
        let (checksum, tarball_sha256) = add_package_full(&server);
        assert_ne!(
            checksum, tarball_sha256,
            "the two digests must differ, otherwise this bug would be invisible"
        );
    }

    #[test]
    fn a_registry_without_a_transport_digest_still_downloads() {
        // An index published before v0.31.0 carries no `tarball_sha256`. The
        // wire check must be skipped rather than fail a good download;
        // integrity still holds because install re-verifies the extracted
        // tree against the manifest hash.
        let server = spawn_server();
        let (checksum, _) = add_package_full(&server);
        std::fs::write(
            server.root.join("packages").join("demo").join("index.json"),
            format!(r#"{{ "name": "demo", "versions": {{ "1.0.0": {{ "checksum": "{checksum}" }} }} }}"#),
        )
        .unwrap();

        let mut reg = HttpRegistry::new(&server.url);
        let idx = reg.index("demo").unwrap();
        assert!(idx.get("1.0.0").unwrap().tarball_sha256.is_none());
        let tar = reg
            .fetch_tarball(&test_cache(&server), "demo", "1.0.0", &checksum, None, false)
            .unwrap();
        assert!(tar.exists(), "a good download must not be rejected");
    }

    #[test]
    fn unknown_package_is_package_not_found() {
        let server = spawn_server();
        add_package(&server);
        let mut reg = HttpRegistry::new(&server.url);
        match reg.index("nope") {
            Err(PkgError::PackageNotFound(n)) => assert_eq!(n, "nope"),
            other => panic!("expected PackageNotFound, got {other:?}"),
        }
    }

    #[test]
    fn tampered_tarball_fails_integrity_check() {
        let server = spawn_server();
        let (_checksum, tarball_sha256) = add_package_full(&server);
        // corrupt the served tarball after the index was written
        let served = server.root.join("packages").join("demo").join("1.0.0.tar.gz");
        std::fs::write(&served, b"not a tarball").unwrap();
        let mut reg = HttpRegistry::new(&server.url);
        match reg.fetch_tarball(
            &test_cache(&server),
            "demo",
            "1.0.0",
            // a real manifest checksum: sha256 hex. The cache path is built from
            // this string, so a malformed one is refused BEFORE it reaches
            // the filesystem (that is what `../escaped` used to exploit).
            &"d".repeat(64),
            Some(&tarball_sha256),
            false,
        ) {
            Err(PkgError::Integrity { name, version }) => {
                assert_eq!((name.as_str(), version.as_str()), ("demo", "1.0.0"))
            }
            other => panic!("expected Integrity, got {other:?}"),
        }
    }

    #[test]
    fn offline_mode_refuses_uncached_tarball() {
        let server = spawn_server();
        let (checksum, tarball_sha256) = add_package_full(&server);
        let mut reg = HttpRegistry::new(&server.url);
        match reg.fetch_tarball(
            &test_cache(&server),
            "demo",
            "1.0.0",
            &checksum,
            Some(&tarball_sha256),
            true,
        ) {
            Err(PkgError::Offline(_)) => {}
            other => panic!("expected Offline, got {other:?}"),
        }
    }

    #[test]
    fn a_registry_with_no_trust_json_reports_none() {
        let server = spawn_server();
        add_package(&server);
        let mut reg = HttpRegistry::new(&server.url);
        assert!(reg.trust().unwrap().is_none());
    }

    #[test]
    fn a_registry_trust_index_is_read_over_http() {
        let server = spawn_server();
        add_package(&server);
        std::fs::write(
            server.root.join("trust.json"),
            br#"{"schema":1,"updated":"2026-10-02","packages":{"demo":{"tier":"audited","reviewer":"@ryan"}}}"#,
        )
        .unwrap();
        let mut reg = HttpRegistry::new(&server.url);
        let trust = reg.trust().unwrap().expect("trust.json should be found");
        assert_eq!(trust.tier_of("demo"), crate::trust::Tier::Audited);
        assert_eq!(trust.entry("demo").unwrap().reviewer.as_deref(), Some("@ryan"));
    }

    #[test]
    fn publish_and_yank_are_read_only() {
        let server = spawn_server();
        let mut reg = HttpRegistry::new(&server.url);
        let req = PublishRequest {
            name: "demo".into(),
            version: "1.0.0".into(),
            tarball: PathBuf::from("x"),
            checksum: "0".repeat(64),
            tarball_sha256: Some("1".repeat(64)),
            dependencies: Default::default(),
            aoxn: None,
        };
        assert!(reg.publish(&req, false).is_err());
        assert!(reg.set_yank("demo", "1.0.0", true).is_err());
    }

    #[test]
    fn https_url_is_rejected_with_a_tls_message() {
        let mut reg = HttpRegistry::new("https://registry.example.com");
        match reg.index("demo") {
            Err(e) => assert!(format!("{e}").contains("https"), "message: {e}"),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn chunked_responses_are_decoded() {
        let root = std::env::temp_dir().join(format!("aoxn-http-chunk-{}", std::process::id()));
        std::fs::create_dir_all(root.join("packages/demo")).unwrap();
        let body = br#"{ "name": "demo", "versions": {} }"#;
        std::fs::write(root.join("packages/demo/index.json"), body).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let root_for_thread = root.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut s = stream;
                // serve every request head, then the index.json chunked
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                respond(&mut s, 200, body, true);
            }
            let _ = root_for_thread;
        });
        let mut reg = HttpRegistry::new(&format!("http://127.0.0.1:{port}"));
        let idx = reg.index("demo").unwrap();
        assert_eq!(idx.name, "demo");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn url_parsing_rejects_garbage() {
        assert!(parse_url("ftp://x/y").is_err());
        assert!(parse_url("http:///no-host").is_err());
        assert!(parse_url("http://h:notaport/").is_err());
        let (host, port, path) = parse_url("http://example.com:8080/registry/").unwrap();
        assert_eq!((host.as_str(), port, path.as_str()), ("example.com", 8080, "/registry"));
    }
}
