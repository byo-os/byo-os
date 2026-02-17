# BYO/OS for Wayland Developers

Wayland compositors manage a tree of *surfaces* (windows and sub-surfaces), but
the content within them remains a black box of client-rendered pixels. BYO/OS
extends this managed state all the way down. Instead of stopping at the window
frame, the system manages the entire UI hierarchy—buttons, text, and layout are
all first-class compositor resources. This page maps Wayland concepts to BYO/OS.

## Rendering

In Wayland, each app renders its own window into a buffer (shared memory
or DMA-BUF). The compositor composites the buffers. Apps own their pixels.

In BYO/OS, apps describe UI as a declarative object tree. The compositor
renders all of it. Apps never touch pixels.

> Note: the BYO/OS compositor is not a "server" in the X11 sense. It's a
> peer process — a client of the orchestrator that subscribes to object
> types via `?observe`. The orchestrator is the hub; the compositor is a
> spoke, symmetrical with apps.

| Wayland | BYO/OS |
|---|---|
| App renders into buffer | App writes object tree commands |
| Compositor composites buffers | Compositor renders tree directly |
| Needs a toolkit (GTK, Qt) or manual rendering | `printf` to stdout |
| Full visual freedom per app | Constrained to tree semantics |

Wayland gives apps complete rendering freedom. The trade-off: each app
handles its own rendering, accessibility, and input methods.

BYO/OS gives the system control. The trade-off: apps cannot do arbitrary
pixel-level rendering through the tree. Kitty graphics commands provide
a separate channel for raster content.

## Protocol

Wayland is a binary protocol with typed objects and interfaces.
Interfaces define requests and events. New functionality requires XML
protocol extensions compiled into bindings.

BYO/OS is a text protocol with string IDs and freeform types. A small
fixed set of operations (`+`, `-`, `@`, `!`, `?`, `.`) applies to all
types. New types need no protocol extension.

```xml
<!-- Wayland: new capability requires an XML extension -->
<interface name="zwlr_layer_shell_v1" version="4">
  <request name="get_layer_surface">
    <arg name="surface" type="object" interface="wl_surface"/>
    ...
  </request>
</interface>
```

```byo
// BYO/OS: new type, no extension
+layer-surface root output=HDMI-1 layer=overlay anchor=top
```

Wayland provides compile-time type safety and introspection. BYO/OS
makes new types free but does not currently have type-level guarantees.
A schema system is planned.

## Surfaces → Object Tree

Wayland clients create **surfaces** — rectangular pixel buffers that the
compositor positions and composites.

BYO/OS apps create **objects in a tree**. The compositor receives this
tree (after daemon expansion) and renders it. There is no concept of a
surface that an app allocates.

Wayland apps can render anything: OpenGL, video, terminal emulators.
BYO/OS apps describe structure. Complex rendering (3D, video) requires
the Kitty graphics channel or a specialized object type.

## Input

Wayland delivers raw input (pointer coordinates, key codes, touch) to
the focused surface. The app's toolkit does hit testing.

BYO/OS delivers semantic events targeted at specific objects
(`!click 0 save`). The compositor does hit testing and routes events to
the owning process. Apps acknowledge with a handling disposition for
bubbling.

## Extensions → Object Types

Wayland adds capabilities via protocol extensions reviewed through
`wayland-protocols`. Cross-compositor compatibility, slow process.

BYO/OS adds capabilities by creating new types. First-party: bare names
(`view`). Third-party: dot-qualified (`org.mybrowser.WebView`). No
standardization process. The trade-off: no formal contract for what a
type supports yet.

## Network Transparency

Wayland has no built-in network transparency. Buffers use shared memory
or DMA-BUF. Remote display requires RDP, VNC, or PipeWire streaming.

BYO/OS is a text stream over stdin/stdout. SSH provides remote display
directly. The trade-off: buffer-sharing is more efficient for
high-frequency pixel updates.

## Concept Map

| Wayland | BYO/OS |
|---|---|
| `wl_surface` | Object in the tree (`+view id`) |
| `wl_buffer` / DMA-BUF | No equivalent (compositor owns rendering) |
| `wl_compositor` | Compositor process (observer, not hub) |
| Protocol extension (XML) | New object type (string) |
| `wl_seat` / input events | Semantic events (`!click`, `!keydown`) |
| `xdg_surface` / shell role | Object kind + props |
| Subsurfaces | Child objects (`{ ... }`) |
| `wl_callback` (frame) | Not applicable (no client rendering) |
