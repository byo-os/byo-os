# BYO/OS for AppKit Developers

AppKit relies on a strict View Hierarchy and a Responder Chain to organize UI
and route events. BYO/OS defines these structures within the system protocol
rather than the application runtime. The Orchestrator maintains the hierarchy
and routes events, allowing the responder chain to span process boundaries.
This page maps AppKit concepts to BYO/OS.

## View Hierarchy → Object Tree

AppKit builds UI as a tree of `NSView` instances. Each view has a
`superview` and an ordered list of `subviews`. Views handle their own
drawing via `draw(_:)`.

BYO/OS builds UI as a tree of typed objects. Each object has a parent
and ordered children. Objects do not draw themselves — the compositor
renders the entire tree.

```swift
// AppKit: build a view hierarchy
let sidebar = NSView()
sidebar.identifier = NSUserInterfaceItemIdentifier("sidebar")
let label = NSTextField(labelWithString: "Hello")
sidebar.addSubview(label)
root.addSubview(sidebar)
```

```byo
// BYO/OS: same structure
+view root {
  +view sidebar {
    +text label content="Hello"
  }
}
```

| AppKit | BYO/OS |
|---|---|
| `NSView` hierarchy | Object tree |
| `addSubview(_:)` | `+kind id { ... }` (nested children) |
| `removeFromSuperview()` | `-kind id` (cascade destroy) |
| `NSView.identifier` | Object ID (`sidebar`) |
| `NSView` subclass | Object kind (`view`, `button`) |

## Core Animation & Rendering

Traditional AppKit views use `draw(_:)` to paint pixels imperatively using
**Core Graphics** (Quartz 2D). However, modern AppKit relies on **Core Animation**
(`CALayer`), where views manage a retained tree of layers with properties
(opacity, transform, background color), and the **Quartz Window Server**
composites them.

BYO/OS acts like a system-wide Core Animation. Think of BYO/OS objects as
`CALayer`s that live in the Orchestrator. You don't implement `draw(_:)`;
you strictly update properties, and the BYO/OS Compositor (acting as the
Window Server) renders the frame.

| AppKit | BYO/OS |
|---|---|
| `draw(_:)` / Core Graphics | No equivalent (no imperative drawing) |
| `CALayer` tree | Object tree |
| Quartz Window Server | Compositor |
| `layer.backgroundColor = ...` | `@view id bg="..."` |

For raster content (images, custom software rendering), BYO/OS provides the
Kitty graphics channel—a separate protocol for uploading pixel buffers that
the Compositor manages as GPU textures (similar to setting `layer.contents`
with a `CGImage`).

## Properties and KVO

AppKit uses Objective-C properties with optional Key-Value Observing
(KVO). You can observe any KVO-compliant property and receive change
notifications.

BYO/OS properties are string key-value pairs set via the protocol.
Observers subscribe to object types (`?observe 0 view,text`) and
receive all mutations for those types. There is no per-property
observation — observers see all property changes on their subscribed
types.

```swift
// AppKit: set property, KVO notifies observers
button.title = "Save"
button.observe(\.title) { button, change in ... }
```

```byo
// BYO/OS: set property, observer receives the patch
+button save label="Save"
@button save label="New"
```

## Responder Chain → Event Bubbling

AppKit uses the **responder chain** for event handling. Events start at
the first responder and walk up through superviews, the window, and the
window controller until handled.

BYO/OS has event acknowledgment with a handling disposition. The
compositor sends events targeted at specific objects. Apps acknowledge
with `handled=true` (consumed, stop) or `handled=false` (bubble to
parent).

```swift
// AppKit: responder chain
override func mouseDown(with event: NSEvent) {
    // handle or call super to pass up the chain
    super.mouseDown(with: event)
}
```

```byo
// BYO/OS: event targeting and ACK
!click 0 save
!ack click 0 handled=true
```

AppKit's responder chain is synchronous and in-process. BYO/OS's event
routing is asynchronous and cross-process.

## Target-Action → Events

AppKit controls use target-action: a control sends an action message to
a target object when activated. `NSButton` sends its action on click.

BYO/OS uses events. The compositor (or daemon) sends `!click` to the
app when a button is activated. The app handles it and acknowledges.

```swift
// AppKit: target-action
button.target = self
button.action = #selector(save(_:))
```

```byo
// BYO/OS: event-based
!click 0 save
!ack click 0 handled=true
```

## NSWindowController → Orchestrator

AppKit's `NSWindowController` manages a window's lifecycle, coordinates
between the model and the view hierarchy, and handles nib loading.

The BYO/OS orchestrator manages all objects across all processes. It
tracks the tree, routes events, coordinates daemon expansion, and
handles crash recovery. It is more like a system-wide window server
than a per-window controller.

## Interface Builder / SwiftUI → Protocol Commands

Interface Builder (XIBs/Storyboards) and SwiftUI are declarative ways
to describe AppKit UI.

BYO/OS protocol commands serve a similar purpose — declarative
descriptions of a UI tree — but at the process boundary rather than
within a framework.

```swift
// SwiftUI
VStack {
    Text("Hello")
        .font(.title)
    Button("Save") { save() }
}
```

```byo
// BYO/OS
+view root {
  +text label content="Hello" class="text-2xl"
  +button save label="Save"
}
```

SwiftUI runs in-process with a rich type system, layout engine, and
animation support. BYO/OS commands are text over stdin/stdout with
string properties.

## Concept Map

| AppKit | BYO/OS |
|---|---|
| `NSView` hierarchy | Object tree |
| `NSView` subclass | Object kind (`view`, `text`) |
| `NSView.identifier` | Object ID (qualified: `app:sidebar`) |
| `draw(_:)` | No equivalent (compositor renders) |
| KVO property observation | Observer subscriptions (`?observe`) |
| Responder chain | Event ACK bubbling (`handled=true/false`) |
| Target-action | Event (`!click seq target`) |
| `NSWindowController` | Orchestrator (system-wide) |
| Interface Builder / SwiftUI | Protocol commands (`+kind id props`) |
| `NSApplication` | Orchestrator process management |
| `removeFromSuperview()` | `-kind id` (cascade destroy) |
| Cocoa Bindings | No equivalent (explicit mutations) |
