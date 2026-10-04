# The OpenAI SDK (`stdlib/openai/`) — v0.40.1

An OpenAI API client written in Aoxn and shipped as a stdlib subpackage —
the Aoxn answer to `openai-python`. It covers the core REST surface
(chat completions, the Responses API, embeddings, models, moderations),
both blocking and streaming (SSE), with the same defaults as the reference
Python SDK: `https://api.openai.com/v1` base URL, 600 s receive timeout,
5 s connect timeout, 2 retries after the first failure (0.5 s, then 1 s).

Everything is plain Aoxn over raw memory — the self-hosted compiler can
compile the whole package (no dict/None/fn-ptr/raise anywhere).

## Quick start

```powershell
$env:OPENAI_API_KEY = "sk-..."
cargo run -- run examples\openai_chat.ax -l winhttp
```

```ax
import * from "stdlib/openai/client"

def main() -> int:
    c = oa_client_env()                       # OPENAI_API_KEY / OPENAI_BASE_URL / OPENAI_ORG_ID / OPENAI_PROJECT_ID
    roles = vec_new()
    contents = vec_new()
    roles = vec_push_str(roles, "user")
    contents = vec_push_str(contents, "Say hello in one word")
    r = oa_chat_create(c, "gpt-4o-mini", roles, contents)
    if not oa_resp_ok(r):
        print(oa_err_msg(r))
        return 1
    print(oa_chat_text(r))                    # -> hello
    return 0
```

Link requirement: **`-l winhttp`** — the transport is WinHTTP, the one
Windows system facility that provides TLS without a TLS stack. Azure-style
gateways and local proxies work through `oa_client_at(key, base_url)` or
`OPENAI_BASE_URL` (plain `http://` included, which is what the test suite's
mock server uses).

## The five modules

| File | What lives there |
|---|---|
| `stdlib/openai/codec.ax` | base64 (RFC 4648), percent-encoding (RFC 3986), UTF-8 ↔ UTF-16LE — everything WinHTTP and Basic auth need |
| `stdlib/openai/json.ax` | the JSON DOM: 40-byte nodes in one slab, recursive-descent parser, serializer, typed getters with defaults |
| `stdlib/openai/http.ax` | WinHTTP transport (one-shot + streaming), URL split, header assembly, retry policy, error names |
| `stdlib/openai/sse.ax` | server-sent-events parser: feed bytes, pull `data:` payloads, `[DONE]` detection, CRLF/LF, comment lines |
| `stdlib/openai/client.ax` | `OaClient`, the request/retry core, resource functions, response accessors, `OaStream` |

## The client and its defaults

`oa_client_new(api_key)` starts from the reference defaults
(`https://api.openai.com/v1`, 600 s / 5 s timeouts, 2 retries); `oa_client_env()`
fills the key and the optional base URL / org / project from the environment
(the same variable names the Python SDK reads: `OPENAI_API_KEY`,
`OPENAI_BASE_URL`, `OPENAI_ORG_ID`, `OPENAI_PROJECT_ID`).

Every request carries `Authorization: Bearer …`, `Accept: application/json`
and a `User-Agent: aoxn-openai/<version>`; `Content-Type: application/json`
rides along on bodies, `OpenAI-Organization` / `OpenAI-Project` only when
set. Retries fire on transport errors, 429 and 5xx (a server `Retry-After`
of up to 120 s wins over the backoff); 4xx never retries.

## Resources

| Function | Endpoint |
|---|---|
| `oa_chat_create` / `oa_chat_create_full` | `POST /chat/completions` |
| `oa_responses_create` / `oa_responses_create_full` | `POST /responses` |
| `oa_embeddings_create` / `oa_embeddings_create_many` | `POST /embeddings` |
| `oa_models_list` / `oa_models_retrieve` / `oa_models_delete` | `GET/DELETE /models[/{id}]` |
| `oa_moderations_create` / `oa_moderations_create_model` | `POST /moderations` |
| `oa_chat_stream` (+ `oa_stream_next` / `oa_stream_close`) | `POST /chat/completions` with `"stream":true` |

Message lists are parallel `Vec`s of strings (roles, contents) — the Aoxn
idiom for struct-less lists. Bodies are rendered with the SDK's own JSON
escaper; anything the typed helpers don't cover can be posted raw:

```ax
r = oa_request(c, "POST", "/chat/completions", my_body_json)
```

## Reading responses

Nothing raises. `oa_resp_ok(r)` is the gate; `oa_err_msg(r)` renders
whichever layer failed first — transport (`winhttp error …`), HTTP status,
or the API's `error.message` — into one string. Field accessors are
default-safe on any response:

```ax
oa_chat_text(r)      # choices[0].message.content
oa_chat_finish(r)    # choices[0].finish_reason
oa_usage_in/out/total(r)
oa_responses_text(r) # Responses API: output[].content[].text joined
oa_data_len/id(r, i) # list endpoints: data[] count, data[i].id
oa_embedding_len/at  # embeddings: data[i].embedding[k]
```

## Streaming

```ax
st = oa_chat_stream(c, "gpt-4o-mini", roles, contents, 0.2, 1, 0)
if st.err == 0:
    while not st.done and st.err == 0:
        st = oa_stream_next(st)
        if st.res != "":
            print(oa_stream_text(st.res))   # choices[0].delta.content
    oa_stream_close(st)
```

A stream that never reaches 2xx is drained and reported through
`st.err` / `st.errmsg` (the API's error message) before the loop starts —
a stream either runs clean or fails clean. `oa_stream_text` /
`oa_stream_finish` are pure functions over one chunk's JSON, which is what
the offline tests pin.

## Errors

| Layer | Signal | Message source |
|---|---|---|
| transport | `r.status == 0`, `r.terr == 1` | `oa_winhttp_err` (timeout, dns failure, connection refused, …) |
| HTTP | `r.status >= 400` | the parsed body's `error.message`, else `http <status>` |
| body | `r.pok == 0` | the JSON parser's message |

## Tests

`cargo test --test openai_sdk` runs two drivers, both offline:

- **pure layers** — RFC 4648 base64 vectors, JSON round-trips and escapes
  (surrogate pairs included), URL split, header blocks, the retry table,
  SSE extraction (whole events, split feeds, CRLF), body rendering,
  response accessors;
- **end-to-end** — a mock OpenAI server *written in Aoxn* (on
  `web/sock_win.ax`, ws2_32) is spawned on a loopback port; the real
  WinHTTP path performs a chat completion, consumes a full SSE stream and
  walks a 401 error body. No credentials, no external service.

`examples/openai_chat.ax` is the live demo; without a key it prints what
is missing and exits 0, so CI can always run it.

## Deliberate limits (v0.40.1)

- Floats serialize with up to 15 significant digits and no printf —
  `snprintf` is variadic and cannot be reached from a fixed-arity extern
  (the float varargs would be read from register slots the caller never
  filled). Whole floats print without a decimal point.
- `Retry-After` is honored only up to 120 s; there is no exponential
  jitter (the reference SDK's backoff constants are used as-is).
- Files, uploads (multipart), audio, images and realtime/websockets are
  not covered yet; `oa_request` + the JSON DOM are the escape hatch.
