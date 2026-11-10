# RedGear API protocol 4

**Status: implemented by `mroww-redgear-api`.** The adapter speaks protocol 4; protocol 3 is not supported. Optional atomic recompilation is specified below but is not currently implemented.

Protocol 4 lets a client send a schematic, start compilation, receive progress, and operate the resulting simulation session. A session owns its circuit and simulation state.

## Connection and message format

The transport is a raw TCP socket. Each message is one UTF-8 JSON object followed by a newline (`\n`). There is no HTTP request or WebSocket handshake. The existing adapter listens on IPv4 loopback at `127.0.0.1:8765` by default.

Clients and servers must buffer socket reads until a complete newline-terminated message is available. One socket read can contain part of a message or several messages. The server serializes outgoing messages so that bytes from concurrent sessions cannot interleave. Diagnostic logs go to standard error, never into the socket stream.

Examples below show one JSON object per line. `<base64>` represents actual Base64-encoded bytes.

The current adapter limits each message to 16 MiB, including its terminating newline. A schematic can contain at most 30,000 cells and span at most 64 blocks along each axis. An `advance` request accepts at most 64 inputs and 1,000,000 game ticks. Each connection can own at most four sessions, and the service accepts at most four simultaneous connections. Excess connections are closed; rejected requests on an established connection receive an error.

## Requests, sessions and identifiers

The first request on a connection is `begin`. It performs the protocol handshake, supplies a schematic and settings, and starts compilation. No separate `rawr` message is required.

A connection can own multiple sessions. The server assigns each session an opaque `session_id`; the client uses it in subsequent requests. Session IDs are scoped to the connection. Disconnecting cancels its active compilations and releases its sessions.

`id` is an optional client-provided unsigned integer used to match a request with its response. An `id` used by an outstanding request must be unique within the connection. If an ordinary request includes `id`, its result or error echoes that `id`. If `begin` includes `id`, `accepted` echoes it. Progress and the final compilation message may omit it and always identify their session with `session_id`.

When sending `begin` without an `id`, wait for its `accepted` or error response before sending another `begin` without an `id`. This makes it possible to match an acknowledgment to its request. For other requests without an `id`, wait for the response before sending another request for that session.

Requests for a session execute in order. Different sessions may compile and run concurrently. While a session is compiling, operations other than `close` receive a `session_busy` error. An implementation may enforce resource limits and must report rejected requests with an error rather than silently starting a partial session.

## Start a session

The client sends:

```json
{"id":1,"op":"begin","protocol":4,"format":"sponge-v3","schematic":"<base64>","settings":"--publish-io","backend":"mroww-llvm"}
```

| Field | Meaning |
| --- | --- |
| `id` | Optional request identifier. |
| `op` | Must be `begin`. |
| `protocol` | Must be the integer `4`. |
| `format` | Must be `sponge-v3`. |
| `schematic` | Base64 encoding of the compressed Sponge v3 schematic. |
| `settings` | Compiler options as one string. An empty string selects defaults. |
| `backend` | Simulation implementation selected from the table below. |

| Backend selector | Behavior |
| --- | --- |
| `mroww-redpilator` | Redpilator with the Rust executor. Rejects conflicting `--llvm` settings. |
| `mroww-llvm` | Redpilator with the LLVM executor. `--llvm` need not also appear in settings. |
| `mroww-interpreter` | Physical interpreter. Requires empty settings. |
| `mroww-interpteter` | Accepted alias for `mroww-interpreter`. |
| `redpilator` | Compatibility selector used by the RedGear client. Uses Rust by default and LLVM when settings contain `--llvm`. |

Settings use the existing Redpilator option syntax, for example `""`, `"--publish-io"`, or `"--llvm --publish-io"`. Unknown or conflicting settings are errors. The string is parsed as options; it is not executed as a shell command. Selectors choose the simulation implementation; the server identity in `accepted` remains `mroww`.

After validating the protocol, request envelope, settings and available capacity, the server acknowledges the request:

