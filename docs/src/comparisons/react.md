# BYO/OS for React Developers

React popularized the pattern of declarative UI, where a runtime reconciles the
difference between a desired state and the actual DOM. BYO/OS applies this
architecture at the system level: the Orchestrator acts as the reconciler,
merging state updates from various processes into a single cohesive tree.
This page maps React concepts to BYO/OS.

## Declarative UI

Both React and BYO/OS use a declarative model. You describe the desired
state; the runtime applies it.

```jsx
// React: declare a component tree
<div className="sidebar">
  <span>Hello</span>
</div>
```

```byo
// BYO/OS: declare an object tree
+view sidebar class="sidebar" {
  +text label content="Hello"
}
```

In React, JSX compiles to `createElement` calls that produce a virtual
DOM. In BYO/OS, you write protocol commands directly — there is no
virtual layer. Commands go to the orchestrator, which maintains the
actual tree.

## Components → Daemons

React components encapsulate rendering logic. A `<Button>` component
decides what DOM elements to produce.

BYO/OS daemons fill a similar role. A daemon claims a type (e.g.
`button`) and expands it into renderable primitives when any app creates
an object of that type.

```jsx
// React: Button component renders DOM elements
function Button({ label }) {
  return (
    <div className="px-4 py-2 rounded bg-blue-500">
      <span className="text-white">{label}</span>
    </div>
  );
}
<Button label="Save" />
```

```byo
// BYO/OS: app creates a button
+button save label="Save"

// Controls daemon expands it into views
.expand 0 {
  +view save-root class="px-4 py-2 rounded bg-blue-500" {
    +text save-label content="Save" class="text-white"
  }
}
```

The difference: React components run in the same process as the app.
BYO/OS daemons are separate processes. The orchestrator sends `?expand`
requests to the daemon and splices the result into the tree.

## Props

React props are typed JavaScript values — strings, numbers, objects,
functions, anything. They flow top-down from parent to child.

BYO/OS props are string key-value pairs. `label="Save"` and `order=0`
are both strings. A schema system is planned.

```jsx
// React: props are typed JS values
<Button label="Save" onClick={handleSave} disabled={false} />
```

```byo
// BYO/OS: props are strings
+button save label="Save"
@button save disabled
```

React props include callback functions for events. BYO/OS separates
events from props — events are a distinct protocol mechanism (`!`).

## State and Re-rendering

React re-renders components when state changes. The reconciler diffs
the virtual DOM and applies minimal DOM updates.

BYO/OS has no virtual layer. Apps send explicit mutations:
- `+` replaces all props (full state)
- `@` patches specific props (incremental update)
- `-` destroys an object

```jsx
// React: state change triggers re-render + diff
const [label, setLabel] = useState("Save");
setLabel("Saved!"); // React diffs and patches the DOM
```

```byo
// BYO/OS: explicit patch
@button save label="Saved!"
```

When a daemon-owned object is patched, the orchestrator re-expands it
and reconciles the old and new expansion, producing minimal updates for
the compositor. This is conceptually similar to React's reconciliation,
but happens in the orchestrator rather than in-process.

## Events

React uses synthetic events that wrap browser events. Event handlers are
props (functions passed to components).

BYO/OS events are protocol messages. The compositor does hit testing and
sends events to the orchestrator, which routes them to the owning
process. Apps acknowledge with a handling disposition for bubbling.

```jsx
// React: event handler as prop
<button onClick={(e) => { e.stopPropagation(); save(); }}>
  Save
</button>
```

```byo
// BYO/OS: compositor sends event, app acknowledges
!click 0 save
!ack click 0 handled=true
```

React events bubble synchronously within a process. BYO/OS events are
asynchronous and cross process boundaries.

## DOM → Object Tree

React targets the browser DOM — a tree of typed nodes (`div`, `span`,
`button`) with attributes and styles.

BYO/OS targets a protocol-level object tree — typed objects (`view`,
`text`, `button`) with string properties. The compositor renders the
tree via GPU. There is no browser, no CSS engine, no HTML parser.

| React / DOM | BYO/OS |
|---|---|
| `<div>` | `+view id` |
| `<span>text</span>` | `+text id content="text"` |
| `className="..."` | `class="..."` |
| `style={{ ... }}` | Props (convention, not CSS) |
| `ref` | Qualified ID (`app:id`) |

## Concept Map

| React | BYO/OS |
|---|---|
| Component | Daemon (for claimed types) |
| JSX element | Object command (`+kind id props`) |
| Virtual DOM | No equivalent (tree is the source of truth) |
| Reconciliation | Orchestrator re-expansion reconciliation |
| Props | Props (`key=value`, strings) |
| `useState` / `setState` | `@kind id` (patch) or `+kind id` (replace) |
| Synthetic event | Event (`!kind seq target`) |
| `onClick` handler | Event handler (app reads event, sends ACK) |
| `React.createElement` | Protocol command (`+kind id props`) |
| Context / Redux | No equivalent (no shared state between apps) |
| Server-side rendering | Centralized compositor rendering |
