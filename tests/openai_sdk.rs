//! stdlib/openai SDK tests.
//!
//! Two layers:
//! - PURE: codecs (base64 / percent / UTF-16), the JSON DOM (parse, dumps,
//!   getters, escapes), URL splitting, header assembly, the retry policy,
//!   SSE event extraction, request-body rendering and response accessors —
//!   one driver exe, every layer offline;
//! - END-TO-END: a full WinHTTP round trip against a MOCK OpenAI server
//!   written in Aoxn itself (web/sock_win.ax) — a real chat completion, a
//!   real SSE stream consumed through oa_chat_stream, and a 401 error path.
//!   No external service, no credentials: the mock is spawned on a loopback
//!   port and answers three canned requests.
//!
//! Both drivers need `-l winhttp` (the externs are declared even where the
//! gated paths never run), the mock needs `-l ws2_32`.

use std::path::PathBuf;
use std::process::Command;

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

fn abs(rel: &str) -> String {
    // forward slashes: backslashes would read as escape sequences inside
    // the Aoxn import string literal
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(rel)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aoxn-openai-{}-{}", name, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// clang is required to turn emitted C into an executable (see tests/ui.rs).
fn have_clang() -> bool {
    aoxn::find_clang().is_some()
}

fn build(src: &str, name: &str, libs: &[&str]) -> PathBuf {
    let dir = temp_dir(name);
    let src_path = dir.join(format!("{name}.ax"));
    let exe = dir.join(format!("{name}{EXE}"));
    std::fs::write(&src_path, src).unwrap();
    let libs: Vec<String> = libs.iter().map(|s| s.to_string()).collect();
    aoxn::build_paths_opts(&[src_path.display().to_string()], &exe, true, &libs, &[])
        .unwrap_or_else(|d| panic!("{name} failed to compile: {d:?}"));
    exe
}

// ============================================================
// pure layers
// ============================================================

const PURE_SRC: &str = r#"
def main() -> int:
    # ---- base64: RFC 4648 vectors ----
    print("PASS b64-empty " + net_b64_encode_str(""))
    print("PASS b64-f " + net_b64_encode_str("f"))
    print("PASS b64-fo " + net_b64_encode_str("fo"))
    print("PASS b64-foo " + net_b64_encode_str("foo"))
    print("PASS b64-foob " + net_b64_encode_str("foob"))
    print("PASS b64-fooba " + net_b64_encode_str("fooba"))
    print("PASS b64-foobar " + net_b64_encode_str("foobar"))
    # ---- percent-encoding ----
    print("PASS urlenc " + net_url_encode("a b/c~d"))
    # ---- UTF-16 round trip ----
    w = net_to_wide("héllo 😀")
    print("PASS utf16rt " + net_from_wide(w))
    # ---- JSON: parse / dumps / round trip ----
    p = j_parse("{\"model\":\"gpt-4o\",\"n\":42,\"f\":0.5,\"ok\":true,\"arr\":[1,2,3],\"nested\":{\"a\":\"b\"}}")
    print("PASS json-err " + str(p.err))
    print("PASS json-model " + j_get_str(p.dom, p.dom.last, "model", "?"))
    print("PASS json-n " + str(j_get_int(p.dom, p.dom.last, "n", -1)))
    print("PASS json-ok " + str(j_get_bool(p.dom, p.dom.last, "ok", False)))
    arr = j_obj_get(p.dom, p.dom.last, "arr")
    print("PASS json-arr " + str(j_len(p.dom, arr)) + ":" + str(j_arr_int(p.dom, arr, 1, -1)))
    print("PASS json-nested " + j_get_str(p.dom, j_obj_get(p.dom, p.dom.last, "nested"), "a", "?"))
    rt = j_roundtrip("{\"k\":[1,2.5,\"s\",null,true]}")
    print("PASS json-rt " + rt)
    esc = j_parse("\"a\\nb\\u00e9\\ud83d\\ude00\\t\"")
    print("PASS json-esc " + j_dumps(esc.dom, esc.dom.last))
    lone = j_parse("\"\\ud83d\"")
    print("PASS json-lone " + str(lone.err))
    bad = j_parse("{\"a\":}")
    print("PASS json-bad " + str(bad.err))
    trail = j_parse("[] x")
    print("PASS json-trail " + str(trail.err))
    print("PASS json-fmt " + j_fmt_f(0.7) + " " + j_fmt_f(-0.5) + " " + j_fmt_f(0.0) + " " + j_fmt_f(150000000000000000000.0))
    # ---- URL splitting ----
    u1 = net_url_split("https://api.openai.com/v1/chat/completions")
    print("PASS url1 " + str(u1.secure) + " " + u1.host + " " + str(u1.port) + " " + u1.path)
    u2 = net_url_split("http://localhost:8080/v1/models")
    print("PASS url2 " + str(u2.secure) + " " + u2.host + " " + str(u2.port) + " " + u2.path)
    u3 = net_url_split("https://host:8443/a/b?q=1")
    print("PASS url3 " + u3.host + " " + str(u3.port) + " " + u3.path)
    u4 = net_url_split("notaurl")
    print("PASS url4 " + str(len(u4.host) == 0))
    # ---- headers ----
    h = oa_headers_build("sk-test", "", "", "{}")
    print("PASS hdr-auth " + str(str_sub(h, 0, 22) == "Authorization: Bearer "))
    print("PASS hdr-ctype " + str(find_str(h, "Content-Type: application/json") >= 0))
    h2 = oa_headers_build("sk", "o", "p", "")
    print("PASS hdr-org " + str(find_str(h2, "OpenAI-Organization: o") >= 0) + str(find_str(h2, "OpenAI-Project: p") >= 0))
    # ---- retry policy ----
    print("PASS retry " + str(net_retry_delay(0, 2, 429, 0)) + "," + str(net_retry_delay(1, 2, 500, 0)) + "," + str(net_retry_delay(2, 2, 500, 0)) + "," + str(net_retry_delay(0, 2, 404, 0)) + "," + str(net_retry_delay(0, 2, 429, 30)) + "," + str(net_retry_delay(0, 2, 0, 0)))
    # ---- request bodies ----
    roles = vec_new()
    contents = vec_new()
    roles = vec_push_str(roles, "system")
    contents = vec_push_str(contents, "Terse.")
    roles = vec_push_str(roles, "user")
    contents = vec_push_str(contents, "Say \"hi\"")
    print("PASS body-chat " + oa_body_chat("gpt-4o", roles, contents, False, 0.7, 1, 0))
    print("PASS body-chat-stream " + oa_body_chat("gpt-4o", roles, contents, True, 0.0, 0, 256))
    print("PASS body-resp " + oa_body_responses("gpt-4o", "haiku", "be brief"))
    print("PASS body-emb " + oa_body_embeddings("e3-small", "hello", False, vec_new()))
    ins = vec_new()
    ins = vec_push_str(ins, "a")
    ins = vec_push_str(ins, "b")
    print("PASS body-emb-many " + oa_body_embeddings("e3-small", "", True, ins))
    print("PASS body-mod " + oa_body_moderations("", "x"))
    # ---- SSE ----
    s = net_sse_new()
    f1 = "data: {\"a\":1}\n\ndata: {\"b\":2}\n\ndata: [DONE]\n\n"
    s = net_sse_feed(s, as_ptr(f1), len(f1))
    s = net_sse_next(s)
    e1 = s.res
    s = net_sse_next(s)
    e2 = s.res
    s = net_sse_next(s)
    print("PASS sse " + e1 + "|" + e2 + "|" + str(s.done))
    # split feed: one event arriving across three chunks
    s2 = net_sse_new()
    part1 = "data: {\"c\":"
    part2 = "42}\n"
    part3 = "\n"
    s2 = net_sse_feed(s2, as_ptr(part1), len(part1))
    s2 = net_sse_next(s2)
    mid = s2.res
    s2 = net_sse_feed(s2, as_ptr(part2), len(part2))
    s2 = net_sse_next(s2)
    mid2 = s2.res
    s2 = net_sse_feed(s2, as_ptr(part3), len(part3))
    s2 = net_sse_next(s2)
    print("PASS sse-split " + str(len(mid) == 0) + str(len(mid2) == 0) + s2.res)
    # CRLF endings and a comment line (CRLF spelled as raw bytes: the
    # language has no \r escape)
    s3 = net_sse_new()
    nl = crlf()
    l1 = ": keep-alive"
    l2 = "data: {\"d\":9}"
    f3 = l1 + nl + nl + l2 + nl + nl
    s3 = net_sse_feed(s3, as_ptr(f3), len(f3))
    s3 = net_sse_next(s3)
    print("PASS sse-crlf " + s3.res)
    # the named-event form (Anthropic's shape): `event:` names the frame and
    # must NOT leak into the next frame. A frame with an event name but no
    # data line carries nothing, so it is dropped rather than dispatched.
    # OpenAI never sends either, so this pins the shared filter itself.
    s4 = net_sse_new()
    f4 = "event: message_start" + nl + nl + "event: content_block_delta" + nl + "data: {\"e\":1}" + nl + nl + "data: {\"f\":2}" + nl + nl
    s4 = net_sse_feed(s4, as_ptr(f4), len(f4))
    s4 = net_sse_next(s4)
    ev1 = s4.event
    pay1 = s4.res
    s4 = net_sse_next(s4)
    print("PASS sse-event [" + ev1 + "][" + pay1 + "] [" + s4.event + "][" + s4.res + "]")
    # ---- stream chunk helpers ----
    chunk = "{\"id\":\"x\",\"choices\":[{\"delta\":{\"content\":\"Hel\"},\"finish_reason\":null}]}"
    print("PASS chunk " + oa_stream_text(chunk) + "|" + oa_stream_finish(chunk))
    chunk2 = "{\"id\":\"x\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}"
    print("PASS chunk2 " + oa_stream_text(chunk2) + "|" + oa_stream_finish(chunk2))
    # ---- response accessors on canned bodies ----
    body = "{\"id\":\"cmpl-1\",\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"Hello!\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"total_tokens\":12}}"
    p2 = j_parse(body)
    r = OaResp(status=200, dom=p2.dom, pok=1, pmsg="", raw=body, request_id="", terr=0, tmsg="", retries=0)
    print("PASS resp-text " + oa_chat_text(r) + " " + oa_chat_finish(r) + " " + oa_chat_model(r))
    print("PASS resp-usage " + str(oa_usage_in(r)) + "/" + str(oa_usage_out(r)) + "/" + str(oa_usage_total(r)))
    print("PASS resp-ok " + str(oa_resp_ok(r)) + "|" + oa_err_msg(r))
    eb = "{\"error\":{\"message\":\"Incorrect API key\",\"type\":\"invalid_request_error\",\"code\":\"invalid_api_key\"}}"
    q = j_parse(eb)
    r2 = OaResp(status=401, dom=q.dom, pok=1, pmsg="", raw=eb, request_id="", terr=0, tmsg="", retries=0)
    print("PASS resp-401 " + str(oa_resp_ok(r2)) + "|" + oa_err_msg(r2))
    r3 = OaResp(status=0, dom=jdom_new(), pok=0, pmsg="x", raw="", request_id="", terr=1, tmsg="timeout", retries=2)
    print("PASS resp-terr " + str(oa_resp_ok(r3)) + "|" + oa_err_msg(r3))
    rb = "{\"id\":\"resp_1\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hi there\"}]}]}"
    wp = j_parse(rb)
    r4 = OaResp(status=200, dom=wp.dom, pok=1, pmsg="", raw=rb, request_id="", terr=0, tmsg="", retries=0)
    print("PASS resp-outputtext " + oa_responses_text(r4))
    lb = "{\"object\":\"list\",\"data\":[{\"id\":\"m1\",\"object\":\"model\"},{\"id\":\"m2\",\"object\":\"model\"}]}"
    v = j_parse(lb)
    r5 = OaResp(status=200, dom=v.dom, pok=1, pmsg="", raw=lb, request_id="", terr=0, tmsg="", retries=0)
    print("PASS resp-list " + str(oa_data_len(r5)) + " " + oa_data_id(r5, 0) + " " + oa_data_id(r5, 1))
    emb = "{\"data\":[{\"embedding\":[0.25,-0.5,2.0]}]}"
    z = j_parse(emb)
    r6 = OaResp(status=200, dom=z.dom, pok=1, pmsg="", raw=emb, request_id="", terr=0, tmsg="", retries=0)
    print("PASS resp-emb " + str(oa_embedding_len(r6, 0)) + " " + j_fmt_f(oa_embedding_at(r6, 0, 0)) + " " + j_fmt_f(oa_embedding_at(r6, 0, 1)) + " " + j_fmt_f(oa_embedding_at(r6, 0, 2)))
    return 0

# find the first occurrence of a string in a string (-1 = absent)
def find_str(s: string, needle: string) -> int:
    n = len(needle)
    i = 0
    while i + n <= len(s):
        if str_sub(s, i, i + n) == needle:
            return i
        i = i + 1
    return -1

# CRLF as raw bytes (no \r escape in the language)
def crlf() -> string:
    p = malloc(3)
    store_u8(p, 0, 13)
    store_u8(p, 1, 10)
    store_u8(p, 2, 0)
    return as_string(p)
"#;

#[test]
fn openai_sdk_pure_layers() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    let src = format!("import * from \"{}\"\n{}", abs("stdlib/openai/client"), PURE_SRC);
    let exe = build(&src, "oa_pure", &["winhttp"]);
    let out = Command::new(&exe).output().expect("failed to run pure driver");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "pure driver exited {:?}\nstdout:\n{text}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    for marker in [
        "b64-empty ", "b64-foobar Zm9vYmFy", "urlenc a%20b%2Fc~d", "utf16rt héllo 😀",
        "json-err 0", "json-model gpt-4o", "json-n 42", "json-ok true", "json-arr 3:2",
        "json-nested b", "json-rt {\"k\":[1,2.5,\"s\",null,true]}", "json-esc \"a\\nbé😀\\t\"",
        "json-lone 1", "json-bad 1", "json-trail 1", "json-fmt 0.7 -0.5 0 1.5e20",
        "url3 host 8443 /a/b", "url4 true",
        "hdr-auth true", "hdr-ctype true", "hdr-org truetrue",
        "retry 500,1000,-1,-1,30000,500",
        "body-chat {\"model\":\"gpt-4o\",\"messages\":[{\"role\":\"system\",\"content\":\"Terse.\"},{\"role\":\"user\",\"content\":\"Say \\\"hi\\\"\"}],\"temperature\":0.7}",
        "body-chat-stream {\"model\":\"gpt-4o\",\"messages\":[{\"role\":\"system\",\"content\":\"Terse.\"},{\"role\":\"user\",\"content\":\"Say \\\"hi\\\"\"}],\"max_tokens\":256,\"stream\":true}",
        "body-resp {\"model\":\"gpt-4o\",\"input\":\"haiku\",\"instructions\":\"be brief\"}",
        "body-emb {\"model\":\"e3-small\",\"input\":\"hello\"}",
        "body-emb-many {\"model\":\"e3-small\",\"input\":[\"a\",\"b\"]}",
        "body-mod {\"input\":\"x\"}",
        "sse {\"a\":1}|{\"b\":2}|true", "sse-split truetrue{\"c\":42}", "sse-crlf {\"d\":9}",
        "sse-event [content_block_delta][{\"e\":1}] [][{\"f\":2}]",
        "chunk Hel|", "chunk2 |stop",
        "resp-text Hello! stop gpt-4o", "resp-usage 10/2/12", "resp-ok true|",
        "resp-401 false|openai 401: Incorrect API key", "resp-terr false|timeout",
        "resp-outputtext Hi there", "resp-list 2 m1 m2",
        "resp-emb 3 0.25 -0.5 2",
    ] {
        assert!(text.contains(marker), "missing marker {marker:?}\nstdout:\n{text}");
    }
    assert!(text.contains("b64-f Zg=="), "b64 vector\n{text}");
    assert!(text.contains("b64-fo Zm8="), "b64 vector\n{text}");
    assert!(text.contains("b64-foo Zm9v"), "b64 vector\n{text}");
    assert!(text.contains("b64-foob Zm9vYg=="), "b64 vector\n{text}");
    assert!(text.contains("b64-fooba Zm9vYmE="), "b64 vector\n{text}");
}

