//! Package tarball (transport format) and the manifest hash (integrity).
//!
//! **Integrity does NOT live in tarball bytes.** The checksum stored in the
//! registry index and pinned in `aoxn.lock` is a *manifest hash* over a
//! normalized file listing:
//!
//! ```text
//! checksum = sha256( concat( "<posix_rel_path>\0<sha256(content)>\n" ) for f in sorted(files) )
//! ```
//!
//! This is immune to tar/gzip implementation quirks, platform differences
//! and re-packing; `install` verifies it by recomputing over the extracted
//! tree, which is exactly the supply-chain property we want (the bytes you
//! run are what the checksum describes). The tarball is merely transport —
//! but we still normalize it best-effort (sorted walk, mtime=0, uid/gid=0,
//! fixed modes, gzip mtime=0) so re-packs of the same tree are byte-stable.
//!
//! Excluded from packages: `target/`, `aox_modules/`, `vendor/` (npm
//! imports), `.git/`, `aoxn.lock`, `*.tar.gz` (so packing never includes
//! its own output).

use std::fs::File;
use std::path::{Component, Path, PathBuf};

use crate::cache::sha256_hex;
use crate::errors::PkgError;

/// Directories/files never included in a published tarball.
fn excluded(relative: &Path) -> bool {
    for comp in relative.components() {
        let Some(s) = comp.as_os_str().to_str() else {
            return true; // non-UTF-8 path component: skip
        };
        if matches!(
            s,
            "target" | "aox_modules" | "vendor" | ".git" | ".aoxn" | ".hg" | ".svn"
        ) && comp == Component::Normal(std::ffi::OsStr::new(s))
        {
            return true;
        }
    }
    let file_name = relative.file_name().map(|f| f.to_string_lossy().to_string());
    match file_name.as_deref() {
        Some("aoxn.lock") => true,
        Some(f) => f.ends_with(".tar.gz"),
        None => false,
    }
}

/// POSIX-style relative path (forward slashes), for deterministic keys.
fn rel_posix(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

/// Sorted list of (posix rel path, absolute path) for every packed file.
pub fn list_files(pkg_dir: &Path) -> Result<Vec<(String, PathBuf)>, PkgError> {
    let mut out = Vec::new();
    fn walk(dir: &Path, rel: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), PkgError> {
        let full = dir.join(rel);
        if full.is_dir() {
            if !rel.as_os_str().is_empty() && excluded(rel) {
                return Ok(());
            }
            let mut entries: Vec<_> = std::fs::read_dir(&full)?.collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let next = rel.join(e.file_name());
                walk(dir, &next, out)?;
            }
        } else if !excluded(rel) {
            out.push((rel_posix(rel), full));
        }
        Ok(())
    }
    walk(pkg_dir, Path::new(""), &mut out)?;
    Ok(out)
}

/// The manifest hash: sha256 over a normalized file listing (see module
/// docs). Computed at publish time, stored in the registry index, pinned in
/// the lockfile, re-verified over the extracted tree at install time.
pub fn manifest_hash(pkg_dir: &Path) -> Result<String, PkgError> {
    let files = list_files(pkg_dir)?;
    let mut acc = String::new();
    for (rel, abs) in &files {
        let content = std::fs::read(abs)?;
        acc.push_str(&format!("{rel}\0{}\n", sha256_hex(&content)));
    }
    Ok(sha256_hex(acc.as_bytes()))
}

