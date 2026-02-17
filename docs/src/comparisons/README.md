# BYO/OS for...

If you're coming from an existing UI technology, these pages map the
concepts you already know to their BYO/OS counterparts. Start with the
one closest to your background.

| If you know... | Read this | You'll learn about... |
|---|---|---|
| [D-Bus](./dbus.md) | BYO/OS for D-Bus Developers | IPC, message routing, service activation |
| [Wayland](./wayland.md) | BYO/OS for Wayland Developers | Display protocol, rendering, surfaces |
| [X11](./x11.md) | BYO/OS for X11 Developers | Centralized rendering, resources, network transparency |
| [GObject / GTK](./gobject.md) | BYO/OS for GObject Developers | Type system, properties, signals |
| [Qt / QObject](./qt.md) | BYO/OS for Qt Developers | Object trees, properties, QML |
| [React](./react.md) | BYO/OS for React Developers | Declarative UI, components, reconciliation |
| [AppKit](./appkit.md) | BYO/OS for AppKit Developers | View hierarchy, responder chain, KVO |
| [Win32](./win32.md) | BYO/OS for Win32 Developers | HWNDs, message loop, common controls |
| [AT-SPI](./at-spi.md) | BYO/OS for AT-SPI Developers | Accessibility, tree projection, observers |

## Quick Orientation

A traditional desktop composes several layers:

| Layer | Traditional | BYO/OS |
|---|---|---|
| In-process type system | GObject / QObject | None (objects are protocol-level) |
| IPC | D-Bus | Orchestrator (APC routing) |
| Display protocol | Wayland / X11 | BYO protocol over APC |
| Accessibility | AT-SPI | Observer subscriptions |
| App rendering | GTK / Qt (per-app) | Compositor + daemons (centralized) |

BYO/OS merges these into one protocol. The object tree serves as the IPC
mechanism, the display description, and the accessibility source.