// ============================================================
// end-to-end against the Aoxn-written mock server
// ============================================================

const MOCK_SRC: &str = r#"
extern def getenv(name: string) -> string

def env_of(name: string) -> string:
    p = getenv(name)
    if as_ptr(p) == 0:
        return ""
    return p

def env_port() -> int:
    s = env_of("MOCK_PORT")
    if len(s) == 0:
        return 0
    port = 0
    i = 0
    while i < len(s):
        c = load_u8(s, i)
        if c < 48 or c > 57:
            return 0
        port = port * 10 + (c - 48)
        i = i + 1
    return port

def crlf() -> string:
    p = malloc(3)
    store_u8(p, 0, 13)
    store_u8(p, 1, 10)
    store_u8(p, 2, 0)
    return as_string(p)

# first index of a literal inside a raw buffer (-1 = absent)
def find_bytes(hay: int, hlen: int, needle: string, start: int) -> int:
    nl = len(needle)
    np = as_ptr(needle)
    i = start
    while i + nl <= hlen:
        k = 0
        same = 1
        while k < nl:
            if load_u8(hay, i + k) != load_u8(np, k):
                same = 0
                k = nl
            else:
                k = k + 1
        if same == 1:
            return i
        i = i + 1
    return -1

def hdr_end(buf: int, n: int) -> int:
    i = 0
    while i + 3 < n:
        if load_u8(buf, i) == 13 and load_u8(buf, i + 1) == 10 and load_u8(buf, i + 2) == 13 and load_u8(buf, i + 3) == 10:
            return i
        i = i + 1
    return -1