```json
{"type":"accepted","id":1,"session_id":"s1","protocol":4,"engine":"mroww","capabilities":{"recompile":false}}
```

`accepted` confirms that the request was accepted for processing. It does not guarantee that the schematic is valid or that compilation will succeed. If validation fails before a session is allocated, the server sends an `error` response; it has no `session_id`.

Incremental recompilation is an optional capability. A server that supports it advertises `"capabilities":{"recompile":true}` in `accepted`. If the field is absent or false, clients must not assume that `recompile` is available.

## Compilation progress

After `accepted`, the server may send zero or more progress messages:

```json
{"type":"progress","session_id":"s1","phase":"sampling","description":"Sampling piston arrangements","details":"Checking movable blocks and their electrical connections."}
```

| Field | Meaning |
| --- | --- |
| `type` | `progress`. |
| `session_id` | Session being compiled. |
| `id` | Optional identifier of the original `begin` request. |
| `phase` | Short machine-readable phase identifier. |
| `description` | Short human-readable status. |
| `details` | Optional longer human-readable status. |
| `completed`, `total`, `unit` | Optional progress within the current phase, when known. |

Descriptions are for display; clients should not parse their wording. Clients must tolerate unknown phase identifiers. Counts describe the current phase, not necessarily the whole compilation. The server should emit progress when the phase changes and periodically during long phases, without inventing percentages when the total is unknown.

The current adapter reports phase `compiling`, description `Preparing simulation`, and the compiler monitor's current status in `details`. These messages are emitted periodically while preparation runs. The example above illustrates another valid phase; clients must not depend on receiving it.

Every accepted `begin` ends with exactly one terminal message: `finished` or `error`. No progress follows that terminal message.

Successful compilation:

```json
{"type":"finished","session_id":"s1","tick":0,"backend":"llvm"}
```

Here `backend` describes the executor actually selected: `rust`, `llvm`, or `interpreter`. In the `begin` request, `backend` instead selects a simulation implementation using the selectors above. The `finished` message can optionally echo the `begin` request's `id`.

## Edit blocks and recompile

The current adapter advertises `recompile:false` and rejects `recompile` with `unsupported_operation`, leaving the existing session usable. The rest of this section defines the optional capability for implementations that advertise it.

When `recompile` is supported, the client can replace selected blocks without resending the whole schematic:

```json
{"id":6,"op":"recompile","session_id":"s1","incremental":true,"states":["minecraft:redstone_block","minecraft:air"],"changes":[[2,3,4,0],[5,1,2,1]]}
```

`states` and `changes` use the same palette and `[x,y,z,state_index]` representation as a projection response. Each position must lie within the original schematic dimensions and appear at most once. Every palette entry and index must be valid; air explicitly removes a block. An empty change list is valid and requests recompilation without block edits. Validate the entire request before starting work.

`incremental` is optional and defaults to `true`. It asks the backend to reuse previous compilation work when safe. If reuse is unavailable or unsafe, the backend performs a full compilation of the edited state instead. With `"incremental":false`, it performs a full compilation without reusing prior program-dependent compilation work. Both choices have the same required simulation behavior; reuse does not guarantee a particular compilation speed.

The patch applies to the session's **current state** at a tick boundary. No simulation ticks are advanced while recompilation runs. Block edits are raw state replacements: they do not act as lever inputs or synthesize placement and neighbor notifications. The candidate compilation uses the session's existing backend and settings.

The server acknowledges and reports progress using the existing message types:

```json
{"type":"accepted","id":6,"session_id":"s1","operation":"recompile"}
{"type":"progress","session_id":"s1","operation":"recompile","phase":"sampling","description":"Sampling changed dependencies"}
{"type":"finished","session_id":"s1","operation":"recompile","tick":16,"backend":"llvm","reused_compilation_work":true}
```

`operation` is `recompile` on its acknowledgment, progress and terminal messages. The optional `reused_compilation_work` field reports whether previous compilation work was actually reused; omit it when the backend cannot determine that accurately. As with `begin`, progress and terminal messages may omit `id`.

