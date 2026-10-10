#![cfg(windows)]

//! stdlib/anthropic SDK tests.
//!
//! Two layers:
//! - PURE: content blocks, the message list, the schema builder, request
//!   bodies, header assembly, pagination paths, the multipart prelude, the
//!   status->class mapping, response accessors, the named-event SSE filter,
//!   the JSON builder — one driver exe, every layer offline;
//! - END-TO-END: a full WinHTTP round trip against a MOCK Anthropic server
//!   written in Aoxn itself (web/sock_win.ax) — a real message creation, a
//!   real SSE stream consumed through an_messages_stream and reassembled by
//!   the accumulator (including a tool call whose arguments arrive as JSON
//!   fragments), a token count, a model list, a 401 and a 429-with-Retry-After.
//!   No external service, no credentials: the mock binds a loopback port and
//!   answers canned requests.
//!
//! Both drivers need `-l winhttp` (the externs are declared even where the
//! gated paths never run), the mock needs `-l ws2_32`.

//!
//! Windows-only by construction: the drivers link `-l winhttp` and the
//! mock links `-l ws2_32`, and there is no such library on Linux — the
//! transport they exercise is Windows-only anyway (docs/platform-support.md).
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
    let dir = std::env::temp_dir().join(format!("aoxn-anthropic-{}-{}", name, std::process::id()));
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
# find the first occurrence of a string in a string (-1 = absent)
def find_str(s: string, needle: string) -> int:
    n = len(needle)
    i = 0
    while i + n <= len(s):
        if str_sub(s, i, i + n) == needle:
            return i
        i = i + 1
    return -1

# the full SSE transcript of one streamed message, reused by the filter test
# and by the accumulator test below.
def stream_frames() -> string:
    nl = "\n"
    b = "event: message_start" + nl
    b = b + "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"mock-claude\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":11,\"output_tokens\":1}}}" + nl + nl
    b = b + "event: ping" + nl + "data: {\"type\":\"ping\"}" + nl + nl
    b = b + "event: content_block_start" + nl
    b = b + "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}" + nl + nl
    b = b + "event: content_block_delta" + nl
    b = b + "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello \"}}" + nl + nl
    b = b + "event: content_block_delta" + nl
    b = b + "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"from the mock\"}}" + nl + nl
    b = b + "event: content_block_stop" + nl
    b = b + "data: {\"type\":\"content_block_stop\",\"index\":0}" + nl + nl
    b = b + "event: content_block_start" + nl
    b = b + "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_9\",\"name\":\"get_weather\",\"input\":{}}}" + nl + nl
    b = b + "event: content_block_delta" + nl
    b = b + "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\"}}" + nl + nl
    b = b + "event: content_block_delta" + nl
    b = b + "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"Paris\\\"}\"}}" + nl + nl
    b = b + "event: content_block_stop" + nl
    b = b + "data: {\"type\":\"content_block_stop\",\"index\":1}" + nl + nl
    b = b + "event: message_delta" + nl
    b = b + "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":27}}" + nl + nl
    b = b + "event: message_stop" + nl
    b = b + "data: {\"type\":\"message_stop\"}" + nl + nl
    return b