def content_length(buf: int, n: int) -> int:
    at = find_bytes(buf, n, "Content-Length: ", 0)
    if at < 0:
        return 0
    i = at + 16
    v = 0
    while i < n:
        c = load_u8(buf, i)
        if c < 48 or c > 57:
            break
        v = v * 10 + (c - 48)
        i = i + 1
    return v

def respond(cs: int, status: string, ctype: string, body: string) -> int:
    nl = crlf()
    h = status + nl + "Content-Type: " + ctype + nl + "Content-Length: " + str(len(body)) + nl + "Connection: close" + nl + nl
    net_send_all(cs, as_ptr(h), len(h))
    if len(body) > 0:
        return net_send_all(cs, as_ptr(body), len(body))
    return 0

def completion_body() -> string:
    return "{\"id\":\"cmpl-1\",\"object\":\"chat.completion\",\"model\":\"mock-gpt\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"Hello from the mock\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":5,\"total_tokens\":17}}"

def sse_body() -> string:
    c1 = "{\"id\":\"cmpl-2\",\"object\":\"chat.completion.chunk\",\"model\":\"mock-gpt\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hel\"},\"finish_reason\":null}]}"
    c2 = "{\"id\":\"cmpl-2\",\"object\":\"chat.completion.chunk\",\"model\":\"mock-gpt\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo!\"},\"finish_reason\":null}]}"
    c3 = "{\"id\":\"cmpl-2\",\"object\":\"chat.completion.chunk\",\"model\":\"mock-gpt\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}"
    return "data: " + c1 + "\n\n" + "data: " + c2 + "\n\n" + "data: " + c3 + "\n\n" + "data: [DONE]" + "\n\n"

