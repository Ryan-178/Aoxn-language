//! stdlib/hashlib.ax + hmac.ax + base64.ax tests (v0.44.0).
//! FIPS 180 / RFC 1321 digest vectors, RFC 4231-style HMAC vectors (the
//! long-key case cross-checked against node:crypto), RFC 4648 base64
//! vectors, incremental == one-shot, and the decode rejection paths.
use std::process::Command;

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

fn abs(rel: &str) -> String {
    // forward slashes: backslashes would read as escape sequences inside
    // the Aoxn import string literal
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(rel)
        .display()
        .to_string()
        .replace('\\', "/")
}

const SRC: &str = r#"
import * from "stdlib/hashlib.ax"
import * from "stdlib/hmac.ax"
import * from "stdlib/base64.ax"

# prints PASS/FAIL per vector; a FAIL list is collected and the exit code
# reports it, so one run shows every drift at once
def main() -> int:
    fails = 0
    # SHA-256 (FIPS 180 vectors)
    if sha256_str("abc") == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad":
        print("PASS sha256-abc")
    else:
        print("FAIL sha256-abc " + sha256_str("abc"))
        fails = fails + 1
    if sha256_str("") == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855":
        print("PASS sha256-empty")
    else:
        print("FAIL sha256-empty " + sha256_str(""))
        fails = fails + 1
    # a > 55-byte message exercises the multi-block padding branch
    long = "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
    if sha256_str(long) == "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1":
        print("PASS sha256-long")
    else:
        print("FAIL sha256-long " + sha256_str(long))
        fails = fails + 1
    # incremental == one-shot across chunk boundaries
    c = sha256_new()
    c = sha256_update(c, as_ptr(long), 20)
    c = sha256_update(c, as_ptr(long) + 20, 20)
    c = sha256_update(c, as_ptr(long) + 40, len(long) - 40)
    if sha256_digest(c) == sha256_str(long):
        print("PASS sha256-incr")
    else:
        print("FAIL sha256-incr")
        fails = fails + 1
    # SHA-1
    if sha1_str("abc") == "a9993e364706816aba3e25717850c26c9cd0d89d":
        print("PASS sha1-abc")
    else:
        print("FAIL sha1-abc " + sha1_str("abc"))
        fails = fails + 1
    if sha1_str("") == "da39a3ee5e6b4b0d3255bfef95601890afd80709":
        print("PASS sha1-empty")
    else:
        print("FAIL sha1-empty " + sha1_str(""))
        fails = fails + 1
    if sha1_str(long) == "84983e441c3bd26ebaae4aa1f95129e5e54670f1":
        print("PASS sha1-long")
    else:
        print("FAIL sha1-long " + sha1_str(long))
        fails = fails + 1
    # MD5 (RFC 1321 vectors)
    if md5_str("") == "d41d8cd98f00b204e9800998ecf8427e":
        print("PASS md5-empty")
    else:
        print("FAIL md5-empty " + md5_str(""))
        fails = fails + 1
    if md5_str("abc") == "900150983cd24fb0d6963f7d28e17f72":
        print("PASS md5-abc")
    else:
        print("FAIL md5-abc " + md5_str("abc"))
        fails = fails + 1
    if md5_str("message digest") == "f96b697d7cb7938d525a2f31aaf161d0":
        print("PASS md5-digest")
    else:
        print("FAIL md5-digest " + md5_str("message digest"))
        fails = fails + 1
    if md5_str("abcdefghijklmnopqrstuvwxyz") == "c3fcd3d76192e4007dfb496cca67e13b":
        print("PASS md5-alpha")
    else:
        print("FAIL md5-alpha " + md5_str("abcdefghijklmnopqrstuvwxyz"))
        fails = fails + 1
    # HMAC (RFC 4231 test case 2 / RFC 2202 equivalents)
    msg = "The quick brown fox jumps over the lazy dog"
    if hmac_sha256("key", msg) == "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8":
        print("PASS hmac-sha256")
    else:
        print("FAIL hmac-sha256 " + hmac_sha256("key", msg))
        fails = fails + 1
    if hmac_sha1("key", msg) == "de7c9b85b8b78aa6bc8a7a36f70a90701c9db4d9":
        print("PASS hmac-sha1")
    else:
        print("FAIL hmac-sha1 " + hmac_sha1("key", msg))
        fails = fails + 1
    if hmac_md5("key", msg) == "80070713463e7749b90c2dc24911e275":
        print("PASS hmac-md5")
    else:
        print("FAIL hmac-md5 " + hmac_md5("key", msg))
        fails = fails + 1
    # a key LONGER than the block takes the hash-then-pad branch
    # RFC 4231 case 6: 131-byte key of 0xaa
    key131 = malloc(131)
    fill_bytes(key131, 131, 170)
    hm = hmac_hex(key131, 131, as_ptr(msg), len(msg), sha256_raw, 32)
    if hm == "cb12e2903bcb35afbdaedb6a8e3987cd58ca549c59c36ca5471037a5f5d684ff":
        print("PASS hmac-longkey")
    else:
        print("FAIL hmac-longkey " + hm)
        fails = fails + 1
    # base64 round trips (RFC 4648)
    if b64_encode_str("") == "":
        print("PASS b64-empty")
    else:
        fails = fails + 1
        print("FAIL b64-empty")
    if b64_encode_str("foobar") == "Zm9vYmFy":
        print("PASS b64-foobar")
    else:
        fails = fails + 1
        print("FAIL b64-foobar " + b64_encode_str("foobar"))
    if b64_encode_str("foob") == "Zm9vYg==":
        print("PASS b64-foob")
    else:
        fails = fails + 1
        print("FAIL b64-foob " + b64_encode_str("foob"))
    if b64_decode_str("Zm9vYmFy") == "foobar":
        print("PASS b64-decode")
    else:
        fails = fails + 1
        print("FAIL b64-decode " + b64_decode_str("Zm9vYmFy"))
    r = b64_decode("Zm9vYg==")
    if r.n == 4:
        print("PASS b64-decode-pad")
    else:
        fails = fails + 1
        print("FAIL b64-decode-pad " + str(r.n))
    r2 = b64_decode("Zm9vYg")
    if r2.n == 4 and b64_decode_str("Zm9vYg") == "foob":
        print("PASS b64-unpadded")
    else:
        fails = fails + 1
        print("FAIL b64-unpadded " + str(r2.n))
    r3 = b64_decode("a!")
    if r3.n == -1:
        print("PASS b64-invalid")
    else:
        fails = fails + 1
        print("FAIL b64-invalid " + str(r3.n))
    r4 = b64_decode("abcde")
    if r4.n == -1:
        print("PASS b64-orphan")
    else:
        fails = fails + 1
        print("FAIL b64-orphan")
    if b64_encode_str("subjects? _") == "c3ViamVjdHM/IF8=":
        print("PASS b64-plus-slash")
    else:
        fails = fails + 1
        print("FAIL b64-plus-slash " + b64_encode_str("subjects? _"))
    if b64_encode_url_str("subjects? _") == "c3ViamVjdHM_IF8=":
        print("PASS b64-url")
    else:
        fails = fails + 1
        print("FAIL b64-url " + b64_encode_url_str("subjects? _"))
    print("DONE fails=" + str(fails))
    return fails

