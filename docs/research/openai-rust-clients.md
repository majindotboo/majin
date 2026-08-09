# OpenAI Rust client capabilities

Research date: 2026-08-09.

## Question

Majin needs one real OpenAI provider without rebuilding the Responses API transport and type surface.
The client must preserve OpenAI-native features that matter to a coding harness.

## Sources

- OpenAI API documentation for Responses WebSocket, custom tools, context-free grammars, tools, and compaction.
- `64bit/async-openai` release `0.41.3`, commit `ca746070fdcd27de56b41966d31f1aa6c8600ce4`.
- `fortunto2/openai-oxide` crate `0.15.0`, commit `0ee0694e1ee8cd34095d79793ee9e364ce4e946c`.
- `jeremychone/rust-genai` commit `21c36bc4ad60063e20407ee2e6d566cedfff0c47`.
- Local `openai/codex` checkout commit `50ef7395faee1d0e2d01730f9636aa06091c7be3` from 2026-08-09.
- `jeremychone/rust-genai` issue `#176` from 2026-03-24.

## Conclusion

`async-openai` and `openai-oxide` are the direct published OpenAI client candidates.
rust-genai is a provider-neutral adapter.
Vendoring Codex gives the strongest Codex-specific transport behavior but the weakest ownership boundary.

Use `async-openai` when complete typed OpenAI Responses coverage matters more than WebSocket transport.
Evaluate `openai-oxide` when Responses WebSocket is required immediately.
Its WebSocket implementation is real, but its public Responses request and event enums lag behind its generated type catalog.

Do not vendor the Codex workspace unchanged.
Its internal crates are not a stable SDK and have a large transitive workspace dependency graph.
Do not put rust-genai between Majin and OpenAI unless multi-provider convenience becomes more important than native event fidelity.

## Capability matrix

| Capability | async-openai 0.41.3 | openai-oxide 0.15.0 | rust-genai 0.7 beta | Vendor Codex `50ef7395` |
| --- | --- | --- | --- | --- |
| OpenAI Responses API | Native endpoint and generated types | Native endpoint and generated types | Adapter behind provider-neutral chat API | Codex-specific internal wire layer |
| Responses HTTP/SSE | Yes | Yes | Yes | Yes. SSE-focused |
| Responses WebSocket | No | Yes. One in-flight request per connection | No | Yes. Sequential reuse with prewarm and fallback |
| Custom freeform tools | Typed | Partial. Generated type exists, but public `ResponseTool` omits it | Yes through custom format JSON | Typed custom tool with stringly typed format |
| Lark CFG | Typed | Generated type. Public request builder is incomplete | Supported through custom format JSON and example | Yes. Used by Codex freeform tools |
| Regex CFG | Typed | Generated type. Public request builder is incomplete | Passed through custom format JSON | Raw syntax string. No enum validation |
| Structured JSON Schema output | Yes | Yes, including `parse::<T>()` | Yes. No typed parse helper | Typed request controls. No typed parse helper |
| Provider-native stream events | Broad typed Responses event enum | Partial typed enum with raw `Other` fallback | Normalized coarse chat events | Selective Codex event enum plus typed response items |
| Responses stream accumulator | No | Partial function-call helper. WebSocket `send` returns final response | Partial capture in end event | No in wire layer. Codex core owns accumulation |
| Conversations API | Yes | Yes | No | No |
| `previous_response_id` | Yes | Yes | Yes | Yes for incremental WebSocket requests. HTTP request type omits it |
| Encrypted reasoning replay | Typed | Generated types | Yes, opt-in capture and replay | Typed |
| Codex message phases | Typed | Generated types | No explicit native phase model | Typed and used |
| Native `apply_patch` | Typed request and events | Generated types, but public request tool enum omits it | Custom grammar example, not native event type | Custom Lark tool, not native API tool type |
| Native local or managed shell | Typed request and events | Generated types, but public request tool enum omits them | No | Typed response item only |
| Responses compaction endpoint | Typed | Endpoint exists and returns raw JSON | No | Typed endpoint returning response items |
| Background response cancellation | Yes | Yes | No explicit API | No endpoint |
| Production transport policy | Basic client policy | Basic retry and persistent WebSocket | Provider-neutral retry policy | Strong retry, prewarm, connection reuse, compression, telemetry, and HTTP fallback |
| Public API stability | Published SDK | Published SDK | Published beta abstraction | Private workspace crates at version `0.0.0` |
| Maintenance burden in Majin | Low | Medium | Medium | Very high |
| Native metadata fidelity | High | Medium-high with typed gaps | Medium | High for Codex paths, incomplete for general Responses |

## openai-oxide

Strengths:

- It is the only reviewed published Rust client with a Responses WebSocket implementation.
- `WsSession` supports complete responses and streamed events over one persistent connection.
- HTTP Responses supports SSE, raw requests, structured `parse::<T>()`, background cancellation, compaction, Conversations, and conversation items.
- Generated types include custom grammar tools, Lark, regex, reasoning encrypted content, Codex message phases, apply patch, local shell, managed shell, MCP, skills, and compaction.
- Raw HTTP request methods provide an escape hatch for fields missing from curated request types.

