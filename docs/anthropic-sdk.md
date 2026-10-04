# The Anthropic SDK (`stdlib/anthropic/`) — v0.41.0

An Anthropic API client written in Aoxn and shipped as a stdlib subpackage
— the Aoxn answer to the `anthropic-sdk-python` sources vendored in
`third-party-package-in-the-making/anthropic-sdk-python-main`. It covers the
core REST surface (Messages, token counting, models, files, message batches),
blocking and streaming (SSE), and **tool use** end to end, with the same
defaults as the reference client: `https://api.anthropic.com` base URL,
600 s receive timeout, 5 s connect timeout, 2 retries after the first
failure.

Everything is plain Aoxn over raw memory — the self-hosted compiler can
compile the whole package (no dict/None/fn-ptr/raise anywhere).

## Quick start

```powershell
$env:ANTHROPIC_API_KEY = "sk-ant-..."
cargo run -- run examples\anthropic_chat.ax -l winhttp
```

```ax
import * from "stdlib/anthropic/client"

def main() -> int:
    c = an_client_env()                       # ANTHROPIC_API_KEY / ANTHROPIC_AUTH_TOKEN / ANTHROPIC_BASE_URL
    m = an_msgs_new()
    m = an_msgs_push_text(m, "user", "Say hello in one word")
    r = an_messages_create(c, "claude-sonnet-4-5", m, 256, an_opts_new())
    if not an_resp_ok(r):
        print(an_err_msg(r))
        return 1
    print(an_msg_text(r))                     # -> Hello!
    print(an_usage_line(r))                   # -> tokens 12 in / 25 out
    return 0
```