def main() -> int:
    # ---- headers: the API REQUIRES x-api-key + anthropic-version ----
    c = an_client_new("sk-ant-test")
    h = an_headers_build(c, "{}", "application/json")
    print("PASS hdr-key " + str(find_str(h, "x-api-key: sk-ant-test") >= 0))
    print("PASS hdr-ver " + str(find_str(h, "anthropic-version: 2023-06-01") >= 0))
    print("PASS hdr-accept " + str(find_str(h, "Accept: application/json") >= 0))
    print("PASS hdr-ua " + str(find_str(h, "User-Agent: aoxn-anthropic/") >= 0))
    hs = an_headers_build(c, "{}", "text/event-stream")
    print("PASS hdr-sse " + str(find_str(hs, "Accept: text/event-stream") >= 0))
    print("PASS hdr-nobeta " + str(find_str(h, "anthropic-beta:") == -1))
    hb = an_headers_build(an_beta(c, "flag-a"), "{}", "application/json")
    print("PASS hdr-beta " + str(find_str(hb, "anthropic-beta: flag-a") >= 0))
    ca = an_client_new("")
    ca.auth_token = "tok-9"
    ht = an_headers_build(ca, "", "application/json")
    print("PASS hdr-auth " + str(find_str(ht, "Authorization: Bearer tok-9") >= 0) + str(find_str(ht, "x-api-key:") == -1) + str(find_str(ht, "Content-Type:") == -1))
    print("PASS client-ok " + str(an_client_ok(c)) + str(an_client_ok(ca)) + str(an_client_ok(an_client_new(""))))
    print("PASS base-url " + an_client_new("k").base_url)
    # betas: comma-joined, no duplicates
    cbeta = an_beta(an_beta(an_beta(an_client_new("k"), "a"), "b"), "a")
    print("PASS betas " + cbeta.betas + " has=" + str(an_betas_has(cbeta.betas, "b")) + str(an_betas_has(cbeta.betas, "z")))
    # ---- raw response-header parsing (pure; the transport hands these over) ----
    nl2 = net_crlf()
    raw = "HTTP/1.1 200 OK" + nl2 + "Content-Type: application/json" + nl2 + "request-id: req_abc" + nl2 + "Retry-After: 30" + nl2 + "X-Weird: has request-id: inside" + nl2 + nl2
    print("PASS rh-val " + net_header_value(raw, "request-id"))
    print("PASS rh-case " + net_header_value(raw, "Request-ID") + "|" + net_header_value(raw, "CONTENT-TYPE"))
    print("PASS rh-space " + "[" + net_header_value("A:   spaced  " + nl2, "a") + "]")
    print("PASS rh-miss " + "[" + net_header_value(raw, "x-absent") + "]")
    print("PASS rh-nonl " + "[" + net_header_value("A: tail-no-newline", "a") + "]|[" + net_header_value("nocolon", "a") + "]")
    print("PASS rh-empty " + "[" + net_header_value(raw, "") + "]")
    print("PASS rh-int " + str(net_header_int(raw, "retry-after", 0)) + " " + str(net_header_int(raw, "absent", -1)) + " " + str(net_header_int(raw, "content-type", -1)))
    print("PASS rh-ieq " + str(net_ieq("ABC", 0, 3, "abc")) + str(net_ieq("ABC", 0, 2, "abc")) + str(net_ieq("ABC", 0, 3, "abd")))
    print("PASS rh-emptyblock " + "[" + net_header_value("", "a") + "][" + str(len(raw)) + "]")
    # ---- content blocks ----
    print("PASS blk-text " + an_block_text("Say \"hi\"\nnow"))
    print("PASS blk-text-cc " + an_block_text_cached("t", "1h"))
    print("PASS blk-text-cc5 " + an_block_text_cached("t", "5m"))
    print("PASS cc-none [" + an_cache_control("") + "]")
    print("PASS blk-img-b64 " + an_block_image_b64("image/png", "aGk="))
    print("PASS blk-img-url " + an_block_image_url("https://e.com/a.png"))
    print("PASS blk-img-file " + an_block_image_file("file_1"))
    print("PASS blk-doc " + an_block_document("application/pdf", "JVBER"))
    print("PASS blk-doc-text " + an_block_document("text/plain", "plain words"))
    print("PASS blk-doc-titled " + an_block_document_titled("text/plain", "d", "T", "ctx"))
    print("PASS blk-think " + an_block_thinking("hmm", "sig-1"))
    print("PASS blk-redacted " + an_block_redacted_thinking("enc"))
    print("PASS blk-tool " + an_block_tool_use("toolu_1", "get_weather", "{\"city\":\"Paris\"}"))
    print("PASS blk-tool-empty " + an_block_tool_use("toolu_2", "ping", "{}"))
    print("PASS blk-tool-cc " + an_block_tool_use_cached("t", "n", "{}", "1h"))
    print("PASS blk-result " + an_block_tool_result("toolu_1", "18C"))
    print("PASS blk-result-err " + an_block_tool_result_error("toolu_1", "boom"))
    rblocks = vec_new()
    rblocks = vec_push_str(rblocks, an_block_text("see"))
    rblocks = vec_push_str(rblocks, an_block_image_url("https://e.com/b.png"))
    print("PASS blk-result-blocks " + an_block_tool_result_blocks("toolu_1", rblocks))
    print("PASS blk-server " + an_block_server_tool_use("srv_1", "web_search", "{\"q\":\"x\"}"))
    wsr = vec_new()
    wsr = vec_push_str(wsr, "{\"type\":\"web_search_result\",\"encrypted_content\":\"e\",\"title\":\"T\",\"url\":\"https://e.com\",\"page_age\":null}")
    print("PASS blk-websearch " + an_block_web_search_result("srv_1", wsr))
    print("PASS blocks-empty " + an_blocks_json(vec_new()))
    # ---- the message list ----
    m = an_msgs_new()
    m = an_msgs_push_text(m, "user", "hello")
    bl = vec_new()
    bl = vec_push_str(bl, an_block_text("look"))
    bl = vec_push_str(bl, an_block_image_url("https://e.com/a.png"))
    m = an_msgs_push_blocks(m, "user", bl)
    m = an_msgs_push_blocks(m, "assistant", vec_push_str(vec_new(), an_block_tool_use("toolu_1", "w", "{}")))
    m = an_msgs_push_blocks(m, "user", vec_push_str(vec_new(), an_block_tool_result("toolu_1", "ok")))
    print("PASS msgs-len " + str(an_msgs_len(m)))
    print("PASS msgs " + an_msgs_json(m))
    print("PASS msgs-empty " + an_msgs_json(an_msgs_new()))
    # a response with NO text block is normal (a pure tool call): the
    # joined text must be "" -- the v0.50.0 sink rewrite of an_msg_text
    # wrote its final NUL through a NULL buffer here and died with
    # 0xC0000005, so this pin is the regression
    nt = "{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"c\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"w\",\"input\":{}}],\"stop_reason\":\"tool_use\",\"stop_sequence\":null,\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}"
    ntp = j_parse(nt)
    nr = AnResp(status=200, dom=ntp.dom, pok=1, pmsg="", raw=nt, request_id="", terr=0, tmsg="", retries=0)
    print("PASS msg-text-empty [" + an_msg_text(nr) + "][" + an_msg_thinking(nr) + "]")
    mx = "{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"c\",\"content\":[{\"type\":\"text\",\"text\":\"a\"},{\"type\":\"text\",\"text\":\"b\"}],\"stop_reason\":\"end_turn\",\"stop_sequence\":null,\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}"
    mxp = j_parse(mx)
    mr = AnResp(status=200, dom=mxp.dom, pok=1, pmsg="", raw=mx, request_id="", terr=0, tmsg="", retries=0)
    print("PASS msg-text-two [" + an_msg_text(mr) + "]")
    # ---- schema builder: property NAMES must survive ----
    sc = an_schema_new()
    sc = an_schema_str(sc, "city", "City name")
    sc = an_schema_prop(sc, "unit", "string", "c or f", "[\"c\",\"f\"]")
    sc = an_schema_int(sc, "days", "forecast days")
    sc = an_schema_arr(sc, "tags", "labels", "{\"type\":\"string\"}")
    sc = an_schema_raw(sc, "meta", "{\"type\":\"object\"}")
    sc = an_schema_required(sc, an_str_vec2("city", "days"))
    print("PASS schema " + an_schema_json(sc))
    print("PASS schema-empty " + an_schema_empty())
    print("PASS tool " + an_tool("get_weather", "Current \"weather\".", an_schema_json(sc)))
    print("PASS tool-cc " + an_tool_cached("t", "d", an_schema_empty(), "1h"))
    print("PASS server-tool " + an_server_tool("web_search_20250305", "web_search", 5))
    print("PASS server-tool0 " + an_tool_web_search(0))
    print("PASS server-fetch " + an_tool_web_fetch(3))
    print("PASS tc-auto " + an_tool_choice_auto(False) + " " + an_tool_choice_auto(True))
    print("PASS tc-any " + an_tool_choice_any(False))
    print("PASS tc-tool " + an_tool_choice_tool("get_weather", True))
    print("PASS tc-none " + an_tool_choice_none())
    print("PASS think-on " + an_thinking_enabled(2048, "summarized"))
    print("PASS think-adapt " + an_thinking_adaptive(""))
    print("PASS think-off " + an_thinking_disabled())
    # ---- request bodies ----
    om = an_msgs_push_text(an_msgs_new(), "user", "hi")
    o = an_opts_new()
    print("PASS body-min " + an_body_messages("claude-x", om, 64, o, False))
    print("PASS body-stream " + an_body_messages("claude-x", om, 64, o, True))
    o2 = an_opts_new()
    o2.system = "be terse"
    o2.tools = vec_push_str(vec_new(), an_tool("t", "d", an_schema_empty()))
    o2.tool_choice = an_tool_choice_auto(False)
    o2.thinking = an_thinking_enabled(1024, "")
    o2.stop_sequences = an_str_vec2("STOP", "END")
    o2.metadata_user_id = "u-1"
    o2.service_tier = "auto"
    o2.inference_geo = "us"
    print("PASS body-full " + an_body_messages("claude-x", om, 512, o2, False))
    print("PASS body-ctok " + an_body_count_tokens("claude-x", om, an_opts_new()))
    print("PASS body-ctok-full " + an_body_count_tokens("claude-x", om, o2))
    sblocks = vec_new()
    o3 = an_opts_new()
    o3.system_blocks = sblocks
    o3.system_blocks = vec_push_str(o3.system_blocks, an_block_text("sys block"))
    print("PASS body-sysblocks " + an_body_messages("claude-x", om, 8, o3, False))
    br = vec_new()
    br = vec_push_str(br, an_batch_request("req-1", om, 100, an_opts_new()))
    br = vec_push_str(br, an_batch_request("req-2", om, 200, o2))
    print("PASS body-batch " + an_body_batch(br))
    print("PASS batch-req " + an_batch_request("req-1", om, 100, an_opts_new()))
    # ---- pagination paths ----
    print("PASS page-none " + an_page_path("/v1/models", 0, "", ""))
    print("PASS page-limit " + an_page_path("/v1/models", 20, "", ""))
    print("PASS page-after " + an_page_path("/v1/models", 20, "a1", ""))
    print("PASS page-before " + an_page_path("/v1/models", 5, "a1", "b1"))
    print("PASS page-idonly " + an_page_path("/v1/models", 0, "a1", ""))
    # a query string must SURVIVE url splitting (pagination depends on it)
    uq = net_url_split("https://api.anthropic.com/v1/messages/batches?limit=20&after_id=a%201")
    print("PASS url-query " + uq.host + " " + str(uq.port) + " " + uq.path)
    up = net_url_split("https://host:8443/a/b?q=1")
    print("PASS url-q2 " + up.host + " " + str(up.port) + " " + up.path)
    # ---- multipart ----
    mp = an_multipart_prelude("BOUND", "a.png", "image/png")
    print("PASS mp " + str(find_str(mp, "--BOUND") == 0) + str(find_str(mp, "filename=\"a.png\"") >= 0) + str(find_str(mp, "Content-Type: image/png") >= 0))
    hm = an_headers_multipart(c, "multipart/form-data; boundary=BOUND")
    print("PASS mp-hdr " + str(find_str(hm, "Content-Type: multipart/form-data; boundary=BOUND") >= 0))
    # ---- status -> class ----
    print("PASS class " + an_status_class(400) + " " + an_status_class(401) + " " + an_status_class(403))
    print("PASS class2 " + an_status_class(404) + " " + an_status_class(409) + " " + an_status_class(413))
    print("PASS class3 " + an_status_class(422) + " " + an_status_class(429) + " " + an_status_class(529))
    print("PASS class4 " + an_status_class(500) + " " + an_status_class(503) + " " + an_status_class(0) + " " + an_status_class(418))
    # ---- the JSON builder itself ----
    d = jdom_new()
    d = jb_obj(d)
    oo = d.last
    d = jb_set_str(d, oo, "s", "a\"b\nc")
    d = jb_set_int(d, oo, "i", -7)
    d = jb_set_bool(d, oo, "b", True)
    d = jb_set_num(d, oo, "f", 0.25)
    d = jb_set_null(d, oo, "n")
    d = jb_set_raw(d, oo, "raw", "{\"x\":[1,2]}")
    print("PASS jb " + j_dumps(d, oo))
    d = jb_arr(d)
    ar = d.last
    i = 0
    while i < 12:
        d = jb_push_raw(d, ar, "{\"k\":" + str(i) + "}")
        i = i + 1
    print("PASS jb-arr " + str(j_len(d, ar)) + " " + str(j_get_int(d, j_arr_get(d, ar, 11), "k", -1)))
    d = jb_set(d, oo, "arr", ar)
    print("PASS jb-grow " + str(j_len(d, oo)))
    d2 = jb_obj(jdom_new())
    o2i = d2.last
    d2 = jb_set_raw(d2, o2i, "bad", "{oops")
    print("PASS jb-badraw " + j_dumps(d2, o2i))
    # ---- the SSE filter, named events ----
    s = net_sse_new()
    fr = stream_frames()
    s = net_sse_feed(s, as_ptr(fr), len(fr))
    types = ""
    names = ""
    guard = 0
    while guard < 40:
        s = net_sse_next(s)
        if s.res == "":
            break
        types = types + an_stream_type(s.res) + ","
        names = names + s.event + ","
        guard = guard + 1
    print("PASS sse-types " + types)
    print("PASS sse-names " + names)
    # ---- event helpers ----
    tx = "{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}"
    ij = "{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"a\\\":\"}}"
    md = "{\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":9}}"
    th = "{\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hm\"}}"
    er = "{\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"busy\"}}"
    cb = "{\"type\":\"content_block_start\",\"index\":3,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}"
    print("PASS ev-text " + an_stream_text(tx) + "|" + an_stream_partial_json(tx))
    print("PASS ev-ijson " + an_stream_partial_json(ij) + "|" + an_stream_text(ij))
    print("PASS ev-think " + an_stream_thinking(th))
    print("PASS ev-idx " + str(an_stream_index(tx)) + str(an_stream_index(cb)) + str(an_stream_index("bad")))
    print("PASS ev-msgdelta " + an_stream_stop_reason(md) + " " + str(an_stream_output_tokens(md)) + " " + str(an_stream_output_tokens(tx)))
    pingf = "{\"type\":\"ping\"}"
    print("PASS ev-err " + str(an_stream_is_error(er)) + "|" + an_stream_error_msg(er) + "|" + str(an_stream_is_ping(pingf)))
    print("PASS ev-stop " + str(an_stream_is_stop("{\"type\":\"message_stop\"}")) + str(an_stream_is_stop(tx)))
    print("PASS ev-bad " + "[" + an_stream_type("{oops") + "][" + an_stream_text("{oops") + "]" + str(an_stream_is_error("{oops")))
    # ---- the accumulator over the canned transcript ----
    a = an_acc_new()
    s2 = net_sse_new()
    s2 = net_sse_feed(s2, as_ptr(fr), len(fr))
    guard = 0
    while guard < 40:
        s2 = net_sse_next(s2)
        if s2.res == "":
            break
        a = an_acc_feed(a, s2.res)
        guard = guard + 1
    r = an_acc_resp(a)
    print("PASS acc-ok " + str(an_resp_ok(r)) + " id=" + an_msg_id(r) + " model=" + an_msg_model(r) + " role=" + an_msg_role(r))
    print("PASS acc-text [" + an_msg_text(r) + "]")
    print("PASS acc-blocks " + str(an_block_count(r)))
    print("PASS acc-types " + an_block_type(r, 0) + "," + an_block_type(r, 1))
    print("PASS acc-stop " + an_msg_stop_reason(r))
    print("PASS acc-usage " + str(an_usage_in(r)) + "/" + str(an_usage_out(r)))
    ti = an_find_tool_use(r)
    print("PASS acc-tool " + str(ti) + " " + an_block_tool_name(r, ti) + " " + an_block_tool_id(r, ti))
    print("PASS acc-toolinput " + an_block_tool_input(r, ti) + " city=" + an_block_tool_input_str(r, ti, "city"))
    print("PASS acc-events " + str(a.events) + " done=" + str(a.done))
    print("PASS acc-find " + str(an_find_block(r, "text")) + str(an_find_block(r, "thinking")) + str(an_find_tool_use(r)))
    # a second accumulator must not see the first one's state
    a0 = an_acc_new()
    print("PASS acc-fresh " + str(an_block_count(an_acc_resp(a0))) + " [" + an_msg_text(an_acc_resp(a0)) + "]")
    # ---- response accessors on canned bodies ----
    mb = "{\"id\":\"msg_2\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-x\",\"content\":[{\"type\":\"text\",\"text\":\"Hi\"},{\"type\":\"thinking\",\"thinking\":\"t\",\"signature\":\"s\"},{\"type\":\"tool_use\",\"id\":\"toolu_2\",\"name\":\"w\",\"input\":{\"city\":\"Rome\",\"days\":3}}],\"stop_reason\":\"tool_use\",\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":20,\"cache_read_input_tokens\":4,\"cache_creation_input_tokens\":2,\"output_tokens_details\":{\"thinking_tokens\":5}}}"
    p2 = j_parse(mb)
    r2 = AnResp(status=200, dom=p2.dom, pok=1, pmsg="", raw=mb, request_id="req_x", terr=0, tmsg="", retries=0)
    print("PASS resp-text [" + an_msg_text(r2) + "] thinking=[" + an_msg_thinking(r2) + "]")
    print("PASS resp-blocks " + str(an_block_count(r2)) + " " + an_block_type(r2, 1) + " " + an_block_signature_at(r2, 1))
    t2 = an_find_tool_use(r2)
    print("PASS resp-input " + an_block_tool_input(r2, t2) + " " + an_block_tool_input_str(r2, t2, "city") + " " + str(an_block_tool_input_int(r2, t2, "days")))
    print("PASS resp-usage " + an_usage_line(r2) + " cache=" + str(an_usage_cache_read(r2)) + "/" + str(an_usage_cache_create(r2)) + " think=" + str(an_usage_thinking(r2)))
    print("PASS resp-ok " + str(an_resp_ok(r2)) + "|" + an_err_msg(r2) + "|" + an_msg_type(r2))
    eb = "{\"type\":\"error\",\"error\":{\"type\":\"authentication_error\",\"message\":\"invalid x-api-key\"}}"
    q = j_parse(eb)
    r3 = AnResp(status=401, dom=q.dom, pok=1, pmsg="", raw=eb, request_id="req_y", terr=0, tmsg="", retries=0)
    print("PASS resp-401 " + str(an_resp_ok(r3)) + "|" + an_err_msg(r3) + "|" + an_err_type(r3))
    r4 = AnResp(status=0, dom=jdom_new(), pok=0, pmsg="x", raw="", request_id="", terr=1, tmsg="timeout", retries=2)
    print("PASS resp-terr " + str(an_resp_ok(r4)) + "|" + an_err_msg(r4))
    r5 = AnResp(status=200, dom=jdom_new(), pok=0, pmsg="bad", raw="", request_id="", terr=0, tmsg="", retries=0)
    print("PASS resp-badjson " + an_err_msg(r5))
    r6 = AnResp(status=503, dom=jdom_new(), pok=1, pmsg="", raw="", request_id="", terr=0, tmsg="", retries=1)
    print("PASS resp-503 " + an_err_msg(r6))
    # accessors must survive a garbage/empty body
    r7 = AnResp(status=200, dom=jdom_new(), pok=1, pmsg="", raw="", request_id="", terr=0, tmsg="", retries=0)
    print("PASS resp-empty " + "[" + an_msg_text(r7) + "]" + str(an_block_count(r7)) + " " + an_usage_line(r7) + " " + str(an_find_tool_use(r7)))
    # ---- list + batch + file accessors ----
    lb = "{\"data\":[{\"id\":\"claude-a\"},{\"id\":\"claude-b\"}],\"has_more\":true,\"first_id\":\"claude-a\",\"last_id\":\"claude-b\"}"
    v = j_parse(lb)
    r8 = AnResp(status=200, dom=v.dom, pok=1, pmsg="", raw=lb, request_id="", terr=0, tmsg="", retries=0)
    print("PASS list " + str(an_data_len(r8)) + " " + an_data_id(r8, 0) + " " + an_data_id(r8, 1) + " more=" + str(an_has_more(r8)) + " " + an_first_id(r8) + " " + an_last_id(r8))
    bt = "{\"id\":\"msgbatch_1\",\"processing_status\":\"in_progress\",\"results_url\":\"https://api.anthropic.com/v1/messages/batches/msgbatch_1/results\"}"
    w = j_parse(bt)
    r9 = AnResp(status=200, dom=w.dom, pok=1, pmsg="", raw=bt, request_id="", terr=0, tmsg="", retries=0)
    print("PASS batch " + an_batch_status(r9) + " " + an_batch_results_url(r9))
    ct = "{\"input_tokens\":2095}"
    y = j_parse(ct)
    r10 = AnResp(status=200, dom=y.dom, pok=1, pmsg="", raw=ct, request_id="", terr=0, tmsg="", retries=0)
    print("PASS ctok " + str(an_count_tokens_value(r10)))
    r11 = AnResp(status=400, dom=jdom_new(), pok=1, pmsg="", raw="", request_id="", terr=0, tmsg="", retries=0)
    print("PASS ctok-bad " + str(an_count_tokens_value(r11)))
    fb = "{\"id\":\"file_1\",\"filename\":\"a.png\",\"mime_type\":\"image/png\",\"size_bytes\":1234}"
    fb2 = j_parse(fb)
    r12 = AnResp(status=200, dom=fb2.dom, pok=1, pmsg="", raw=fb, request_id="", terr=0, tmsg="", retries=0)
    print("PASS file " + an_file_id(r12) + " " + an_file_name(r12) + " " + an_file_media_type(r12) + " " + str(an_file_size(r12)))
    # ---- tool_result accessors ----
    tb = "{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_3\",\"content\":\"done\",\"is_error\":true}]}"
    tbb = j_parse(tb)
    r13 = AnResp(status=200, dom=tbb.dom, pok=1, pmsg="", raw=tb, request_id="", terr=0, tmsg="", retries=0)
    print("PASS result " + an_block_result_text(r13, 0) + " err=" + str(an_block_result_is_error(r13, 0)))
    return 0
