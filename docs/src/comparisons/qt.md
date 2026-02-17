# BYO/OS for Qt / QObject Developers

Qt applications are structured as trees of `QObject` instances that communicate
via signals and slots. BYO/OS uses this same topology but distributes it. The
Orchestrator manages the hierarchy, and signals flow between processes rather
than just between threads. This page maps Qt concepts to BYO/OS.

## QObject Tree → Object Tree

QObject has a parent-child ownership model. Setting a parent transfers
ownership — deleting a parent deletes all its children. Children are
ordered; you can enumerate them with `findChildren`.

BYO/OS has the same pattern. Objects live in a tree with parent-child
relationships. Destroying an object cascades to all descendants. The
orchestrator maintains the tree.

```cpp
// Qt: create parent-child hierarchy
auto *sidebar = new QWidget(root);
sidebar->setObjectName("sidebar");
auto *label = new QLabel("Hello", sidebar);
```

```byo
// BYO/OS: same structure
+view root {
  +view sidebar {
    +text label content="Hello"
  }
}
```

| Qt | BYO/OS |
|---|---|
| `QObject` parent-child tree | Object tree in orchestrator |
| `delete parent` cascades | `-kind id` cascades |
| `setObjectName("sidebar")` | ID is positional: `+kind sidebar` |
| `findChildren<T>()` | `objects_of_type("kind")` (orchestrator API) |

The key difference: Qt's tree lives in a single process. BYO/OS's tree
lives in the orchestrator and spans processes.

## Properties

Qt properties use the `Q_PROPERTY` macro with a type, getter, setter,
and optional `NOTIFY` signal. The meta-object system (MOC) generates
introspection data at compile time. Properties can be accessed
dynamically via `setProperty`/`property`.

BYO/OS properties are currently untyped strings set via the protocol.
There is no compile-time metadata. A schema system is planned.

```cpp
// Qt: typed property with change notification
Q_PROPERTY(QString label READ label WRITE setLabel NOTIFY labelChanged)
```

```byo
// BYO/OS: string property, no type declaration
+button save label="Save"
@button save label="New"
```

Qt properties support bindings (especially in QML). BYO/OS properties
are set explicitly — there is no binding system.

## Signals/Slots → Events

Qt's signal/slot mechanism is typed, works across threads (with queued
connections), and supports multiple connections per signal. Slots are
regular methods. MOC generates the dispatch code.

BYO/OS events are protocol messages between processes. They carry string
properties and require acknowledgment.

```cpp
// Qt: connect signal to slot
connect(button, &QPushButton::clicked, this, &App::onSave);
```

```byo
// BYO/OS: compositor sends event to app, app acknowledges
!click 0 save
!ack click 0 handled=true
```

Qt signals are synchronous by default (direct connection) or queued.
BYO/OS events are always asynchronous (cross-process).

## QML → BYO/OS Protocol

QML is Qt's declarative UI language. You describe a tree of elements
with properties, and the Qt Quick runtime renders it. This is the
closest conceptual match to BYO/OS's protocol.

```qml
// QML
Rectangle {
    id: sidebar
    width: 256
    color: "#27272a"
    Text {
        id: label
        text: "Hello"
        color: "white"
    }
}
```

```byo
// BYO/OS
+view sidebar class="w-64 bg-zinc-800" {
  +text label content="Hello" class="text-white"
}
```

Both describe a tree of typed objects with properties. The differences:

- QML runs in-process; BYO/OS commands cross process boundaries
- QML has JavaScript expressions, bindings, and animations; BYO/OS has
  static property values
- QML types are backed by C++ classes; BYO/OS types are freeform strings
  interpreted by daemons and the compositor
- QML's scene graph is internal to the app; BYO/OS's tree is shared
  across all processes

## Qt Quick Scene Graph → Compositor

Qt Quick uses a retained-mode **scene graph** rendered via OpenGL or
Vulkan. The scene graph is a tree of nodes (geometry, transform,
opacity) mapped from the QML element tree.

BYO/OS's compositor receives a similar structure — a tree of typed
objects — and renders it via GPU (Bevy/wgpu). The difference is scope:
Qt Quick's scene graph is per-application. BYO/OS's tree spans the
entire desktop.

## MOC → (Planned) Schema

Qt's Meta-Object Compiler generates introspection data: property types,
signal signatures, enum values. This enables `QMetaObject::invokeMethod`,
the property system, and QML integration.

BYO/OS does not currently have a meta-object system. Object types and
their accepted properties are defined by convention. A schema system is
planned.

## Concept Map

| Qt | BYO/OS |
|---|---|
| `QObject` tree | Object tree |
| `Q_PROPERTY(Type name ...)` | Prop (`key=value`, untyped) |
| `SIGNAL(clicked())` | Event (`!click seq target`) |
| `SLOT(onSave())` | Event handler (app reads event, sends ACK) |
| `Q_INVOKABLE` method | Request (`?kind seq target`) |
| QML element tree | Object tree commands (`+kind id { ... }`) |
| Qt Quick scene graph | Compositor's view of the tree |
| `QObject::deleteLater` | `-kind id` (immediate cascade) |
| MOC metadata | Not yet (schema planned) |
| `QAbstractItemModel` | Observer projection (type-filtered tree) |
| `qmlRegisterType` | `?claim seq type` (daemon claims a kind) |