/// Pack `pkg_dir` into `out_path` (tar.gz, entries rooted at `pkg_name`).
/// Headers are normalized (mtime/uid/gid zeroed, fixed modes, sorted walk,
/// gzip mtime=0) so identical trees re-pack to identical bytes. Returns the
/// number of files packed.
pub fn pack(pkg_dir: &Path, pkg_name: &str, out_path: &Path) -> Result<usize, PkgError> {
    let files = list_files(pkg_dir)?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let out = File::create(out_path)?;
    // gzip header without a timestamp (byte-stable re-packs)
    let gz = flate2::GzBuilder::new().mtime(0).write(out, flate2::Compression::default());
    let mut builder = tar::Builder::new(gz);

    // directory entries first (sorted), then files
    let mut dirs: Vec<String> = Vec::new();
    for (rel, _) in &files {
        let mut p = Path::new(rel);
        while let Some(parent) = p.parent() {
            if parent.as_os_str().is_empty() {
                break;
            }
            let ps = rel_posix(parent);
            if !dirs.contains(&ps) {
                dirs.push(ps.clone());
            }
            p = parent;
        }
    }
    dirs.sort();
    dirs.dedup();
    for d in &dirs {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o755);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_cksum();
        let arc = format!("{pkg_name}/{d}");
        builder.append_data(&mut header, arc, std::io::empty())?;
    }
    for (rel, abs) in &files {
        let content = std::fs::read(abs)?;
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_cksum();
        let arc = format!("{pkg_name}/{rel}");
        builder.append_data(&mut header, arc, content.as_slice())?;
    }
    builder.finish()?;
    Ok(files.len())
}

/// Reject any entry path that could write outside `dest`.
///
/// The old check was `stripped.contains("..")`, which is a substring test on
/// a *relative* path and therefore says nothing about an ABSOLUTE one. An
/// entry named `demo/C:/Users/victim/outside.txt` strips to
/// `C:/Users/victim/outside.txt`; `Path::join` with an absolute path DISCARDS
/// `dest` entirely, so the file landed wherever it pointed — and because
/// integrity is verified only after extraction, the failing check could not
/// undo the write. (`demo//etc/passwd` is the same escape on POSIX.)
///
/// Components are inspected rather than the string, so a legitimate
/// `a..b` is no longer rejected while `a/../b`, `/abs`, `C:\abs` and `` are.
fn check_contained(rel: &std::path::Path, entry_name: &str) -> Result<(), PkgError> {
    let bad = |why: &str| {
        Err(PkgError::Registry(format!(
            "malformed package tarball: entry `{entry_name}` {why}"
        )))
    };
    if rel.as_os_str().is_empty() {
        return bad("has an empty path");
    }
    if rel.is_absolute() {
        return bad("is an absolute path");
    }
    for comp in rel.components() {
        use std::path::Component;
        match comp {
            Component::Normal(_) => {}
            Component::CurDir => return bad("contains a `.` component"),
            Component::ParentDir => return bad("escapes the package directory"),
            Component::RootDir | Component::Prefix(_) => {
                return bad("is an absolute path")
            }
        }
    }
    Ok(())
}

/// A package archive carries SOURCE, so it carries no links.
///
/// `tar::Entry::unpack` — unlike `unpack_in` — honours the archive's own link
/// targets, and every link interpretation here is an escape:
///
/// - a hard link to an absolute path writes outside `dest` outright;
/// - the manifest hash covers CONTENT, not link identity, so a checksum
///   computed over a known external file's original bytes PASSES, and the
///   shim write that follows (`install::write_shim_if_needed`) then truncates
///   that external file;
/// - a symlink parent turns a later, ordinary-looking entry into an escape.
///
/// Refusing links outright is both simpler and stronger than resolving them:
/// `pack` never emits one, so a legitimate archive loses nothing, and there is
/// no relative-target interpretation left to get wrong.
fn reject_link(entry: &tar::Entry<impl std::io::Read>, entry_name: &str) -> Result<(), PkgError> {
    let kind = entry.header().entry_type();
    if kind.is_hard_link() || kind.is_symlink() {
        let target = entry
            .link_name()
            .ok()
            .flatten()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<none>".to_string());
        return Err(PkgError::Registry(format!(
            "malformed package tarball: entry `{entry_name}` is a {} pointing at `{target}`; a package may not contain links",
            if kind.is_symlink() { "symlink" } else { "hard link" }
        )));
    }
    Ok(())
}

