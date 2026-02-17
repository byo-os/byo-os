# BYO/OS for D-Bus Developers

D-Bus serves as the standard message bus for the Linux desktop, routing method
calls and signals through a central daemon. BYO/OS adopts this centralized
hub-and-spoke topology but shifts the interaction model. Instead of imperative
RPC (Remote Procedure Calls), BYO/OS processes coordinate via shared,
declarative state. This page maps D-Bus concepts to their BYO/OS equivalents.

## The Bus

Both use a hub-and-spoke topology with a central router.

| D-Bus | BYO/OS |
|---|---|
| `dbus-daemon` / `dbus-broker` | Orchestrator |
| Unix socket connection | stdin/stdout pipes |
| Binary wire format | Text (APC escape sequences) |
| System bus + session bus | Single bus per session |

D-Bus clients connect via a Unix socket. BYO/OS clients are child
processes that communicate over standard I/O.

## Objects

Both systems have objects that live on the bus.

In D-Bus, objects live at path-style addresses
(`/org/freedesktop/NetworkManager`) and expose typed interfaces with
methods, signals, and properties. Interaction is imperative: call a
method, get a return value.

In BYO/OS, objects live in a parent-child tree, identified by qualified
IDs (`app:sidebar`). They carry string properties and are mutated
declaratively with `+` (upsert), `@` (patch), and `-` (destroy).

```byo
// D-Bus: invoke an operation
dbus-send --print-reply --dest=org.mpris.MediaPlayer2.spotify \
  /org/mpris/MediaPlayer2 org.mpris.MediaPlayer2.Player.PlayPause

// BYO/OS: declare state
printf '\e_B @media player playing \e\\'
```

D-Bus's imperative model fits service APIs where you invoke discrete
operations. BYO/OS's declarative model fits persistent UI state. The
trade-off: declarative state is harder to use for one-shot operations;
imperative RPC is harder to use for synchronized views of shared state.

## Properties

D-Bus has a rich type system: booleans, integers, strings, arrays, dicts,
variants, file descriptors. Properties are typed and introspectable.

BYO/OS properties are currently untyped strings. This keeps the protocol
simple and language-agnostic but pushes validation to endpoints. A schema
system is planned.

## Signals → Events

D-Bus **signals** are typed publish-subscribe messages. They are
fire-and-forget — no acknowledgment.

The BYO/OS equivalent is **events** (`!click 0 save`). Events carry
string properties and require acknowledgment
(`!ack click 0 handled=true`). The ACK tells the sender whether the
event was consumed (for bubbling) and lets the orchestrator detect
unresponsive apps.

## Service Activation → Daemon Expansion

D-Bus supports **auto-activation**: sending a message to an inactive bus
name can start the owning service on demand.

BYO/OS does not currently activate daemons on demand. Daemons must be
running before apps create objects of their types. The orchestrator
buffers commands for late-starting daemons and replays on connection,
but does not spawn daemons itself.

Where the two differ is what happens after the service is available.
D-Bus delivers the message directly — the client knows who it's talking
to. In BYO/OS, the orchestrator rewrites the tree: it sends `?expand`
to the daemon, receives a subtree of renderable objects, and splices it
in before delivering to observers.

```byo
// App sends:
+button save label="Save"

// Orchestrator sends to daemon:
?expand 0 app:save kind=button label=Save

// Daemon responds:
.expand 0 {
  +view save-root class="btn" {
    +text save-label content="Save"
  }
}

// Compositor receives (after rewrite):
+view controls:save-root class="btn" {
  +text controls:save-label content="Save"
}
```

D-Bus's model is simpler and more transparent. BYO/OS's model lets the
orchestrator provide crash recovery and re-expansion without app
involvement.

## State

D-Bus is stateless from the bus daemon's perspective. If a service
crashes, callers must re-establish state.

The orchestrator maintains **reduced state** — the latest upsert merged
with all subsequent patches for every object. When a daemon restarts,
the orchestrator replays objects as `?expand` requests. Apps don't need
to do anything.

## Concept Map

| D-Bus | BYO/OS |
|---|---|
| Bus name | Process name |
| Object path | Qualified ID (`client:local-id`) |
| Interface | Object kind (freeform string) |
| Method call | Request (`?kind seq target`) |
| Method return | Response (`.kind seq props`) |
| Signal | Event (`!kind seq target props`) |
| Property | Prop (`key=value`) |
| Match rules | Observer subscriptions (`?observe`) |
| Auto-activation | Late-daemon replay (partial equivalent) |

BYO/OS can coexist with D-Bus — use D-Bus for non-UI services (network,
power), BYO/OS for the UI tree.