def error_body() -> string:
    return "{\"error\":{\"message\":\"Incorrect API key provided: sk-test\",\"type\":\"invalid_request_error\",\"param\":null,\"code\":\"invalid_api_key\"}}"

# serve one request; returns 1 when it was a plain (non-stream) chat
def serve(cs: int) -> int:
    buf = malloc(32768)
    total = 0
    he = -1
    while total < 32000:
        got = net_recv(cs, buf + total, 4096)
        if got <= 0:
            break
        total = total + got
        he = hdr_end(buf, total)
        if he >= 0:
            if total - (he + 4) >= content_length(buf, he):
                break
    if he < 0:
        respond(cs, "HTTP/1.1 400 Bad Request", "application/json", "{}")
        return 0
    # the path is the second token of the request line
    sp1 = find_bytes(buf, he, " ", 0)
    sp2 = find_bytes(buf, he, " ", sp1 + 1)
    plen = sp2 - sp1 - 1
    pp = malloc(plen + 1)
    memcpy(pp, buf + sp1 + 1, plen)
    store_u8(pp, plen, 0)
    path = as_string(pp)
    blen = total - he - 4
    is_stream = find_bytes(buf + he + 4, blen, "\"stream\":true", 0) >= 0
    if path == "/v1/chat/completions" and is_stream:
        respond(cs, "HTTP/1.1 200 OK", "text/event-stream", sse_body())
        return 0
    if path == "/v1/chat/completions":
        respond(cs, "HTTP/1.1 200 OK", "application/json", completion_body())
        return 1
    if path == "/v1/error":
        respond(cs, "HTTP/1.1 401 Unauthorized", "application/json", error_body())
        return 0
    respond(cs, "HTTP/1.1 404 Not Found", "application/json", "{\"error\":{\"message\":\"no such route\"}}")
    return 0

