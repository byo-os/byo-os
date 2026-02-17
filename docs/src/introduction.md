# BYO/OS

BYO/OS is a terminal-protocol-based desktop environment. All communication
between components happens via ECMA-48 APC escape sequences over stdin/stdout.

The system is built around a central **orchestrator** that routes messages
between **apps** (which produce declarative UI descriptions), **daemons**
(which expand semantic objects like buttons into renderable primitives), and
a **compositor** (which renders the final output via GPU).

```text
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

Apps create objects in a shared tree using a text-based protocol. The
orchestrator maintains the authoritative state, routes messages, handles
daemon expansion, and delivers projected views to observers like the
compositor.

For the full protocol specification, see [`CLAUDE.md`](../../CLAUDE.md)
in the repository root.

## How This Book Is Organized

- **BYO/OS for Developers of...** — Maps BYO/OS concepts to familiar
  technologies (D-Bus, Wayland, X11, GObject, Qt, AT-SPI). Start with the
  one you know best.