Recompilation is atomic. The server builds a candidate from the current session and applies the edits to that candidate. The existing session stays paused and unchanged until the candidate is successfully compiled and installed. On success, the session ID and tick remain the same, and unaffected simulation state and pending work are preserved. If the backend cannot safely preserve the current state—for example, because an active motion or pending work cannot be imported—it must reject the operation instead of silently resetting it. A failed or cancelled recompilation discards the candidate and leaves the old session usable and unchanged.

On successful recompilation, a block edit cancels any scheduled input release at that position. Releases at unedited positions remain scheduled. A failed or cancelled recompilation leaves all scheduled releases unchanged.

Block entity data is not included in this patch format. Preserve compatible existing block entities when changing a block state; remove incompatible old entities and create any required default entity when supported. Reject an edit when the resulting block or active state cannot be represented safely without additional entity or motion data.

The original schematic supplied to `begin` remains the reference for every `baseline`, including after successful edits. A `recompile` response does not itself send a projection and does not change the last-sent reference. The next `changes` request therefore includes edited blocks whose current states differ from that reference, along with any other unsent changes. Edits can also cause other visible changes; those are included under the same projection rules.

Reusing cached observations is an implementation detail. The installed program must behave as a fresh compilation of the same edited snapshot, settings and entry state. Existing source-order observations can be reused only when their dependency and context checks succeed; wiring, piston arrangements and other invalidated work must be rebuilt.

## Advance the simulation

After `finished`, the client can apply inputs and advance an exact number of game ticks:

```json
{"id":2,"op":"advance","session_id":"s1","ticks":16,"inputs":[{"kind":"use","pos":[2,3,4]},{"kind":"plate","pos":[5,1,2],"powered":true}],"projection":true}
```

- `ticks` is an unsigned integer; `0` applies inputs without advancing a tick.
- `inputs` is an array and may be empty. Inputs are applied in array order before any requested ticks.
- `use` operates a supported control such as a lever or button. `plate` sets a pressure plate to the supplied Boolean `powered` state.
- `pos` is `[x,y,z]` in schematic-local block coordinates. Each coordinate must lie inside the original schematic dimensions.
- `projection` is optional and defaults to `false`. When true, the response also includes `changes` as defined below.

An input can also explicitly set a control's power, optionally for a limited number of ticks:

```json
{"id":2,"op":"advance","session_id":"s1","ticks":16,"inputs":[{"pos":[2,3,4],"power":15,"duration_ticks":10}],"projection":true}
```

This form has no `kind` field and supports levers and binary pressure plates. The position must identify a supported control. Button presses remain available through `kind: "use"` and use the button's normal release behavior.

- `power` is optional and defaults to `15`. It must be an integer from `0` to `15`: `0` turns the control off, and any value from `1` to `15` turns it on at power `15`. This API does not provide analog input strength.
- `duration_ticks` is optional and, when supplied, must be a positive integer. An on input is released to power `0` after that many game ticks. Without a duration, it stays in its requested state until another action changes it. A duration on an off input has no effect.
- Each new input at the same position cancels the previous scheduled release, even if it sets the same state. This includes `use` and `plate` inputs. Inputs in one request are processed in array order, so the last action determines the control's state and any new release.
- Scheduled releases belong to the session and carry over between requests. Time advances only with simulation ticks, not wall-clock time. If an input is applied at tick `T` for `10` ticks, it is on for ticks `T+1` through `T+10` and is released at the boundary after tick `T+10`. Releases due at a boundary are processed before the response and before inputs from a subsequent request at that boundary. Simultaneous releases are processed in the order they were scheduled.
- A request with `ticks:0` applies inputs but does not advance or expire their durations. The server must split internal batches as needed to process releases at their exact boundaries, without advancing extra ticks.

For the example above, the input is on for the first 10 ticks and off for the remaining 6. The response reports tick `16` and, when requested, a final-state projection; intermediate transitions are not a separate event stream.

A successful response contains the new absolute session tick:

```json
{"type":"result","id":2,"session_id":"s1","tick":16}
```

