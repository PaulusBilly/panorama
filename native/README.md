# Panorama native

This workspace is the foundation for Panorama's Rust and GPUI desktop app.
`panorama-core` contains shared logic without UI dependencies.
`panorama-app` provides the native shell, navigation and shared theme.

Install rustup; this workspace selects Rust 1.99.0 with rustfmt and Clippy.
On Windows, install the MSVC Build Tools with Desktop development with C++
and a Windows SDK for the `x86_64-pc-windows-msvc` target.

Run these commands from `native/`:

```sh
cargo run -p panorama-app
cargo test --workspace
```

The Electron app remains the shipping app.

## Dependencies

- `rusqlite` (=0.40.2, `bundled`): the native key-value store; ships SQLite without a system dependency.
- `tempfile` (=3.27.0, dev-dependency): isolated directories for store tests.
- `windows` (=0.62.2): Win32 HWND and GDI desktop capture, matching GPUI's resolved version.
- `raw-window-handle` (=0.6.2): obtain the GPUI window's HWND without platform assumptions.
- `png` (=0.18.1): encode debug window captures on a background executor.

## App shell

Run `cargo run -p panorama-app -- --reduced-motion` to disable route transitions.
Light is the shipping default; dark is available only for debug captures:

```sh
target/release/panorama.exe --screenshot home home.png
target/release/panorama.exe --screenshot search:arrival search.png --size 1280x800 --theme dark --reduced-motion
```

Screenshot sizes use logical pixels and respect the 960x600 window minimum.
On Windows capture maps the client rectangle to screen coordinates for desktop BitBlt;
keep the interactive desktop visible and the window unobscured. Capture and PNG
writing run off the UI thread, after a settled frame and at least 500 ms.

Every `--screenshot` invocation checks the shell's inherited GPUI text style at
400/500/700. It fails with a non-zero exit if the resolved family or the bundled
faces' M advances differ. On Windows the registered family is `DM Sans 14pt`.

The app modules are `main` (startup), `args`, `app`, `router`, `titlebar`,
`header`, `routes/{home,search,film,player,addons}`, `theme`, `motion`, `assets`,
and `debug/screenshot`. History stores 100 entries; the eight most recently visited
views retain their entities and scroll handles. Older entries remount on revisit.
Interrupted motion resumes from the sampled opacity and offset. The macOS
38px caption keeps system traffic lights and native titlebar dragging; it has
not been run on macOS.

The component theme is synchronized through `Theme::change` then `Theme::update`
so its solid colors, renderable tokens and Base projection agree. Mapped tokens:
background/foreground/border, accent/foreground, muted/foreground,
popover/foreground, ring, selection, scrollbar/thumb/hover, button/foreground/
hover/active, primary/foreground, secondary/foreground/hover/active,
danger/foreground, input, caret and skeleton. Component fonts use DM Sans;
component radii are zero because the CSS supplies no general radius.

GPUI 0.3.8 provides Windows caption hit-test areas (including snap layouts) and
keyboard click activation for focused controls. Raw SVG elements require an explicit
text color; parent text color is not inherited, so caption icon hover uses group hover.
It has no exit animation primitive; the shell retains outgoing views during their fades.
Windows application activation is a no-op, so debug captures activate the window
explicitly. Frame callbacks have no current view; animation requests must be made
during rendering. Its text-input focus query is component-owned, so future custom
text inputs should register with that facility or extend the app's Backspace guard.
Tracked focus handles own their Tab eligibility; element tab_stop settings do not
update those handles. Bound Tab actions bypass raw key handlers, so a keystroke
observer updates focus visibility. GPUI clears keyboard modality on mouse movement;
the shell keeps focus rings visible until pointer-down, matching Electron. Rings use
border overlays because GPUI drop shadows fill transparent control interiors.
The CSS drift tests fall back to the fixture when the Electron stylesheet is absent.