def main() -> int:
    port = env_port()
    if port == 0:
        print("mock-port-fail")
        return 1
    if net_init() != 0:
        print("mock-init-fail")
        return 1
    ls = net_listen(port)
    if ls == -1:
        print("mock-listen-fail")
        return 1
    print("mock-ready")
    served = 0
    want = 0
    ctxt = env_of("MOCK_COUNT")
    i = 0
    while i < len(ctxt):
        want = want * 10 + (load_u8(ctxt, i) - 48)
        i = i + 1
    while served < want:
        cs = net_accept(ls)
        if cs == 4294967295:
            break
        serve(cs)
        net_close(cs)
        served = served + 1
    return 0
"#;

const E2E_SRC: &str = r#"
def main() -> int:
    port = oa_env("MOCK_PORT")
    c = oa_client_at("sk-test", "http://127.0.0.1:" + port + "/v1")
    roles = vec_new()
    contents = vec_new()
    roles = vec_push_str(roles, "user")
    contents = vec_push_str(contents, "ping")
    r = oa_chat_create(c, "mock-gpt", roles, contents)
    print("E1 ok=" + str(oa_resp_ok(r)) + " text=[" + oa_chat_text(r) + "] model=" + oa_chat_model(r))
    print("E2 usage=" + str(oa_usage_in(r)) + "/" + str(oa_usage_out(r)) + "/" + str(oa_usage_total(r)))
    st = oa_chat_stream(c, "mock-gpt", roles, contents, 0.0, 0, 0)
    print("E3 serr=" + str(st.err) + " status=" + str(st.status))
    acc = ""
    guard = 0
    while not st.done and st.err == 0 and guard < 100:
        st = oa_stream_next(st)
        if st.res != "":
            acc = acc + oa_stream_text(st.res)
        guard = guard + 1
    oa_stream_close(st)
    print("E4 stream=[" + acc + "]")
    r2 = oa_request(c, "POST", "/error", "{}")
    print("E5 status=" + str(r2.status) + " msg=[" + oa_err_msg(r2) + "]")
    return 0