"#;

#[test]
fn anthropic_sdk_pure_layers() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    let src = format!("import * from \"{}\"\n{}", abs("stdlib/anthropic/client"), PURE_SRC);
    let exe = build(&src, "an_pure", &["winhttp"]);
    let out = Command::new(&exe).output().expect("failed to run pure driver");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "pure driver exited {:?}\nstdout:\n{text}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    let mut missing: Vec<String> = Vec::new();
    for marker in [
        // headers
        "hdr-key true", "hdr-ver true", "hdr-accept true", "hdr-ua true",
        "hdr-sse true", "hdr-nobeta true", "hdr-beta true", "hdr-auth truetruetrue",
        // an empty key AND an empty token is the only not-ok client
        "client-ok truetruefalse", "base-url https://api.anthropic.com",
        "betas a,b has=truefalse",
        // raw response-header parsing
        "rh-val req_abc",
        "rh-case req_abc|application/json",
        "rh-space [spaced  ]",
        "rh-miss []",
        "rh-nonl [tail-no-newline]|[]",
        "rh-empty []",
        "rh-int 30 -1 -1",
        "rh-ieq truefalsefalse",
        "rh-emptyblock []",
        // blocks
        "blk-text {\"type\":\"text\",\"text\":\"Say \\\"hi\\\"\\nnow\"}",
        "blk-text-cc {\"type\":\"text\",\"text\":\"t\",\"cache_control\":{\"type\":\"ephemeral\",\"ttl\":\"1h\"}}",
        "blk-text-cc5 {\"type\":\"text\",\"text\":\"t\",\"cache_control\":{\"type\":\"ephemeral\"}}",
        "cc-none []",
        "blk-img-b64 {\"type\":\"image\",\"source\":{\"type\":\"base64\",\"media_type\":\"image/png\",\"data\":\"aGk=\"}}",
        "blk-img-url {\"type\":\"image\",\"source\":{\"type\":\"url\",\"url\":\"https://e.com/a.png\"}}",
        "blk-img-file {\"type\":\"image\",\"source\":{\"type\":\"file\",\"file_id\":\"file_1\"}}",
        "blk-doc {\"type\":\"document\",\"source\":{\"type\":\"base64\",\"media_type\":\"application/pdf\",\"data\":\"JVBER\"}}",
        "blk-doc-text {\"type\":\"document\",\"source\":{\"type\":\"text\",\"media_type\":\"text/plain\",\"data\":\"plain words\"}}",
        "blk-doc-titled {\"type\":\"document\",\"source\":{\"type\":\"text\",\"media_type\":\"text/plain\",\"data\":\"d\"},\"title\":\"T\",\"context\":\"ctx\"}",
        "blk-think {\"type\":\"thinking\",\"thinking\":\"hmm\",\"signature\":\"sig-1\"}",
        "blk-redacted {\"type\":\"redacted_thinking\",\"data\":\"enc\"}",
        "blk-tool {\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"get_weather\",\"input\":{\"city\":\"Paris\"}}",
        "blk-tool-empty {\"type\":\"tool_use\",\"id\":\"toolu_2\",\"name\":\"ping\",\"input\":{}}",
        "blk-tool-cc {\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"n\",\"input\":{},\"cache_control\":{\"type\":\"ephemeral\",\"ttl\":\"1h\"}}",
        "blk-result {\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"18C\"}",
        "blk-result-err {\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"boom\",\"is_error\":true}",
        "blk-result-blocks {\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":[{\"type\":\"text\",\"text\":\"see\"},{\"type\":\"image\",\"source\":{\"type\":\"url\",\"url\":\"https://e.com/b.png\"}}]}",
        "blk-server {\"type\":\"server_tool_use\",\"id\":\"srv_1\",\"name\":\"web_search\",\"input\":{\"q\":\"x\"}}",
        "blocks-empty []",
        // message list
        "msgs-len 4",
        "msgs [{\"role\":\"user\",\"content\":\"hello\"},{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"look\"},{\"type\":\"image\",\"source\":{\"type\":\"url\",\"url\":\"https://e.com/a.png\"}}]},{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"w\",\"input\":{}}]},{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"ok\"}]}]",
        "msgs-empty []",
        "msg-text-empty [][]",
        "msg-text-two [a\nb]",
        // tools
        "schema {\"type\":\"object\",\"properties\":{\"city\":{\"type\":\"string\",\"description\":\"City name\"},\"unit\":{\"type\":\"string\",\"description\":\"c or f\",\"enum\":[\"c\",\"f\"]},\"days\":{\"type\":\"integer\",\"description\":\"forecast days\"},\"tags\":{\"type\":\"array\",\"description\":\"labels\",\"items\":{\"type\":\"string\"}},\"meta\":{\"type\":\"object\"}},\"required\":[\"city\",\"days\"]}",
        "schema-empty {\"type\":\"object\",\"properties\":{}}",
        "server-tool {\"type\":\"web_search_20250305\",\"name\":\"web_search\",\"max_uses\":5}",
        "server-tool0 {\"type\":\"web_search_20250305\",\"name\":\"web_search\"}",
        "server-fetch {\"type\":\"web_fetch_20250910\",\"name\":\"web_fetch\",\"max_uses\":3}",
        "tc-auto {\"type\":\"auto\"} {\"type\":\"auto\",\"disable_parallel_tool_use\":true}",
        "tc-any {\"type\":\"any\"}",
        "tc-tool {\"type\":\"tool\",\"name\":\"get_weather\",\"disable_parallel_tool_use\":true}",
        "tc-none {\"type\":\"none\"}",
        "think-on {\"type\":\"enabled\",\"budget_tokens\":2048,\"display\":\"summarized\"}",
        "think-adapt {\"type\":\"adaptive\"}",
        "think-off {\"type\":\"disabled\"}",
        // bodies
        "body-min {\"model\":\"claude-x\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":64}",
        "body-stream {\"model\":\"claude-x\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":64,\"stream\":true}",
        "body-sysblocks {\"model\":\"claude-x\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":8,\"system\":[{\"type\":\"text\",\"text\":\"sys block\"}]}",
        "body-ctok {\"model\":\"claude-x\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}",
        // pagination + url
        "page-none /v1/models",
        "page-limit /v1/models?limit=20",
        "page-after /v1/models?limit=20&after_id=a1",
        "page-before /v1/models?limit=5&before_id=b1",
        "page-idonly /v1/models?after_id=a1",
        "url-query api.anthropic.com 443 /v1/messages/batches?limit=20&after_id=a%201",
        "url-q2 host 8443 /a/b?q=1",
        // multipart
        "mp truetruetrue",
        "mp-hdr true",
        // status classes
        "class bad_request_error authentication_error permission_error",
        "class2 not_found_error conflict_error request_too_large",
        "class3 unprocessable_entity_error rate_limit_error overloaded_error",
        "class4 internal_server_error internal_server_error connection_error api_error",
        // json builder
        "jb {\"s\":\"a\\\"b\\nc\",\"i\":-7,\"b\":true,\"f\":0.25,\"n\":null,\"raw\":{\"x\":[1,2]}}",
        "jb-arr 12 11",
        "jb-badraw {}",
        // sse + events
        "sse-types message_start,ping,content_block_start,content_block_delta,content_block_delta,content_block_stop,content_block_start,content_block_delta,content_block_delta,content_block_stop,message_delta,message_stop,",
        "sse-names message_start,ping,content_block_start,content_block_delta,content_block_delta,content_block_stop,content_block_start,content_block_delta,content_block_delta,content_block_stop,message_delta,message_stop,",
        "ev-text hi|",
        "ev-ijson {\"a\":|",
        "ev-think hm",
        "ev-idx 03-1",
        "ev-msgdelta end_turn 9 0",
        "ev-err true|overloaded_error: busy|true",
        "ev-stop truefalse",
        "ev-bad [][]false",
        // accumulator
        "acc-ok true id=msg_1 model=mock-claude role=assistant",
        "acc-text [Hello from the mock]",
        "acc-blocks 2",
        "acc-types text,tool_use",
        "acc-stop tool_use",
        "acc-usage 11/27",
        "acc-tool 1 get_weather toolu_9",
        "acc-toolinput {\"city\":\"Paris\"} city=Paris",
        "acc-events 12 done=true",
        "acc-find 0-11",
        "acc-fresh 0 []",
        // accessors
        "resp-text [Hi] thinking=[t]",
        "resp-blocks 3 thinking s",
        "resp-input {\"city\":\"Rome\",\"days\":3} Rome 3",
        "resp-usage tokens 10 in / 20 out cache=4/2 think=5",
        "resp-ok true||message",
        "resp-401 false|anthropic 401 authentication_error: invalid x-api-key|authentication_error",
        "resp-terr false|timeout",
        "resp-badjson bad json in response: bad",
        "resp-503 http 503",
        "resp-empty []0 tokens 0 in / 0 out -1",
        "list 2 claude-a claude-b more=true claude-a claude-b",
        "batch in_progress https://api.anthropic.com/v1/messages/batches/msgbatch_1/results",
        "ctok 2095",
        "ctok-bad -1",
        "file file_1 a.png image/png 1234",
        "result done err=true",
    ] {
        // collect every mismatch rather than stopping at the first: a
        // hand-written expectation table drifts one line at a time, and
        // one run per drifted line is a slow way to find them
        if !text.contains(marker) {
            missing.push(marker.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "{} marker(s) missing from the driver output: {missing:#?}\nstdout:\n{text}",
        missing.len()
    );
    // the long bodies, checked here so the failure names itself
    if !text.contains("body-full {\"model\":\"claude-x\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":512,\"system\":\"be terse\",\"stop_sequences\":[\"STOP\",\"END\"],\"tools\":[{\"name\":\"t\",\"description\":\"d\",\"input_schema\":{\"type\":\"object\",\"properties\":{}}}],\"tool_choice\":{\"type\":\"auto\"},\"thinking\":{\"type\":\"enabled\",\"budget_tokens\":1024},\"metadata\":{\"user_id\":\"u-1\"},\"service_tier\":\"auto\",\"inference_geo\":\"us\"}") {
        missing.push("body-full".to_string());
    }
    if !text.contains("body-batch {\"requests\":[{\"custom_id\":\"req-1\",\"params\":{\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":100}},{\"custom_id\":\"req-2\",\"params\":{\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":200,\"system\":\"be terse\",\"stop_sequences\":[\"STOP\",\"END\"],\"tools\":[{\"name\":\"t\",\"description\":\"d\",\"input_schema\":{\"type\":\"object\",\"properties\":{}}}],\"tool_choice\":{\"type\":\"auto\"},\"thinking\":{\"type\":\"enabled\",\"budget_tokens\":1024},\"metadata\":{\"user_id\":\"u-1\"},\"service_tier\":\"auto\",\"inference_geo\":\"us\"}}]}") {
        missing.push("body-batch".to_string());
    }
    if !text.contains("body-ctok-full {\"model\":\"claude-x\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"system\":\"be terse\",\"stop_sequences\":[\"STOP\",\"END\"],\"tools\":[{\"name\":\"t\",\"description\":\"d\",\"input_schema\":{\"type\":\"object\",\"properties\":{}}}],\"tool_choice\":{\"type\":\"auto\"},\"thinking\":{\"type\":\"enabled\",\"budget_tokens\":1024},\"metadata\":{\"user_id\":\"u-1\"},\"service_tier\":\"auto\",\"inference_geo\":\"us\"}") {
        missing.push("body-ctok-full".to_string());
    }
    for extra in [
        "blk-websearch {\"type\":\"web_search_tool_result\",\"tool_use_id\":\"srv_1\",\"content\":[{\"type\":\"web_search_result\",\"encrypted_content\":\"e\",\"title\":\"T\",\"url\":\"https://e.com\",\"page_age\":null}]}",
        "batch-req {\"custom_id\":\"req-1\",\"params\":{\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":100}}",
        "body-ctok {\"model\":\"claude-x\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}",
        "tool {\"name\":\"get_weather\",\"description\":\"Current \\\"weather\\\".\",\"input_schema\":{\"type\":\"object\"",
        "tool-cc {\"name\":\"t\"",
        "jb-grow 7",
    ] {
        if !text.contains(extra) {
            missing.push(extra.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "markers missing: {missing:#?}\nstdout:\n{text}"
    );
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

def respond(cs: int, status: string, ctype: string, body: string, extra: string) -> int:
    nl = crlf()
    h = status + nl + "Content-Type: " + ctype + nl + "Content-Length: " + str(len(body)) + nl + "request-id: req_mock_7" + nl + extra + "Connection: close" + nl + nl
    net_send_all(cs, as_ptr(h), len(h))
    if len(body) > 0:
        return net_send_all(cs, as_ptr(body), len(body))
    return 0

def message_body() -> string:
    return "{\"id\":\"msg_mock_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"mock-claude\",\"content\":[{\"type\":\"text\",\"text\":\"Hello from the mock\"},{\"type\":\"tool_use\",\"id\":\"toolu_mock\",\"name\":\"get_weather\",\"input\":{\"city\":\"Paris\"}}],\"stop_reason\":\"tool_use\",\"stop_sequence\":null,\"usage\":{\"input_tokens\":12,\"output_tokens\":9,\"cache_read_input_tokens\":3,\"cache_creation_input_tokens\":1}}"

def stream_body() -> string:
    nl = "\n"
    b = "event: message_start" + nl
    b = b + "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_mock_2\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"mock-claude\",\"content\":[],\"usage\":{\"input_tokens\":14,\"output_tokens\":1}}}" + nl + nl
    b = b + "event: content_block_start" + nl
    b = b + "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}" + nl + nl
    b = b + "event: content_block_delta" + nl
    b = b + "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Streamed \"}}" + nl + nl
    b = b + "event: content_block_delta" + nl
    b = b + "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"reply\"}}" + nl + nl
    b = b + "event: content_block_stop" + nl
    b = b + "data: {\"type\":\"content_block_stop\",\"index\":0}" + nl + nl
    b = b + "event: content_block_start" + nl
    b = b + "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_s\",\"name\":\"lookup\",\"input\":{}}}" + nl + nl
    b = b + "event: content_block_delta" + nl
    b = b + "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"q\\\":\\\"a\"}}" + nl + nl
    b = b + "event: content_block_delta" + nl
    b = b + "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"bc\\\"}\"}}" + nl + nl
    b = b + "event: content_block_stop" + nl
    b = b + "data: {\"type\":\"content_block_stop\",\"index\":1}" + nl + nl
    b = b + "event: message_delta" + nl
    b = b + "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":31}}" + nl + nl
    b = b + "event: message_stop" + nl
    b = b + "data: {\"type\":\"message_stop\"}" + nl + nl
    return b