/// Extract a tarball into `dest` (created if missing). All entries must be
/// rooted at `pkg_name/`, stay inside `dest` as plain relative components,
/// and any link must resolve inside `dest` too — anything else is rejected
/// as malformed (and treated as a potential supply-chain attack).
pub fn unpack(tarball: &Path, dest: &Path, pkg_name: &str) -> Result<(), PkgError> {
    std::fs::create_dir_all(dest)?;
    let file = File::open(tarball)?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    let root = format!("{pkg_name}/");
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        let entry_name = path.display().to_string();
        let Some(stripped) = path.to_str().and_then(|p| p.strip_prefix(&root)) else {
            return Err(PkgError::Registry(format!(
                "malformed package tarball: entry `{entry_name}` is not rooted at `{root}`",
            )));
        };
        let rel = std::path::Path::new(stripped);
        check_contained(rel, &entry_name)?;
        let target = dest.join(stripped.replace('/', std::path::MAIN_SEPARATOR_STR));
        // links are refused before anything is created, so a rejected archive
        // has written nothing at all
        reject_link(&entry, &entry_name)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry.unpack(&target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, content: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    fn temp_root(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aoxn-pkg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn pack_unpack_roundtrip_excludes_artifacts() {
        let tmp = temp_root("test");
        write(&tmp.join("src").join("main.ax"), "def main(): pass\n");
        write(&tmp.join("src").join("aoxn.json"), "{}");
        write(&tmp.join("target").join("cache").join("junk"), "no");
        write(&tmp.join("aox_modules").join("dep").join("junk.ax"), "no");

        let tgz = tmp.join("out.tar.gz");
        let n = pack(&tmp, "mypkg", &tgz).unwrap();
        assert!(n >= 2);

        let dest = tmp.join("unpacked");
        unpack(&tgz, &dest, "mypkg").unwrap();
        assert!(dest.join("src").join("main.ax").exists());
        assert!(dest.join("src").join("aoxn.json").exists());
        assert!(!dest.join("target").exists());
        assert!(!dest.join("aox_modules").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn manifest_hash_is_content_and_layout_sensitive() {
        let a = temp_root("hash-a");
        write(&a.join("lib.ax"), "def f(): pass\n");
        let h1 = manifest_hash(&a).unwrap();

        // same content, same name -> same hash
        let b = temp_root("hash-b");
        write(&b.join("lib.ax"), "def f(): pass\n");
        assert_eq!(h1, manifest_hash(&b).unwrap());

        // changed content -> different hash
        write(&a.join("lib.ax"), "def f(): pass # changed\n");
        assert_ne!(h1, manifest_hash(&a).unwrap());

        // same content, different layout -> different hash
        let c = temp_root("hash-c");
        write(&c.join("other.ax"), "def f(): pass\n");
        assert_ne!(h1, manifest_hash(&c).unwrap());

        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
        let _ = std::fs::remove_dir_all(&c);
    }

    #[test]
    fn extracted_tree_matches_manifest_hash() {
        // the install-time guarantee: hash(extracted) == hash(packed tree)
        let tmp = temp_root("verify");
        write(&tmp.join("src").join("main.ax"), "def main(): pass\n");
        write(&tmp.join("src").join("util.ax"), "def u(): pass\n");
        let before = manifest_hash(&tmp).unwrap();
        let tgz = tmp.join("out.tar.gz");
        pack(&tmp, "mypkg", &tgz).unwrap();
        let dest = tmp.join("unpacked");
        unpack(&tgz, &dest, "mypkg").unwrap();
        let after = manifest_hash(&dest).unwrap();
        assert_eq!(before, after, "extracted tree must hash identically");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pack_is_byte_stable() {
        let tmp = temp_root("stable");
        write(&tmp.join("b.ax"), "b\n");
        write(&tmp.join("a.ax"), "a\n");
        write(&tmp.join("sub").join("c.ax"), "c\n");
        let t1 = tmp.join("p1.tar.gz");
        let t2 = tmp.join("p2.tar.gz");
        pack(&tmp, "mypkg", &t1).unwrap();
        pack(&tmp, "mypkg", &t2).unwrap();
        assert_eq!(
            crate::cache::sha256_file(&t1).unwrap(),
            crate::cache::sha256_file(&t2).unwrap(),
            "two packs of the same tree must be byte-identical"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn traversal_entry_is_rejected() {
        // hand-craft a tar header with a `..` entry name (tar-rs refuses to
        // write such paths itself, so the bytes are built by hand)
        let tmp = temp_root("evil");
        let tgz = tmp.join("evil.tar.gz");
        {
            let f = File::create(&tgz).unwrap();
            let mut enc = flate2::write::GzEncoder::new(f, flate2::Compression::default());
            let mut block = [0u8; 512];
            block[..11].copy_from_slice(b"../evil.txt");
            // size = 2 (octal, NUL-terminated), field at offset 124
            block[124..136].copy_from_slice(b"00000000002\x00");
            block[156] = b'0'; // regular file
            // cksum = sum of all header bytes with the cksum field as spaces
            let mut sum: u32 = 0;
            for (i, b) in block.iter().enumerate() {
                sum += if (148..156).contains(&i) { 32 } else { *b as u32 };
            }
            block[148..156].copy_from_slice(format!("{sum:06o}\x00 ").as_bytes());
            use std::io::Write;
            enc.write_all(&block).unwrap();
            enc.write_all(b"oh").unwrap();
            enc.write_all(&[0u8; 512]).unwrap(); // end-of-archive
            enc.finish().unwrap();
        }
        let dest = tmp.join("out");
        let err = unpack(&tgz, &dest, "mypkg").unwrap_err();
        assert!(format!("{err}").contains("malformed"), "got: {err}");
        assert!(!dest.join("evil.txt").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A tarball the packer would never produce: an entry NAME, an entry KIND
    /// and an optional LINK TARGET of our choosing. The escapes that matter
    /// are about the name and the link rather than the bytes, and tar-rs
    /// refuses to `set_path` some of the names we need to test (any path with
    /// `..`), so the 512-byte blocks are written directly — the same
    /// hand-rolling the existing corruption test uses.
    fn octal(mut v: u64, width: usize) -> Vec<u8> {
        let mut out = vec![b'0'; width - 1];
        for i in (0..width - 1).rev() {
            out[i] = b'0' + (v & 7) as u8;
            v >>= 3;
        }
        out.push(0);
        out
    }

    fn raw_header(name: &str, size: u64, typeflag: u8, link: &str) -> Vec<u8> {
        let mut h = vec![0u8; 512];
        let put = |h: &mut Vec<u8>, at: usize, bytes: &[u8]| {
            let n = bytes.len().min(h.len() - at);
            h[at..at + n].copy_from_slice(&bytes[..n]);
        };
        put(&mut h, 0, name.as_bytes());
        put(&mut h, 100, &octal(0o644, 8));
        put(&mut h, 108, &octal(0, 8));
        put(&mut h, 116, &octal(0, 8));
        put(&mut h, 124, &octal(size, 12));
        put(&mut h, 136, &octal(0, 12));
        h[148..156].fill(b' ');
        h[156] = typeflag;
        put(&mut h, 157, link.as_bytes());
        put(&mut h, 257, b"ustar\0");
        put(&mut h, 263, b"00");
        let sum: u64 = h.iter().map(|b| *b as u64).sum();
        let cks = format!("{:06o}\0 ", sum);
        put(&mut h, 148, cks.as_bytes());
        h
    }

    fn raw_tar_gz(dest: &Path, entries: &[(&str, EntryKind, Option<&str>)]) {
        use std::io::Write as _;
        let f = File::create(dest).unwrap();
        let mut gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
        for (name, kind, link) in entries {
            let (flag, lname) = match kind {
                EntryKind::Link => (b'1', link.unwrap_or("")),
                EntryKind::Symlink => (b'2', link.unwrap_or("")),
                _ => (b'0', ""),
            };
            gz.write_all(&raw_header(name, 0, flag, lname)).unwrap();
        }
        gz.write_all(&[0u8; 1024]).unwrap();
        gz.finish().unwrap();
    }

    use tar::EntryType as EntryKind;
    /// THE FIRST ESCAPE: an entry whose remainder is ABSOLUTE. `Path::join`
    /// with an absolute path discards the destination entirely, so
    /// `stripped.contains("..")` never got a chance to fire and the file
    /// landed at the path it named. Reproduced on Windows with a drive
    /// letter and on POSIX with a leading `/` (`demo//etc/passwd`).
    #[test]
    fn unpack_rejects_an_absolute_entry_path() {
        let tmp = temp_root("abs");
        let outside = tmp.join("outside-archive.txt");
        let tgz = tmp.join("evil.tar.gz");
        let abs = if cfg!(windows) {
            format!("C:{}", outside.display().to_string().replace('\\', "/"))
        } else {
            outside.display().to_string().clone()
        };
        raw_tar_gz(&tgz, &[("demo/C:/Users/victim/outside-archive.txt", EntryKind::Regular, None)]);
        let dest = tmp.join("out");
        let err = unpack(&tgz, &dest, "demo").unwrap_err();
        assert!(format!("{err}").contains("absolute"), "got: {err}");
        assert!(!outside.exists(), "the archive wrote outside the destination");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A POSIX-shaped absolute remainder, checked on Windows too, because the
    /// component walk is what rejects it and it must not depend on the host.
    #[test]
    fn unpack_rejects_a_posix_absolute_remainder() {
        let tmp = temp_root("posixabs");
        let tgz = tmp.join("evil.tar.gz");
        raw_tar_gz(&tgz, &[("demo//etc/passwd", EntryKind::Regular, None)]);
        let dest = tmp.join("out");
        let err = unpack(&tgz, &dest, "demo").unwrap_err();
        assert!(format!("{err}").contains("absolute"), "got: {err}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The escape that DID work before still works as a defence, and the
    /// substring test's false positive is gone: `a..b` is a legal name.
    #[test]
    fn unpack_rejects_parent_segments_but_allows_dots_in_a_name() {
        let tmp = temp_root("dots");
        let tgz = tmp.join("ok.tar.gz");
        raw_tar_gz(&tgz, &[("demo/a..b/keep.txt", EntryKind::Regular, None)]);
        unpack(&tgz, &tmp.join("ok"), "demo").unwrap();
        assert!(tmp.join("ok").join("a..b").join("keep.txt").exists());

        let tgz2 = tmp.join("bad.tar.gz");
        raw_tar_gz(&tgz2, &[("demo/a/../../escape.txt", EntryKind::Regular, None)]);
        let err = unpack(&tgz2, &tmp.join("bad"), "demo").unwrap_err();
        assert!(format!("{err}").contains("escapes"), "got: {err}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// THE SECOND ESCAPE: `tar::Entry::unpack` honours the archive's own
    /// link targets. A hard link to a file outside the destination passes the
    /// content check (the manifest hash covers CONTENT, not link identity),
    /// and the shim write that follows it overwrites the external file. Links
    /// are now refused outright — absolute, climbing, and in-package alike —
    /// which also closes the symlink-parent variant (an entry under a symlinked
    /// directory becomes an escape without any link target at all).
    #[test]
    fn unpack_rejects_links_outright() {
        let tmp = temp_root("links");
        let cases: [(&str, EntryKind, &str); 3] = [
            ("demo/index.ax", EntryKind::Link, "C:/Users/victim/important.txt"),
            ("demo/index.ax", EntryKind::Link, "../../outside/important.txt"),
            ("demo/lib.ax", EntryKind::Symlink, "demo/index.ax"),
        ];
        for (i, (name, kind, target)) in cases.iter().enumerate() {
            let tgz = tmp.join(format!("case{i}.tar.gz"));
            raw_tar_gz(&tgz, &[(name, *kind, Some(target))]);
            let dest = tmp.join(format!("out{i}"));
            let err = unpack(&tgz, &dest, "demo").unwrap_err();
            let msg = format!("{err}");
            assert!(msg.contains("may not contain links"), "case {i}: {msg}");
            assert!(!dest.join("index.ax").exists(), "case {i} wrote something");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}