# BYO/OS for AT-SPI Developers

AT-SPI requires applications to maintain a parallel "accessibility tree"
alongside their visual widget tree. BYO/OS removes this requirement. Because
the primary UI is defined declaratively in the Orchestrator, the rendering tree
*is* the accessibility tree, removing the need for synchronization. This page
maps AT-SPI concepts to BYO/OS.

## Architecture

AT-SPI exposes UI as a tree of accessible objects over D-Bus. Each
application maintains an **accessibility tree** — a parallel
representation of its widget tree as `AtkObject` entries (ATK in GTK,
`QAccessibleInterface` in Qt). A registry service (`at-spi2-registryd`)
coordinates between applications and assistive technology clients.

```
App (GTK)        AT-SPI Registry       Screen Reader (Orca)
   |                    |                       |
   |-- publishes tree ->|                       |
   |                    |<-- subscribes --------|
   |                    |--- forwards events -->|
   |<-- queries --------|<-- queries -----------|
```

BYO/OS has a single authoritative tree in the orchestrator. Observers
subscribe to object types (`?observe 0 view,text`) and receive a
projected view filtered to those types.

```
App              Orchestrator           Daemon           Compositor
 |                    |                   |                   |
 |-- +button save --->|                   |                   |
 |                    |-- ?expand ------->|                   |
 |                    |<-- .expand -------|                   |
 |                    |-- projected view (view,text only) --->|
```

| AT-SPI | BYO/OS |
|---|---|
| Per-app accessibility tree | Single orchestrator tree |
| `at-spi2-registryd` | Orchestrator |
| D-Bus transport | BYO protocol over APC |
| Event subscription (focus, text-changed) | Type subscription (`?observe`) |

## Tree Maintenance

AT-SPI requires each application to maintain both a widget tree (for
rendering) and an accessibility tree (for AT-SPI). Keeping these in
sync is error-prone. Toolkits handle it for standard widgets, but custom
widgets often have broken accessibility because developers must maintain
the parallel tree manually.

BYO/OS centralizes this problem. Each observer receives a projected
view that differs in shape from the app's original tree, but the
orchestrator computes and delivers projections automatically whenever
the source tree changes. Apps and daemons don't maintain a separate
tree for each consumer.

This makes tree sync a system responsibility rather than a per-app
burden. It does not eliminate the problem; it centralizes it.

## Projection

AT-SPI exposes the full accessibility tree to all consumers. Consumers
query and filter as needed.

BYO/OS observers receive type-filtered projections. If an observer
subscribes to `view` and `text`, it receives only those types.
Non-observed types are transparent: if a `button` (not observed)
contains a `text` (observed), the observer receives the `text`
re-parented under the nearest observed ancestor.

## Events

AT-SPI events are accessibility-specific: `focus:`,
`object:state-changed:checked`, `object:text-changed:insert`. They
carry payloads designed for screen readers.

BYO/OS events are general-purpose input: `!click`, `!keydown`, `!focus`.
They track focus and input flow. A production accessibility layer would
need additional event types or property conventions for things like live
region announcements and text selection.

## Performance

AT-SPI tree traversal involves D-Bus round-trips. Scanning the full tree
(e.g., find-by-name) can generate thousands of calls. Caching and batch
APIs mitigate this.

BYO/OS observers receive push-based updates. The observer always has an
up-to-date local copy — no polling or querying. The trade-off is that
each observer maintains state proportional to its projected tree size.

> Note: AT-SPI's accessibility tree carries semantic information that
> BYO/OS does not currently standardize: roles (checkbox vs radio
> button), states (checked, expanded), relations (labelled-by), and text
> attributes (bold, font size). An accessibility consumer of BYO/OS
> objects would need conventions for these properties.

## Concept Map

| AT-SPI | BYO/OS |
|---|---|
| Accessibility tree | Object tree |
| `AtkObject` / `QAccessibleInterface` | Object (`+kind id props`) |
| Role (button, checkbox) | Kind string (`button`, `checkbox`) |
| State (checked, expanded) | Props (convention, not standardized) |
| `at-spi2-registryd` | Orchestrator |
| D-Bus events | BYO events (`!kind seq target`) |
| Full tree query | Type-filtered projection (`?observe`) |
| Per-app tree sync | Centralized projection (orchestrator) |
| `atspi_accessible_get_name` | Object prop lookup |