def error_body() -> string:
    return "{\"type\":\"error\",\"error\":{\"type\":\"authentication_error\",\"message\":\"invalid x-api-key\"}}"

# serve one request; returns 1 when the request carried the required auth
# headers (so the test can assert the SDK sent them)
def serve(cs: int) -> int:
    buf = malloc(65536)
    total = 0
    he = -1
    while total < 64000:
        got = net_recv(cs, buf + total, 8192)
        if got <= 0:
            break
        total = total + got
        he = hdr_end(buf, total)
        if he >= 0:
            if total - (he + 4) >= content_length(buf, he):
                break
    if he < 0:
        respond(cs, "HTTP/1.1 400 Bad Request", "application/json", "{}", "")
        return 0
    # the path (with any query string) is the second token of the request line
    sp1 = find_bytes(buf, he, " ", 0)
    sp2 = find_bytes(buf, he, " ", sp1 + 1)
    plen = sp2 - sp1 - 1
    pp = malloc(plen + 1)
    memcpy(pp, buf + sp1 + 1, plen)
    store_u8(pp, plen, 0)
    path = as_string(pp)
    authed = 1
    if find_bytes(buf, he, "x-api-key: sk-ant-test", 0) < 0:
        authed = 0
    if find_bytes(buf, he, "anthropic-version: 2023-06-01", 0) < 0:
        authed = 0
    if authed == 0:
        respond(cs, "HTTP/1.1 401 Unauthorized", "application/json", error_body(), "")
        return 0
    blen = total - he - 4
    if path == "/v1/messages" and find_bytes(buf + he + 4, blen, "\"stream\":true", 0) >= 0:
        respond(cs, "HTTP/1.1 200 OK", "text/event-stream", stream_body(), "")
        return 1
    if path == "/v1/messages":
        respond(cs, "HTTP/1.1 200 OK", "application/json", message_body(), "")
        return 1
    if path == "/v1/messages/count_tokens":
        respond(cs, "HTTP/1.1 200 OK", "application/json", "{\"input_tokens\":2095}", "")
        return 1
    if path == "/v1/models?limit=20":
        respond(cs, "HTTP/1.1 200 OK", "application/json", "{\"data\":[{\"id\":\"claude-mock\"}],\"has_more\":false,\"first_id\":\"claude-mock\",\"last_id\":\"claude-mock\"}", "")
        return 1
    if path == "/v1/models":
        respond(cs, "HTTP/1.1 200 OK", "application/json", "{\"data\":[{\"id\":\"claude-mock\"}],\"has_more\":false}", "")
        return 1
    if path == "/v1/boom":
        respond(cs, "HTTP/1.1 429 Too Many Requests", "application/json", "{\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\",\"message\":\"slow down\"}}", "Retry-After: 1" + crlf())
        return 1
    respond(cs, "HTTP/1.1 404 Not Found", "application/json", "{\"type\":\"error\",\"error\":{\"type\":\"not_found_error\",\"message\":\"no such route\"}}", "")
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
    port = an_env("MOCK_PORT")
    c = an_client_at("sk-ant-test", "http://127.0.0.1:" + port)
    # a plain message creation
    m = an_msgs_new()
    m = an_msgs_push_text(m, "user", "ping")
    opts = an_opts_new()
    opts.system = "be terse"
    r = an_messages_create(c, "mock-claude", m, 256, opts)
    print("E1 ok=" + str(an_resp_ok(r)) + " text=[" + an_msg_text(r) + "] model=" + an_msg_model(r))
    print("E2 stop=" + an_msg_stop_reason(r) + " usage=" + an_usage_line(r) + " cache=" + str(an_usage_cache_read(r)))
    ti = an_find_tool_use(r)
    print("E3 tool=" + an_block_tool_name(r, ti) + " city=" + an_block_tool_input_str(r, ti, "city"))
    print("E4 reqid=" + r.request_id)
    # the streaming path, including a tool call reassembled from fragments
    st = an_messages_stream(c, "mock-claude", m, 256, opts)
    print("E5 serr=" + str(st.err) + " status=" + str(st.status))
    live = ""
    guard = 0
    while not st.done and st.err == 0 and guard < 200:
        st = an_stream_next(st)
        if st.res != "":
            d = an_stream_text(st.res)
            if len(d) > 0:
                live = live + d
        guard = guard + 1
    an_stream_close(st)
    sr = an_acc_resp(st.acc)
    print("E6 live=[" + live + "]")
    print("E7 acc=[" + an_msg_text(sr) + "] stop=" + an_msg_stop_reason(sr) + " usage=" + an_usage_line(sr))
    ati = an_find_tool_use(sr)
    print("E8 tool=" + an_block_tool_name(sr, ati) + " input=" + an_block_tool_input(sr, ati))
    # token counting, model list (with a query string), a retried 429, an error
    ct = an_messages_count_tokens(c, "mock-claude", m, an_opts_new())
    print("E9 ctok=" + str(an_count_tokens_value(ct)))
    ml = an_models_list(c)
    print("E10 models=" + str(an_data_len(ml)) + " first=" + an_data_id(ml, 0) + " more=" + str(an_has_more(ml)))
    rb = an_request(c, "POST", "/v1/boom", "{}")
    print("E11 boom=" + str(rb.status) + " retries=" + str(rb.retries) + " msg=[" + an_err_msg(rb) + "] type=" + an_err_type(rb))
    er = an_request(c, "GET", "/v1/nope", "")
    print("E12 err=" + str(er.status) + " msg=[" + an_err_msg(er) + "]")
    return 0