Gaps and inconsistencies:

- WebSocket supports one in-flight request and no multiplexing.
- WebSocket `send` accepts the curated `ResponseCreateRequest`.
- The curated `ResponseTool` enum omits custom tools, apply patch, local shell, managed shell, namespaces, and tool search.
- The typed `ResponseStreamEvent` enum covers core lifecycle, text, function, and reasoning events.
- Custom-tool, shell, apply-patch, MCP, and other newer events fall through to raw `Other(serde_json::Value)`.
- Responses accumulation is limited to a function-call helper.
- The full accumulator is Chat Completions-only.
- The repository is much smaller and less exercised than async-openai.

The crate advertises Python SDK parity.
The generated catalog approaches that goal.
The hand-maintained public Responses facade does not yet provide complete typed parity.

## async-openai

Strengths:

- `responses` exposes create, stream, retrieve, retrieve-stream, cancel, compact, Conversations, and conversation items.
- `Tool` includes function, custom, MCP, local shell, managed shell, computer, namespace, tool search, and apply patch variants.
- `CustomToolParamFormat` includes unconstrained text and typed grammar configuration.
- Grammar syntax supports Lark and regex.
- `ResponseStreamEvent` preserves provider-native event variants.
- Response types preserve reasoning encrypted content, compaction bodies, message phases, tool calls, and provider IDs.

Gaps:

- No Responses WebSocket transport.
- No general stream accumulator or reducer.
- No generic cancellation token for an active foreground SSE request.
- It is an API client, not an agent runtime.

Majin already intends to own the missing runtime behavior.
These gaps are narrow and belong at the Majin provider boundary.

## rust-genai

rust-genai has improved since issue `#176`.
The issue is an informational benchmark from March 2026.
Its capability table is not current enough to select a library without source inspection.

Fixed or present now:

- OpenAI Responses adapter.
- HTTP/SSE streaming.
- Custom freeform tools.
- Grammar-constrained custom tools.
- Structured JSON Schema output.
- Encrypted reasoning capture and replay.
- `previous_response_id` stateful Responses flow.
- Captured text, tools, reasoning, usage, stop reason, and response ID at stream end.

Still absent or partial:

- Responses WebSocket is absent.
- OpenAI Conversations API is absent.
- Responses compaction endpoint is absent.
- Explicit request cancellation is absent.
- Stream events are normalized into coarse chat events.
- Provider-native tool and item variants are not preserved as distinct public types.
- No general typed stream accumulator exists.

Its custom format is raw JSON passed to OpenAI.
The current example demonstrates Lark.
Regex grammar can use the same passthrough shape, but it lacks a dedicated typed helper or example.

rust-genai is useful when one normalized API across many providers is the main goal.
Majin already defines that normalization boundary in ECS.
Using both creates two provider-neutral models and loses OpenAI-native information before Majin can decide what to persist.

## Vendoring Codex

The relevant internal crates are `codex-api`, `codex-client`, `codex-http-client`, `codex-websocket-client`, `codex-protocol`, and parts of `codex-tools`.

Strengths:

- `codex-api` implements Responses SSE, Responses WebSocket, compaction, structured output controls, authentication, provider configuration, and stream parsing.
- The WebSocket client supports connection prewarm, sequential reuse, `previous_response_id`, incremental input, compression, telemetry, and HTTP fallback.
- `codex-protocol::ResponseItem` preserves messages, phases, reasoning encrypted content, local shell calls, function calls, custom tool calls, tool search, web search, image generation, and compaction.
- `codex-tools` models freeform grammar tools and serializes Lark tools used by Codex.
- This code receives direct production exercise in Codex.

Costs:

- These crates are private workspace components at version `0.0.0`.
- Their interfaces change with Codex needs instead of third-party compatibility.
- `codex-api` alone has a transitive closure of 20 internal Codex crates in the current workspace.
- Adding `codex-tools` unchanged expands the closure to 84 internal Codex crates.
- The six headline crates contain about 54,000 lines of Rust before their remaining internal dependencies.
- Tool formats use raw strings for `type`, `syntax`, and grammar definitions.
- The stream event enum is deliberately selective and discards several provider-native event distinctions.
- The HTTP request type lacks `previous_response_id`; Codex uses that optimization through its WebSocket request type.
- It has no OpenAI Conversations client or background response cancellation endpoint.

Vendoring these crates unchanged would import a large moving subsystem.
Extracting only their transport ideas would create a new Majin-owned client and ongoing synchronization work.
Neither option is a low-maintenance dependency choice.

## Recommendation for tickets

- Ticket 2 must add a real `OpenAiProvider` with `async-openai` Responses HTTP/SSE.
- Ticket 2 must stream one real assistant response into World facts.
- Ticket 3 must exercise one custom freeform tool with a small Lark grammar.
- Fake provider behavior remains a deterministic integration-test double.
- Responses WebSocket remains out of scope until SSE behavior is measured.
- The provider module must isolate `async-openai` types from Majin domain components.