# fill n bytes at p with the byte value v
def fill_bytes(p: int, n: int, v: int):
    i = 0
    while i < n:
        store_u8(p, i, v)
        i = i + 1

# hmac over (ptr, len) key and message, hex output
def hmac_hex(kp: int, klen: int, mp: int, mlen: int, h: fn(int, int, int) -> int, hlen: int) -> string:
    out = malloc(hlen)
    hmac_bytes(kp, klen, mp, mlen, h, hlen, out)
    s = hashlib_hex(out, hlen)
    free(out)
    return s

"#;

#[test]
fn hashlib_module_vectors() {
    if aoxn::find_clang().is_none() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    // the drivers import stdlib modules BY NAME, which exercises the
    // AOXN_STDLIB resolution path (env -> <root>/lib/stdlib -> checkout)
    std::env::set_var("AOXN_STDLIB", abs("stdlib"));
    let dir = std::env::temp_dir().join(format!("aoxn-hashlib_module_vectors-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("hashlib_module_vectors.ax");
    let exe = dir.join(format!("hashlib_module_vectors{EXE}"));
    std::fs::write(&src_path, SRC).unwrap();
    aoxn::build_paths_opts(&[src_path.display().to_string()], &exe, true, &[], &[])
        .unwrap_or_else(|d| panic!("driver failed to compile: {d:?}"));
    let out =
    Command::new(&exe).output().expect("failed to run driver");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "driver exited {:?}
stdout:
{text}
stderr:
{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("DONE fails=0"), "driver reported failures:
{text}");
    assert!(!text.contains("FAIL"), "driver printed FAIL:
{text}");
    for marker in [
        "sha256-abc",
        "sha256-empty",
        "sha256-long",
        "sha256-incr",
        "sha1-abc",
        "sha1-empty",
        "sha1-long",
        "md5-empty",
        "md5-abc",
        "md5-digest",
        "md5-alpha",
        "hmac-sha256",
        "hmac-sha1",
        "hmac-md5",
        "hmac-longkey",
        "b64-empty",
        "b64-foobar",
        "b64-foob",
        "b64-decode",
        "b64-decode-pad",
        "b64-unpadded",
        "b64-invalid",
        "b64-orphan",
        "b64-plus-slash",
        "b64-url",
    ] {
        assert!(
            text.contains(&format!("PASS {marker}")),
            "missing PASS {marker}
{text}"
        );
    }
}
