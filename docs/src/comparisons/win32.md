# BYO/OS for Win32 Developers

Win32 assigns a handle (`HWND`) to every UI element, from top-level windows to
individual buttons, and manages them via a central message loop. BYO/OS uses a
similar handle-based model. Every element is an object with a unique ID, and
the Orchestrator serves as the central message router. This page maps Win32
concepts to BYO/OS.

## HWNDs → Objects

Win32 windows are identified by `HWND` handles. Every UI element — top-
level windows, buttons, edit controls, static text — is a window with
an HWND. The system maintains a hierarchy of parent and child windows.

BYO/OS objects are identified by qualified string IDs (`app:sidebar`).
Every UI element is an object in a tree maintained by the orchestrator.

```c
// Win32: create a child window
HWND hButton = CreateWindowEx(0, "BUTTON", "Save",
    WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON,
    10, 10, 80, 30,
    hParent, (HMENU)ID_SAVE, hInstance, NULL);
```

```byo
// BYO/OS: create a child object
+button save label="Save"
```

| Win32 | BYO/OS |
|---|---|
| HWND | Qualified ID (`app:save`) |
| Window class (`"BUTTON"`, `"EDIT"`) | Object kind (`button`, `text`) |
| `CreateWindowEx` | `+kind id props` (upsert) |
| `DestroyWindow` (cascades) | `-kind id` (cascades) |
| `SetWindowText` / `SetWindowLongPtr` | `@kind id key=value` (patch) |
| Child window | Child object (`{ ... }` nesting) |

## Window Classes → Object Types

Win32 window classes (`WNDCLASS`) define behavior for a category of
windows. You register a class with a `WndProc`, then create windows of
that class. System classes like `"BUTTON"` and `"EDIT"` are built in.

BYO/OS object types are freeform strings. Behavior is defined by which
daemon claims the type. The controls daemon claims `button`, `slider`,
etc. — similar to Win32's built-in classes. Third-party types use
dot-qualified names (`org.example.Widget`), like registering a custom
window class.

## Message Loop → Event Routing

Win32 uses a **message loop**. The system posts messages (`WM_PAINT`,
`WM_LBUTTONDOWN`, `WM_COMMAND`) to a thread's message queue. The app
calls `GetMessage` / `DispatchMessage` to route them to the target
window's `WndProc`.

BYO/OS has a similar flow. The compositor sends events to the
orchestrator, which routes them to the owning process. Apps read events
from stdin and acknowledge them.

```c
// Win32: message loop
while (GetMessage(&msg, NULL, 0, 0)) {
    TranslateMessage(&msg);
    DispatchMessage(&msg);
}

// WndProc handles messages
LRESULT CALLBACK WndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    switch (msg) {
        case WM_COMMAND:
            if (LOWORD(wParam) == ID_SAVE) { save(); }
            return 0;
    }
    return DefWindowProc(hwnd, msg, wParam, lParam);
}
```

```byo
// BYO/OS: read event from stdin, acknowledge
!click 0 save
!ack click 0 handled=true
```

Win32's `DefWindowProc` provides default handling for unhandled
messages. BYO/OS's `handled=false` ACK tells the orchestrator to bubble
the event to the parent object.

## GDI / Direct2D → Compositor

Win32 apps draw using GDI (`BeginPaint`, `TextOut`, `FillRect`) or
Direct2D/Direct3D. Each window handles `WM_PAINT` and draws its own
content.

BYO/OS apps do not draw. The compositor renders the entire object tree
via GPU. There is no paint message, no device context, no per-window
rendering.

For raster content, BYO/OS provides the Kitty graphics channel — a
separate protocol for uploading pixel data that the compositor holds as
GPU-side resources.

## Common Controls → Daemons

Win32 common controls (`comctl32.dll`) provide system-standard buttons,
list views, tree views, toolbars, etc. They are window classes
registered by the system.

BYO/OS daemons fill the same role. The controls daemon claims types
like `button` and `slider`, expands them into renderable primitives,
and handles their input events. Apps use semantic props (`label="Save"`)
and the daemon decides how the control looks.

```c
// Win32: create a common control
HWND hList = CreateWindowEx(0, WC_LISTVIEW, "",
    WS_CHILD | WS_VISIBLE | LVS_REPORT,
    0, 0, 400, 300, hParent, NULL, hInstance, NULL);
ListView_InsertColumn(hList, 0, &lvc);
```

```byo
// BYO/OS: create a daemon-owned object
+button save label="Save"
// daemon expands into views the compositor can render
```

## SendMessage / PostMessage → Protocol Commands

Win32 uses `SendMessage` (synchronous) and `PostMessage` (asynchronous)
to communicate between windows. Messages carry a `UINT` message ID and
two integer-sized parameters (`WPARAM`, `LPARAM`).

BYO/OS commands are always asynchronous text messages over stdin/stdout.
Object mutations (`+`, `@`, `-`) are declarative. Events (`!`) and
requests (`?`) carry string properties.

| Win32 | BYO/OS |
|---|---|
| `SendMessage` (sync) | No equivalent (all async) |
| `PostMessage` (async) | Protocol commands over stdin/stdout |
| `WM_COMMAND` | `!click seq target` |
| `WM_SETTEXT` | `@kind id content="text"` |
| `WM_DESTROY` | `-kind id` |

## Registry / DLL → Daemon Claims

Win32 extends its control library through COM classes, ActiveX controls,
and registered window classes. Registration uses the Windows registry or
COM manifests.

BYO/OS extends its type system through daemon claims. A daemon sends
`?claim 0 typename` to own a type. No registry, no DLL loading — the
daemon is a separate process that the orchestrator connects to.

## Concept Map

| Win32 | BYO/OS |
|---|---|
| HWND | Qualified ID (`app:id`) |
| Window class (`WNDCLASS`) | Object kind (string) |
| `CreateWindowEx` | `+kind id props` |
| `DestroyWindow` | `-kind id` (cascade) |
| `SetWindowText` | `@kind id content="text"` |
| Message loop (`GetMessage`) | Read events from stdin |
| `WndProc` | Event handler (parse + ACK) |
| `DefWindowProc` | `!ack kind seq handled=false` (bubble) |
| `WM_PAINT` / GDI | No equivalent (compositor renders) |
| Common controls (`comctl32`) | Daemons (controls, text, etc.) |
| `PostMessage` | Protocol commands (async) |
| Window class registration | `?claim seq type` |
| `DISPLAY=:0` / Remote Desktop | stdin/stdout over SSH |