"#;

#[test]
fn anthropic_sdk_end_to_end_mock() {
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
        "an_mock",
        &["ws2_32"],
    );
    let client = build(
        &format!(
            "import * from \"{}\"\n{}",
            abs("stdlib/anthropic/client"),
            E2E_SRC
        ),
        "an_e2e",
        &["winhttp"],
    );

    let mut child = Command::new(&mock)
        .env("MOCK_PORT", port.to_string())
        // the client's own requests, plus the port poll below, plus the two
        // retries the 429 costs, plus slack
        .env("MOCK_COUNT", "10")
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
        Some(Command::new(&client).env("MOCK_PORT", port.to_string()).output().expect("failed to run e2e client"))
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
        "E1 ok=true text=[Hello from the mock] model=mock-claude",
        "E2 stop=tool_use usage=tokens 12 in / 9 out cache=3",
        "E3 tool=get_weather city=Paris",
        "E4 reqid=req_mock_7",
        "E5 serr=0 status=200",
        "E6 live=[Streamed reply]",
        "E7 acc=[Streamed reply] stop=tool_use usage=tokens 14 in / 31 out",
        "E8 tool=lookup input={\"q\":\"abc\"}",
        "E9 ctok=2095",
        "E10 models=1 first=claude-mock more=false",
        // 429 is retried once (Retry-After: 1 -> the SDK waits 1s), so the
        // final answer carries the API's own message and two attempts
        "E11 boom=429 retries=2 msg=[anthropic 429 rate_limit_error: slow down] type=rate_limit_error",
        "E12 err=404 msg=[anthropic 404 not_found_error: no such route]",
    ] {
        assert!(text.contains(marker), "missing marker {marker:?}\nstdout:\n{text}");
    }
}