Link requirement: **`-l winhttp`** — the transport is WinHTTP, the one
Windows system facility that provides TLS without a TLS stack. Gateways and
local proxies work through `an_client_at(key, base_url)` or
`ANTHROPIC_BASE_URL` (plain `http://` included, which is what the test
suite's mock server uses).

## The layout

| File | What lives there |
|---|---|
| `stdlib/net/codec.ax` | base64, percent-encoding, UTF-8 ↔ UTF-16LE — **shared with the OpenAI SDK** |
| `stdlib/net/json.ax` | the JSON DOM: 40-byte nodes in one slab, recursive-descent parser, serializer, typed getters, and a **builder** (`jb_*`) |
| `stdlib/net/http.ax` | WinHTTP transport (one-shot + streaming), URL split, header assembly, raw-header parsing, retry policy |
| `stdlib/net/sse.ax` | server-sent-events parser: feed bytes, pull `data:` payloads **and `event:` names**, `[DONE]` detection, CRLF/LF, comment lines |
| `stdlib/anthropic/blocks.ax` | content-block constructors and the message list |
| `stdlib/anthropic/tools.ax` | tool definitions, the `input_schema` builder, `tool_choice`, thinking config |
| `stdlib/anthropic/client.ax` | `AnClient`, the request/retry core, resources, response accessors, `AnStream`, `AnAcc` |

The four `stdlib/net/` modules were provider-neutral from the start; they
shipped inside `stdlib/openai/` in v0.40.1 because there was only one SDK
then. This release moved them up one level and renamed their prefix `oa_` →
`net_`, so the OpenAI SDK and this one share one transport instead of
carrying two. `stdlib/openai/client.ax` is now the only OpenAI-specific file
and its public surface is unchanged.

## The client and its defaults

`an_client_new(api_key)` starts from the reference defaults
(`https://api.anthropic.com`, 600 s / 5 s timeouts, 2 retries);
`an_client_env()` fills the key, the optional auth token and the optional
base URL from the environment (`ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`,
`ANTHROPIC_BASE_URL` — the same names the Python SDK reads).

Note the base URL has **no `/v1`**: the paths below carry it, exactly as in
the reference client.

Every request carries `x-api-key` (or `Authorization: Bearer …` when an
auth token is set), **`anthropic-version: 2023-06-01`** — the API requires
it and does not infer it — `Accept`, and a `User-Agent: aoxn-anthropic/<version>`.
`anthropic-beta` is sent only when `an_beta()` added one. Retries fire on
transport errors, 409, 429 and 5xx (a server `Retry-After` of up to 120 s
wins over the backoff); 4xx never retries.

`an_request_url` takes an absolute URL, which is how the batch-results
endpoint works: `results_url` comes back from the API as a full URL to
fetch, not a path to build.

## Optional parameters, without keyword arguments

`messages.create` takes fifteen optional parameters. Aoxn has no default
arguments, no keyword arguments and no overloading, so the optional half is
an `AnOpts` struct. An empty string, an empty `Vec` or a zero means "do not
send this member" — which is exactly the distinction the API draws between
absent and present-and-empty.

```ax
opts = an_opts_new()
opts.system = "Answer in one sentence."
opts.stop_sequences = an_str_vec2("STOP", "END")
opts.tools = tools
opts.tool_choice = an_tool_choice_auto(False)
opts.thinking = an_thinking_enabled(2048, "summarized")
opts.metadata_user_id = "user-42"
opts.service_tier = "auto"
r = an_messages_create(c, model, m, 512, opts)
```

`max_tokens` is **required** and is a plain parameter — the API has no
server default for it, so an omitted one is a 400.

## Messages and content blocks

A message's `content` is either a string or an ordered array of typed
blocks. `AnMsgs` carries the list (roles in one `Vec`, rendered content
values in another):

```ax
m = an_msgs_new()
m = an_msgs_push_text(m, "user", "plain text message")

bl = vec_new()
bl = vec_push_str(bl, an_block_text("What is in this photo?"))
bl = vec_push_str(bl, an_block_image_url("https://example.com/a.png"))
m = an_msgs_push_blocks(m, "user", bl)
```

A block is represented as a **JSON text string**, not a struct. That one
choice is what keeps the streaming path honest: the accumulator rebuilds a
Message from deltas as text, so `an_msg_text` and the block accessors read a
streamed message and a non-streamed one through the same API. The language
has no sum types, so a struct per block kind would mean a struct per kind at
every call site.

Constructors in `blocks.ax`:

| Function | Block |
|---|---|
| `an_block_text(text)` / `an_block_text_cached(text, ttl)` | `text` |
| `an_block_thinking(thinking, signature)` | `thinking` (echoed back verbatim) |
| `an_block_redacted_thinking(data)` | `redacted_thinking` |
| `an_block_image_b64(media_type, data)` / `_url(url)` / `_file(file_id)` | `image` |
| `an_block_document(media_type, data)` / `an_block_document_titled(...)` | `document` |
| `an_block_tool_use(id, name, input_json)` / `_cached(...)` | `tool_use` |
| `an_block_tool_result(tool_use_id, content)` / `_error(...)` / `_blocks(...)` | `tool_result` |
| `an_block_server_tool_use(id, name, input_json)` | `server_tool_use` |
| `an_block_web_search_result(tool_use_id, results)` | `web_search_tool_result` |

## Tools

A tool is `{name, description, input_schema}`. The schema builder assembles
one field at a time so every description goes through the JSON escaper
rather than through a hand-written string literal:

```ax
sc = an_schema_new()
sc = an_schema_str(sc, "city", "City name, e.g. \"Paris\"")
sc = an_schema_prop(sc, "unit", "string", "c or f", "[\"c\",\"f\"]")
sc = an_schema_int(sc, "days", "Forecast days")
sc = an_schema_required(sc, an_str_vec2("city", "days"))

tools = vec_new()
tools = vec_push_str(tools, an_tool("get_weather", "Current weather.", an_schema_json(sc)))
opts.tools = tools
```

`an_tool` / `an_tool_cached` / `an_server_tool` / `an_tool_web_search` /
`an_tool_web_fetch` build the tool object; `an_schema_raw` splices in a
schema you wrote yourself.

`tool_choice`: `an_tool_choice_auto`, `an_tool_choice_any`,
`an_tool_choice_tool(name, …)`, `an_tool_choice_none`.

Thinking: `an_thinking_enabled(budget, display)` (budget ≥ 1024),
`an_thinking_adaptive(display)`, `an_thinking_disabled()`.

### The tool loop

The model answers with a `tool_use` block instead of text. Run the tool,
send the whole assistant turn back verbatim, put the result in a user-role
message, and ask again:

```ax
r = an_messages_create(c, model, m, 512, opts)
ti = an_find_tool_use(r)                      # -1 when it just answered
if ti >= 0:
    call_id = an_block_tool_id(r, ti)
    reply = vec_new()
    reply = vec_push_str(reply, an_block_tool_use(call_id, an_block_tool_name(r, ti), an_block_tool_input(r, ti)))
    m = an_msgs_push_blocks(m, "assistant", reply)
    res = vec_new()
    res = vec_push_str(res, an_block_tool_result(call_id, "18C, light rain"))
    m = an_msgs_push_blocks(m, "user", res)
    r = an_messages_create(c, model, m, 512, opts)
```

`an_block_tool_input_str(r, i, "city")` reads one string argument without a
parse; `an_block_tool_input(r, i)` gives the whole input back as JSON text,
ready to hand to `an_block_tool_use` unchanged.

## Resources

| Function | Endpoint |
|---|---|
| `an_messages_create` | `POST /v1/messages` |
| `an_messages_stream` (+ `an_stream_next` / `an_stream_close`) | `POST /v1/messages` with `"stream":true` |
| `an_messages_count_tokens` / `an_count_tokens_value` | `POST /v1/messages/count_tokens` |
| `an_models_list` / `an_models_retrieve` | `GET /v1/models[/{id}]` |
| `an_files_list` / `an_files_retrieve` / `an_files_delete` | `GET`/`DELETE /v1/files[/{id}]` |
| `an_files_upload` | `POST /v1/files` (multipart/form-data, raw bytes) |
| `an_batches_create` / `an_batches_list` / `an_batches_retrieve` / `an_batches_delete` / `an_batches_cancel` | `/v1/messages/batches[/{id}[/cancel]]` |
| `an_batches_results` | the `results_url` the retrieve response handed back |
| `an_request` / `an_request_url` | anything else |

`an_batches_create` takes a `Vec` of request objects built with
`an_batch_request(custom_id, msgs, max_tokens, opts)`.

## Reading responses

Nothing raises. `an_resp_ok(r)` is the gate; `an_err_msg(r)` renders
whichever layer failed first into one string. `an_status_class(status)` and
`an_err_type(r)` give the reference's exception taxonomy as data:

```ax
an_msg_text(r)        # every text block, joined
an_msg_thinking(r)    # every thinking block, joined
an_msg_stop_reason(r) # end_turn | max_tokens | stop_sequence | tool_use | …
an_block_count(r) / an_block_type(r, i) / an_block_text_at(r, i)
an_find_tool_use(r)   # index of the first tool_use block, or -1
an_usage_in/out/cache_read/cache_create/thinking(r)
an_data_len/id(r, i) / an_has_more(r) / an_first_id(r) / an_last_id(r)
an_batch_status(r) / an_batch_results_url(r)
an_file_id/name/media_type/size(r)
```

Every accessor is default-safe: on an empty, malformed or non-Message body
they return "" / 0 / -1 rather than reading out of bounds.

## Streaming

```ax
st = an_messages_stream(c, model, m, 256, opts)
if st.err == 0:
    while not st.done and st.err == 0:
        st = an_stream_next(st)
        if st.res != "":
            print(an_stream_text(st.res))    # one text_delta
    an_stream_close(st)
print(an_msg_text(an_acc_resp(st.acc)))       # the finished message
```

The event sequence is `message_start`, then a `content_block_start` /
N × `content_block_delta` / `content_block_stop` per block, then
`message_delta` and `message_stop`, with `ping` frames interleaved and an
`error` frame possible anywhere.

Two details make this API's stream harder than OpenAI's, and both are
handled:

- **Events are named.** Each frame carries an `event:` line as well as the
  payload's own `type`. `net/sse.ax` captures both; `st.ev` is the name and
  `an_stream_type(st.res)` the payload's type. A frame with an event name
  and no data line is dropped rather than dispatched.
- **A tool call's arguments arrive as JSON text fragments.** They are not
  valid JSON until the block stops, and they exist nowhere else. That is
  what `AnAcc` is for: `an_stream_next` folds every event into it, and
  `an_acc_resp(st.acc)` hands back a finished `AnResp` — assembled with the
  fragments parsed — that the ordinary accessors read.

The pure per-event helpers (`an_stream_text`, `an_stream_thinking`,
`an_stream_partial_json`, `an_stream_index`, `an_stream_stop_reason`,
`an_stream_output_tokens`, `an_stream_error_msg`) are ordinary functions
over one event's JSON, which is what the offline tests pin. `an_acc_feed`
does the reassembly and is likewise testable with no network.

A stream that never reaches 2xx is drained and reported through `st.err` /
`st.errmsg` before the loop starts — a stream either runs clean or fails
clean. An `error` event mid-stream sets `st.err` and stops the loop.

## Errors

| Layer | Signal | Message source |
|---|---|---|
| transport | `r.status == 0`, `r.terr == 1` | `net_winhttp_err` (timeout, dns failure, connection refused, …) |
| HTTP | `r.status >= 400` | the parsed body's `error.message`, prefixed with the status and its class |
| body | `r.pok == 0` | the JSON parser's message |

## Tests

`cargo test --test anthropic_sdk` runs two drivers, both offline:

- **pure layers** — header assembly (including the required
  `anthropic-version`), content-block JSON, the message list, the schema
  builder, every request body, pagination paths, the multipart prelude, the
  raw response-header parser, the JSON builder (including nested fragments
  parsed into the same slab), the status→class table, response accessors,
  the named-event SSE filter, and the accumulator over a canned transcript
  whose tool arguments arrive in fragments;
- **end-to-end** — a mock Anthropic server *written in Aoxn* (on
  `web/sock_win.ax`, ws2_32) is spawned on a loopback port; the real
  WinHTTP path creates a message, consumes a full SSE stream and reassembles
  it, counts tokens, lists models with a query string, honors a
  `Retry-After: 1` on a 429, and walks a 401 and a 404. No credentials, no
  external service.

`cargo test --test openai_sdk` still passes unchanged: the shared layer's
refactor is pinned by the OpenAI suite as well as this one.

`examples/anthropic_chat.ax` is the live demo (a message, a stream, a tool
loop, a token count, a model list); without a key it prints what is missing
and exits 0, so CI can always run it.

## Deliberate limits (v0.41.0)

- **`temperature` / `top_p` / `top_k` are not sent.** The vendored reference
  is a fork whose `messages.create` body does not contain them; the SDK
  emits exactly the members that body sends. Add them with `an_request` if
  a future API version wants them.
- **The beta surface is not ported.** `client.beta.*` in the reference is
  dozens of endpoints (`?beta=true` on every one); `an_beta()` sends the
  `anthropic-beta` header so those endpoints can be reached with
  `an_request`, but there are no typed wrappers for them.
- **The organization admin surface is not ported** (users, workspaces,
  service accounts, federation, invitations, compliance) — it is dozens of
  CRUD endpoints with no bearing on the Messages API.
- `messages.parse` (structured output) is not ported; it is a Python type
  post-processor over `output_config`.
- **Binary request bodies travel by pointer, not by string.** `len()` on an
  Aoxn string is `strlen`, so a body containing a NUL byte cannot ride
  inside one — which is every multipart upload of a binary file.
  `net_http_request_bytes` takes `(ptr, len)` and `an_files_upload` uses it.
- Floats serialize with up to 15 significant digits and no printf —
  `snprintf` is variadic and cannot be reached from a fixed-arity extern.
- `Retry-After` is honored only up to 120 s; there is no exponential
  jitter.
- **Response headers are read with `WINHTTP_QUERY_RAW_HEADERS`, not
  `WINHTTP_QUERY_CUSTOM`.** The by-name query returns
  `ERROR_INVALID_PARAMETER` (87) on this platform for every name tried, so
  the transport takes the whole header block and `net_header_value` scans
  it — which also makes the parsing pure and offline-testable.