All request fields, input positions, control types and scheduled release times must be validated before applying inputs. Invalid inputs reject the whole request without applying any of them. If execution fails after it has started, the server sends a session error; the session is no longer usable. It must not report success for a partially executed request.

Internal batching must not change the requested tick count. A request for 17 ticks finishes after exactly 17 ticks, regardless of how the engine groups its internal work.

## Read block changes

The client can request either view:

```json
{"id":3,"op":"project","session_id":"s1","kind":"baseline"}
{"id":4,"op":"project","session_id":"s1","kind":"changes"}
```

Both responses use the same representation:

```json
{"type":"result","id":4,"session_id":"s1","tick":16,"states":["minecraft:air","minecraft:redstone_lamp[lit=true]"],"changes":[[2,3,4,1],[5,1,2,0]]}
```

`states` is a palette local to that response. Each entry in `changes` is `[x,y,z,state_index]`; `state_index` indexes the response's `states` array. Coordinates are schematic-local. Each position appears at most once, in lexicographic `x,y,z` order. Block states use namespaced block names and properties.

**`baseline`** contains every block whose current state differs from the corresponding block in the **first schematic supplied to `begin`**, before simulation initialization or input actions. To reconstruct the current region, start from that original schematic and apply the returned entries. A baseline response also establishes the current full region as the reference for the next `changes` response.

**`changes`** contains every block whose current state differs from the last state sent to the client. Before any projection has been sent, the reference is the original schematic. A block that changed and then returned to its last-sent value is omitted. A successful `changes` response establishes the current full region as the next reference. Repeating it without intervening changes returns empty arrays.

For example, suppose a block was stone in the original schematic. After it becomes a lamp, `baseline` returns the lamp. If it then becomes stone again, `changes` returns stone, while a new `baseline` is empty because the block matches the original schematic.

An `advance` response with `projection:true` includes a `changes` projection and updates the same reference. An advance without projection does not update it. A client applying a later `baseline` must first restore its local view to the original schematic; applying that baseline directly on top of a previous view can leave reverted blocks stale.

These projections cover all changed block states in the original region, including when `--publish-io` is selected. The backend must obtain a complete block-state view when serving a projection. Projection is a final-state view, not an ordered event trace or a complete resumable snapshot of scheduled ticks, motion identities and other internal state.

The server updates the last-sent reference only after it has successfully written the complete projection response to the socket. If the connection fails, the session is released; protocol 4 does not resume sessions across connections.

## Close a session

```json
{"id":5,"op":"close","session_id":"s1"}
```

After the session has been released:

```json
{"type":"result","id":5,"session_id":"s1","state":"closed"}
```

The connection remains open for other sessions. Closing a session during compilation requests cancellation. The server sends the active `begin` or `recompile` request's terminal `error` with code `cancelled`, then sends the `close` result after the worker has stopped and the session has been released.

## Errors

An error is a JSON message with a machine-readable code and a human-readable message:

```json
{"type":"error","id":2,"session_id":"s1","code":"invalid_input","message":"Position is outside the schematic."}
```

`id` is echoed when the failed ordinary request supplied it. `session_id` is included when a session has been allocated or identified. Example codes include `unsupported_protocol`, `unsupported_operation`, `invalid_request`, `invalid_settings`, `invalid_schematic`, `invalid_input`, `unknown_session`, `session_busy`, `resource_limit`, `compile_failed`, `cancelled` and `execution_failed`.

A rejected request that has not started execution leaves its session usable. Failure of the initial `begin` compilation or of simulation execution ends the affected session. A failed or cancelled `recompile` leaves the existing session usable, unless the session is being closed or its connection has disconnected. Other sessions on the same connection remain usable. Errors for malformed framing or invalid JSON may require closing the connection when no request can be identified. If a projection cannot be written completely, the connection closes and its sessions are released.

Compilation runs asynchronously relative to the socket reader so progress and cancellation remain possible during long compilations. The protocol does not define automatic retries or request deduplication: reconnecting and sending `begin` again creates a new session.