"#;

#[test]
fn openai_sdk_end_to_end_mock() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    // a free loopback port: grab an ephemeral listener, read its port, drop it
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind :0");
        l.local_addr().unwrap().port()
    };
    let mock = build(
        &format!("import * from \"{}\"\n{}", abs("web/sock_win.ax"), MOCK_SRC),
        "oa_mock",
        &["ws2_32"],
    );
    let client = build(
        &format!("import * from \"{}\"\n{}", abs("stdlib/openai/client"), E2E_SRC),
        "oa_e2e",
        &["winhttp"],
    );

    let mut child = Command::new(&mock)
        .env("MOCK_PORT", port.to_string())
        .env("MOCK_COUNT", "6")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn mock server");

    // wait until the mock's listener is accepting (it binds immediately
    // after startup; polling a real TCP connect avoids startup races)
    let mut up = false;
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            up = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let result = if up {
        let out = Command::new(&client)
            .env("MOCK_PORT", port.to_string())
            .output()
            .expect("failed to run e2e client");
        Some(out)
    } else {
        None
    };
    let _ = child.kill();
    let _ = child.wait();

    let out = result.unwrap_or_else(|| panic!("mock server never came up on port {port}"));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "e2e client exited {:?}\nstdout:\n{text}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    for marker in [
        "E1 ok=true text=[Hello from the mock] model=mock-gpt",
        "E2 usage=12/5/17",
        "E3 serr=0 status=200",
        "E4 stream=[Hello!]",
        "E5 status=401 msg=[openai 401: Incorrect API key provided: sk-test]",
    ] {
        assert!(text.contains(marker), "missing marker {marker:?}\nstdout:\n{text}");
    }
}
