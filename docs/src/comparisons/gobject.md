# BYO/OS for GObject / GTK Developers

GObject functions as an object-oriented extension system on top of C, defining
classes, interfaces, and signals within a process address space. BYO/OS applies
this model to the system. Objects live in a shared tree managed by the
Orchestrator, allowing properties and signals to cross process boundaries
transparently. This page maps GObject concepts to BYO/OS.

## Objects

GObject (part of GLib) provides a C-based runtime type system with single
inheritance, interfaces, reference counting, typed properties, and signals.
A `GtkButton` is a GObject instance in your process's address space.

BYO/OS objects are protocol-level entries in a shared tree maintained by
the orchestrator. They cross process boundaries: an app creates one, the
orchestrator tracks it, a daemon expands it, the compositor renders it.

| GObject | BYO/OS |
|---|---|
| In-process instance (`GtkButton *`) | Object in shared tree (`+button save`) |
| Class hierarchy (`GtkButton → GtkWidget → ...`) | Freeform kind string (`button`) |
| `g_object_set(obj, "label", "Save", NULL)` | `+button save label="Save"` |
| Reference-counted lifetime | Orchestrator-managed lifetime |
| Identity via pointer | Identity via qualified ID (`app:save`) |

## Properties

GObject properties are strongly typed. A `GtkButton`'s `label` is a
`gchararray` — setting it to an integer is a type error. Properties
support validation, defaults, change notification (`notify::label`), and
bidirectional bindings.

BYO/OS properties are currently untyped strings. `label="Save"` and
`order=0` both produce string values. A schema system is planned.
Property changes propagate through the patch mechanism (`@`); observers
receive updates for object types they subscribe to.

```c
// GObject: set property in-process
g_object_set(button, "label", "Save", NULL);
```

```byo
// BYO/OS: set property across process boundary
+button save label="Save"
@button save label="New"
```

## Signals → Events

GObject signals are typed pub-sub within a process. You define a signal
with argument types; handlers receive typed values. Emission is
synchronous by default.

BYO/OS events are protocol messages between processes. They carry string
properties and require acknowledgment.

```c
// GObject: connect signal handler in-process
g_signal_connect(button, "clicked", G_CALLBACK(on_save), NULL);
```

```byo
// BYO/OS: compositor sends event to app, app acknowledges
!click 0 save
!ack click 0 handled=true
```

GObject signal emission is synchronous. BYO/OS events are always
asynchronous (cross-process).

## Methods → Requests

GObject classes define methods — virtual functions called directly in
process. `gtk_button_set_label(button, "Save")` is synchronous.

BYO/OS has no methods. The protocol provides requests (`?`) and
responses (`.`) for interactions beyond declarative state:

```byo
// Daemon claims a type
?claim 0 button

// Orchestrator asks daemon to expand an object
?expand 0 app:save kind=button label=Save

// Daemon responds with renderable subtree
.expand 0 {
  +view save-root class="btn" {
    +text save-label content="Save"
  }
}
```

GObject method calls are synchronous and typed. BYO/OS requests are
asynchronous messages with string properties.

## Inheritance

GObject inheritance is central to GTK. `GtkButton` inherits from
`GtkWidget`, which inherits from `GInitiallyUnowned`, which inherits
from `GObject`. Each level adds behavior, properties, and signals.

BYO/OS has no type hierarchy. A `button` and a `view` are different
kind strings with no structural relationship. What a type means is
determined by which daemon claims it. Common functionality comes from
convention (shared property names) rather than inheritance.

## GIR → (Planned) Schema

GIR files describe the full GObject API in XML, enabling automatic
binding generation for Python, JavaScript, Rust, Vala, and others.

BYO/OS achieves language-agnosticism differently: the protocol is text
over stdin/stdout. Any language that can write to a file descriptor can
participate. There is not yet a machine-readable type description; a
schema system is planned.

## Concept Map

| GObject / GTK | BYO/OS |
|---|---|
| `GObject` instance | Object in tree |
| Class hierarchy | Kind string (no hierarchy) |
| `GParamSpec` property | Prop (`key=value`, untyped) |
| `g_signal_emit` | Event (`!kind seq target`) |
| Signal handler | Event handler (app reads event, sends ACK) |
| Virtual method | Request (`?kind seq target`) |
| Method return value | Response (`.kind seq props`) |
| `g_object_set` | `+kind id props` or `@kind id props` |
| `g_object_unref` / destroy | `-kind id` (cascade) |
| GIR introspection | Not yet (schema planned) |
| GObject in-process | Object tree cross-process |
