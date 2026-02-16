# BYO/OS — Project Conventions

## Architecture

BYO/OS is a terminal-protocol-based desktop environment. All communication
happens via ECMA-48 APC escape sequences over stdin/stdout.

```
+----------+   stdout    +--------------+                   +------------+
|   App    |------------>| Orchestrator |------------------>| Compositor |
|          |<------------|   (router)   |<------------------|            |
+----------+   stdin     |              |   input events    +------------+
                         |              |
                         |              |--> Controls Daemon
                         |              |--> Text Daemon
                         |              |--> Media Daemon
                         +--------------+
```

- **Orchestrator** — launchd-like process manager + APC escape code router (the hub)
- **Compositor** — Bevy/wgpu GPU renderer (just another spoke)
- **Daemons** — System services (controls, text, media) that own rendering of their domain
- **Apps** — Userland programs in any language that can write to stdout

The orchestrator is the central hub. The compositor is symmetrical with apps
from the orchestrator's perspective — it's a client with surface_provider +
input_provider capabilities. SSH is the remote desktop protocol
(stdin/stdout = network transparency).

## APC Escape Sequence Protocol

Transport: APC sequences (`ESC _` ... `ST`), where ST is the two-byte
String Terminator `ESC \` (0x1B 0x5C).

```
ESC _ B <byo protocol payload> ST    -- BYO/OS protocol (B prefix)
ESC _ G <kitty graphics data> ST     -- Kitty graphics passthrough (G prefix)
```

Kitty graphics (`G`) commands are routed directly to the compositor,
out-of-band from the BYO/OS (`B`) protocol. They upload/manage image
data which the compositor holds as GPU-side resources. BYO/OS commands
can then reference these resources via props (e.g. as view backgrounds,
image fills, etc.). The two protocols are complementary: `G` manages
pixel data, `B` manages the object tree that uses it.

Multiple commands can be batched inside a single APC sequence.
Whitespace and newlines between commands are ignored, allowing
readable indentation.

### Object model

The protocol is **type-agnostic**. Object types are freeform strings (e.g.
`view`, `layer`, `window`, `button`). A standard set of operations applies
to all types:

#### Object operations

| Op  | Form          | Meaning                                               |
|-----|---------------|-------------------------------------------------------|
| `+` | `+type id|_`  | Create or update (idempotent upsert, _ for anonymous) |
| `-` | `-type id`    | Destroy (including children)                          |
| `{` | `{`           | Push (begin children of last `+`/`@`)                 |
| `}` | `}`           | Pop (end children context)                            |
| `@` | `@type id`    | Patch props / set context                             |

Type and ID are positional. Props follow as `key=value` pairs.
Use `_` as the ID for anonymous objects (cannot be updated, deleted,
or referenced — internally assigned a unique ID). Only `+` supports
anonymous objects; `-` and `@` require a named ID.

`+` is a **full replace** — all props are set to exactly what's
specified (idempotent). `@` is a **patch** — only mentioned props
are modified, everything else is left untouched. In `@`, bare
`key` or `key=value` sets a prop, `~key` removes a prop:

```
+view sidebar class="w-64" order=0      ← full replace
@view sidebar order=1                    ← patch: update order, keep class
@view sidebar hidden ~disabled           ← add boolean, remove prop
```

`{` implicitly targets the object from the preceding `+` or `@`
command — this ensures the target always exists before pushing into it.
`@` can combine patching with context setting: `@view foo hidden { ... }`.

Objects carry arbitrary key-value props. The protocol itself does not
prescribe any specific prop names — interpretation is up to the receiver
(compositor, daemon, etc.). Visual attributes may be passed as props
on types the compositor handles directly (e.g. `class` on views).
Daemon-owned types like controls use semantic props (e.g. `label`,
`variant`) — the daemon decides how they look.

#### Events

| Op       | Form                | Meaning                             |
|----------|---------------------|-------------------------------------|
| `!`      | `!type seq id`      | Event (input, control, etc.)        |
| `!ack`   | `!ack type seq`     | Acknowledge a received event        |

Events flow in both directions. The compositor/daemons send input
events to apps (pointer, keyboard, focus, control interactions), and
apps ACK them.

Each side maintains its own incrementing sequence counter for
messages it sends. Sequence numbers are **namespaced per event type**
— `!click 0`, `!keydown 0`, etc. each have independent counters.
No global counter needed. ACKs reference the event type and sequence
number: `!ack click 0` is unambiguous.

ACK carries a handling disposition for event bubbling:
- `handled=true` — event was consumed, stop propagation
- `handled=false` — event was not handled, bubble to parent

#### Requests and responses

| Op  | Form                   | Meaning                             |
|-----|------------------------|-------------------------------------|
| `?` | `?claim seq type`      | Claim ownership of an object type   |
| `?` | `?unclaim seq type`    | Release claim on an object type     |
| `?` | `?observe seq type`    | Observe final output for a type     |
| `?` | `?unobserve seq type`  | Stop observing a type               |
| `?` | `?expand seq id`       | Request daemon expansion            |
| `?` | `?kind seq target`     | Generic/custom request              |
| `.` | `.expand seq { body }` | Expansion response with body        |
| `.` | `.kind seq props`      | Generic/custom response             |

Requests (`?`) and responses (`.`) handle system commands and
daemon interactions. `?claim`/`?unclaim`/`?observe`/`?unobserve`
are fire-and-forget (no response needed). `?expand` expects a
`.expand` response from the daemon.

**Two subscription modes:**

| Command | Mode | Meaning | Counterpart |
|---------|------|---------|-------------|
| `?claim seq type` | Expand | "I own this type — send me `?expand`" | `?unclaim seq type` |
| `?observe seq type` | Consume | "I consume final output for this type" | `?unobserve seq type` |

**Claim** is singular — only one daemon can claim a type at a time.
A claim means the daemon receives:
- All `?expand` requests when any app creates/updates that type
- All input events targeted at objects of that type
- Excludes events originating from the daemon itself

**Observe** is plural — multiple processes can observe the same type.
Observers receive the final expanded output. Use cases:
- Compositor observes `view`, `text`, `layer` → renders them
- Accessibility service observes `view`, `text` → builds a11y tree

For example, the controls daemon at startup:
```
\e_B ?claim 0 button ?claim 1 slider ?claim 2 checkbox \e\
```

After this, any `+button` from any app triggers a `?expand` to
the controls daemon.

`?expand` carries the qualified ID and original props (e.g.
`?expand 0 notes-app:save kind=button label="Save"`). The daemon
responds with a grammar-scoped `.expand` containing compositor-native
commands:

```
\e_B
  .expand 0 {
    +view save-root class="inline-flex px-4 py-2 rounded bg-blue-500" {
      +text save-label content="Save" class="text-white"
    }
  }
\e\
```

#### Reserved event and request names

Unqualified names are reserved for BYO/OS built-ins.
Third-party events/requests must use dot-qualified names (same rule
as type names).

Known built-in events parse as keywords with typed fields in the
`byo` library. Unknown events parse into a generic form, keeping
the protocol extensible.

**Input events (`!`):**
- `click`, `keydown`, `keyup`, `pointer`, `scroll`
- `focus`, `blur`, `resize`

**Event responses (`!`):**
- `ack` — acknowledge a received event

**Requests (`?`):**
- `claim` — claim ownership of an object type
- `unclaim` — release claim on an object type
- `observe` — observe final output for a type
- `unobserve` — stop observing a type
- `expand` — request daemon expansion

**Responses (`.`):**
- `expand` — expansion response with body

**Future (reserved):**
- `drag`, `drop`, `touch`, `gesture`, `paste`, `ime`

The orchestrator uses outstanding ACKs for **liveness detection** —
if an app stops ACKing events within a timeout, it is considered
unresponsive.

#### Orchestrator frame coordination

When an app sends commands involving daemon-owned types (e.g.
`+button`), the orchestrator cannot pass them directly to the
compositor — the daemon must expand them into renderable primitives
first.

The orchestrator handles this by **buffering the batch, expanding
daemon types, and rewriting** before sending to the compositor.
The compositor only ever sees native types (`view`, `layer`, `text`).

**Full flow:**

1. App (`notes-app`) sends a batch:
   ```
   +view sidebar {
     +view child1
     +button save label="Save"
     +view child3
   }
   ```
2. Orchestrator buffers the batch, recognizes `button` routes to
   the controls daemon
3. Orchestrator sends `?expand 0 notes-app:save kind=button label="Save"`
   to the controls daemon
4. Controls daemon responds with a grammar-scoped expansion:
   ```
   .expand 0 {
     +view save-root class="inline-flex px-4 py-2 rounded bg-blue-500" {
       +text save-label content="Save" class="text-white"
     }
   }
   ```
5. Orchestrator **rewrites** the batch, substituting the button
   in-place with the daemon's expansion (properly namespaced):
   ```
   +view notes-app:sidebar {
     +view notes-app:child1
     +view controls:save-root class="inline-flex px-4 py-2 rounded bg-blue-500" {
       +text controls:save-label content="Save" class="text-white"
     }
     +view notes-app:child3
   }
   ```
6. Rewritten batch is flushed to the compositor

The orchestrator tracks a **refcount per batch** (WaitGroup pattern).
Each daemon-bound command increments it; each `.expand` response
decrements it. When the count reaches zero, the rewrite is performed
and the batch is flushed. Daemons can trigger further daemon work
(e.g. controls → text daemon), which increments the count again
before it decrements — naturally handling nested expansion.

**Scoping and ordering:** Each APC sequence is an independent
batch/transaction. Only `B` batches containing daemon-owned types
need expansion; batches with only compositor-native types need no
rewriting. However, **all output from the same client** is delivered
to the compositor in order — this includes `B` batches, `G` (kitty
graphics) commands, and plain text/VT100 output. If a `B` batch is
waiting for daemon expansion, all subsequent output from that client
is held until the expansion completes and the batch is flushed.
This ensures the compositor never sees a graphics upload or text
update that was meant to follow a not-yet-expanded UI change.
Different apps' output streams are independent and do not block
each other.

#### State reduction and crash recovery

The orchestrator maintains a **reduced state** for every object: the
latest `+` merged with all subsequent `@` patches, producing the
equivalent of a single `+` with the final props. This enables:

**Daemon crash recovery:**
1. Daemon disconnects — orchestrator removes all `daemon:*` nodes
   from the compositor
2. Daemon restarts, re-claims types via `?claim`
3. Orchestrator replays the reduced state for all objects of the
   daemon's claimed types as `?expand` requests
4. Daemon re-expands everything, compositor gets fresh subtrees

**Late daemon startup:** Apps can create daemon-owned types before
the daemon has connected. The orchestrator buffers the commands.
When the daemon subscribes, it receives the full replay. The UI
may tear briefly (compositor-native content appears before daemon
chrome) but this is graceful degradation, not failure.

**App crash recovery:** The orchestrator holds the reduced state
and can replay it to a restarted app or a newly connected
compositor.

The idempotent `+` design makes replay lossless — the same reduced
command always produces the same result regardless of prior state.

#### Lexical rules

| Token         | Pattern                      | Examples                                  |
|---------------|------------------------------|-------------------------------------------|
| Type name     | `[a-zA-Z][a-zA-Z0-9._-]*`    | `view`, `button`, `org.mybrowser.WebView` |
| ID (local)    | `[a-zA-Z][a-zA-Z0-9_-]*`     | `sidebar`, `item1`, `item1-label`         |
| ID (qualified)| `client:id`                  | `notes-app:save`, `controls:save-bg`      |
| Prop name     | `[a-zA-Z][a-zA-Z0-9_-]*`     | `class`, `content`, `order`               |
| Prop value    | unquoted, `"..."`, or `'...'`| `0`, `move`, `"px-4 py-2"`, `'hi'`        |
| Sequence num  | `[0-9]+`                     | `0`, `12`, `1042`                         |

Unquoted prop values match `[^\s{}="'~\\]+`. Single and double quotes
are interchangeable. Quoted strings support backslash escape sequences
(see [`GRAMMAR.md`](GRAMMAR.md)). Bare values do not support escaping.

Unqualified type names (no dots) are reserved for BYO/OS built-in
types — those shipped with the compositor and first-party daemons
(e.g. `view`, `layer`, `text`, `button`, `slider`). Third-party
daemons must use dot-qualified names with a reverse-domain convention
(e.g. `org.mybrowser.WebView`). IDs and prop names do not use dots.

#### Formal grammar

See [`GRAMMAR.md`](GRAMMAR.md) for the complete PEG grammar defining
the command language within APC payloads.

#### ID scoping

IDs are **local per client** (per process/connection). Apps use bare
IDs (`save`, `sidebar`). The orchestrator assigns each client a name
at connection time and qualifies IDs internally.

Daemons use **qualified IDs** (`client:id`) to reference cross-client
objects. The tree freely mixes nodes from different clients — client
namespaces control **ownership**, not tree position:

```
notes-app:sidebar              ← app owns
  notes-app:save-btn           ← app owns (the +button)
    controls:save-bg           ← controls daemon owns
      controls:save-label      ← controls daemon owns
```

Each client can only mutate nodes in its own namespace (enforced by
the orchestrator). Nested colons (`a:b:c`) are reserved for future
use.

### Examples

Conceptual wire format (`\e_` = `ESC _`, `\e\` = ST).

A view with a text child:

```
\e_B
  +layer content order=0
  +view greeting class="p-4" {
    +text label content="Hello, world" class="text-2xl text-white"
  }
\e\
```

Sidebar with nested children:

```
\e_B
  +view sidebar class="w-64 h-full bg-zinc-800" {
    +view item1 class="px-4 py-2" {
      +text item1-label content="Network"
    }
    +view item2 class="px-4 py-2" {
      +text item2-label content="Display"
    }
    +view item3 class="px-4 py-2" {
      +text item3-label content="Sound"
    }
  }
\e\
```

Controls (rendered by the controls daemon — apps pass semantic props only):

```
\e_B
  +button save label="Save"
  +button cancel label="Cancel"
\e\
```

Destroying an object (also removes its children):

```
\e_B -view item3 \e\
```

Patching props on an existing object:

```
\e_B @view sidebar disabled ~tooltip \e\
```

Appending children to an existing object later:

```
\e_B
  @view sidebar {
    +view item4 class="px-4 py-2" {
      +text item4-label content="Storage"
    }
  }
\e\
```

Events (compositor/daemon → app):

```
\e_B
  !click 0 save
  !keydown 0 key="a" mod="ctrl"
  !pointer 5 content x=120 y=45 type=move
  !focus 0 sidebar
  !resize 0 surface width=1280 height=720
\e\
```

App ACKs:

```
\e_B
  !ack keydown 0 handled=true
  !ack click 0 handled=false
\e\
```

Daemon claims (fire-and-forget, no response needed):

```
\e_B ?claim 0 button ?claim 1 slider ?claim 2 checkbox \e\
```

Expansion request (orchestrator → daemon):

```
\e_B ?expand 0 notes-app:save kind=button label="Save" \e\
```

Expansion response (daemon → orchestrator, grammar-scoped body):

```
\e_B
  .expand 0 {
    +view save-root class="inline-flex px-4 py-2 rounded bg-blue-500" {
      +text save-label content="Save" class="text-white"
    }
  }
\e\
```

### Parsing layering

- `byo` library: intercepts APC sequences, routes BYO/OS protocol messages
- `vte` crate: handles VT100 content within surfaces (text, colors, cursor)

## Directory Structure

```
libs/       — Shared libraries (byo escape code lib, multi-language)
system/     — Core infrastructure (compositor, orchestrator)
services/   — System daemons (controls, text, menu, window manager)
apps/       — Userland applications (any language)
```

### Tier definitions

- **system/**: Core infrastructure the DE cannot function without
- **services/**: Daemons that enhance the experience; DE can boot without them
- **libs/**: Shared libraries across languages
- **apps/**: Userland applications in any language

## Package Naming

All Rust crates use the `byo-` prefix (e.g. `byo-compositor`, `byo-orchestrator`,
`byo-controls`), except the core library which is just `byo`.

## Key Design Principles

- **Language-agnostic**: Any language that can write to stdout is a first-class GUI citizen
- **Protocol-agnostic object model**: No hardcoded object types — `view`, `layer`, `window` are conventions, not protocol primitives
- **Zero-copy routing**: The orchestrator routes raw bytes without parsing content
- **Terminal-first**: The protocol IS the SDK — no separate framework needed
- **Compositor is a spoke**: Not a privileged monolith; multiple compositors can coexist
- **Daemons own rendering**: Controls daemon renders all buttons system-wide (consistency, a11y, theming)
