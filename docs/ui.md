# Aoxn UI (`stdlib/ui.ax` + `stdlib/ui_draw.ax` + a backend)

An immediate-mode GUI standard library for Aoxn, Qt-flavored. Written 100%
in Aoxn on top of raw FFI — no external crates, no C sources, no resource
files. Status: **v0.46.0 — Windows + Linux** — layout managers,
text selection + multi-line editing with clipboard, a focus chain, menu
bar, tree/table model+view, signal-slot events, 20+ widgets, floating
overlays, 16-role themes, horizontal scroll areas and a monospace face.

The toolkit is **three portable files plus one backend per platform**: the
portable core (`ui.ax`), the platform-neutral widget layer (`ui_draw.ax`),
and ONE of the two backends (`ui_win.ax` for Windows, `ui_x11.ax` for
Linux). Both backends implement the same `plat_*` primitive set, so the
widget set is identical everywhere.

```aoxn
import * from "stdlib/ui_win"           # Windows: Win32 + GDI
import * from "stdlib/ui_x11"           # Linux: Xlib + Xft (v0.46.0)

def main() -> int:
    c = ui_init("Hello", 640, 480)
    clicks = 0
    while c.open:
        c = ui_frame(c)             # pump messages, sample input, begin frame
        if c.open:
            if ui_button(c, 20, 20, 120, 34, "Click me"):
                clicks = clicks + 1
            ui_label_int(c, 20, 70, "clicks: ", clicks)
            ui_present(c)           # blit the frame
    ui_fini(c)
    return 0
```

```
aoxn run examples\ui_gallery.ax -l user32 -l gdi32
aoxn run examples\ui_demo.ax -l user32 -l gdi32
```


## Why immediate mode

Aoxn has **no function pointers and no closures**, so the classic
callback/signal-slot architecture of Qt is impossible to express. The
library therefore uses the immediate-mode model (Dear ImGui style), which
fits the language exactly:

- widgets are plain functions called every frame;
- all state is owned by the application and passed in/out
  (`dark = ui_checkbox(c, x, y, "dark", dark)`);
- there is no widget tree, no handles to free, no add/remove calls.

For anything beyond a handful of values, collect widget state in one struct
and pass it through the draw functions — the stdlib write-back idiom:

```aoxn
struct Gal:
    dark: bool
    clicks: int
    name: string

def draw(c: UI, gal: Gal) -> Gal:
    gal.dark = ui_toggle(c, 760, 16, 170, 30, "dark theme", gal.dark)
    if ui_button(c, 20, 20, 120, 34, "Click me"):
        gal.clicks = gal.clicks + 1
    te = ui_textbox(c, 20, 70, 200, 28, gal.name)
    gal.name = te.text
    return gal
```

The event loop is poll-based for the same reason: the window procedure is
literally `DefWindowProcW` (its address is obtained via `GetProcAddress`
since Aoxn cannot pass one of its own functions). Every frame the backend
drains the message queue (catching `WM_CHAR` text input and mouse-wheel
deltas), then polls `GetCursorPos` + `ScreenToClient` (mouse),
`GetAsyncKeyState` (buttons + a 256-key snapshot with per-frame edge
detection) and `GetClientRect` (resize). Closing the window destroys it;
`IsWindow()` goes false and the app loop ends. When idle,
`MsgWaitForMultipleObjects` caps the loop at `c.cap_ms` (default 15 ms
≈ 66 fps) so a background window costs ~0% CPU.

## Files

| File | Contents | Portable? |
|---|---|---|
| `stdlib/ui.ax` | text encoding (UTF-8 → UTF-16LE with surrogates), `ui_rgb`, widget ids, `Palette`/`UI`/`TextSize`/`Rect`/`TextEdit`/`ListPick` types, light/dark palettes, heap-state accessors, per-frame bump arena (`utf16f`/`itoa10`), **layout engine**, **focus chain**, disabled mode, **text-editing primitives**, WM_CHAR queue, overlay slots | yes — no platform externs, so the tests exercise it without a window |
| `stdlib/ui_draw.ax` | **every widget**, the drawing-primitive wrappers, and the portable `ui_init`/`ui_frame`/`ui_present`/`ui_fini` shell. Calls a `plat_*` primitive contract; declares **no** platform externs and never tests `target_os()` | yes — one widget set, no Win32 symbols |
| `stdlib/ui_win.ax` | Windows backend: Win32/GDI externs + the `plat_*` implementations | Windows only |
| `stdlib/ui_x11.ax` | Linux backend: Xlib/Xft externs + the same `plat_*` set (v0.46.0; off-screen Pixmap double buffering, selection-protocol clipboard, `XQueryKeymap` key sweep) | Linux only |

A backend imports `ui_draw.ax` (which imports `ui.ax`) and merges all three
into one flat namespace, so a program reaches every widget through **one
import line**. Backends must be linked explicitly: `-l user32 -l gdi32` on
Windows, `-l X11 -l Xft` on Linux.

### The `plat_*` primitive contract

`ui_draw.ax` never talks to a windowing system directly. It calls these,
and each backend supplies its own:

`plat_supported` · `plat_now_ms` · `plat_canvas_ok` · `plat_init` ·
`plat_pump` · `plat_present` · `plat_canvas_resize` · `plat_close` ·
`plat_fini` · `plat_alert` · `plat_clip_get` · `plat_clip_set` ·
`plat_fill_rect` · `plat_frame_rect` · `plat_round_fill` · `plat_ellipse` ·
`plat_ellipse_fill` · `plat_line` · `plat_text` · `plat_text_big` ·
`plat_text_sub` · `plat_measure` · `plat_measure_sub` · `plat_clip_push` ·
`plat_clip_pop`

`tests/ui.rs` pins this contract three ways: the widget layer must not
mention a single Win32 or Xlib symbol, both backends (`ui_win.ax` AND
`ui_x11.ax`) must implement the same primitive set, and every primitive
the widget layer calls must exist in both. A widget that reaches for a
platform symbol fails the test run — not one user's OS.

## Layout managers (Qt's QV/QH/QGridLayout counterpart)

Boxes and grids live on a stack in the shared heap block (up to 8 nested
levels); widgets keep taking explicit rectangles and the layout hands them
out. Margins and spacing come from `*_begin`; `ui_v_item`/`ui_h_item`
stretch fully across the cross axis; the `*_p` variants size the main axis
in permille (1/1000) of the box, which is how you mix fixed and flexible
rows/columns.

```aoxn
ui_hbox_begin(c, 20, 76, 600, 120, 0, 10)      # x, y, w, h, margin, spacing
r = ui_h_item_p(c, 400)                        # left column: 40%
ui_vbox_begin(c, r.x, r.y, r.w, r.h, 0, 8)     # nest a vbox inside
r2 = ui_v_item(c, 30)                          # full-width row, 30 px
if ui_button(c, r2.x, r2.y, r2.w, r2.h, "OK"): ...
ui_layout_end(c)                               # pop the vbox
r = ui_h_item_p(c, 600)                        # right column: 60%
ui_fill_rect(c, r.x, r.y, r.w, r.h, ui_rgb(90, 150, 220))
ui_layout_end(c)                               # pop the hbox
```

| Function | Signature | Behavior |
|---|---|---|
| `ui_vbox_begin` | `(c, x, y, w, h, margin, spacing)` | vertical stack; items are full inner width |
| `ui_hbox_begin` | `(c, x, y, w, h, margin, spacing)` | horizontal stack; items are full inner height |
| `ui_grid_begin` | `(c, x, y, w, h, cols, margin, spacing)` | grid with `cols` equal columns |
| `ui_layout_end` | `(c)` | pops the current box/grid |
| `ui_v_item` | `(c, h) -> Rect` | next vbox row of height `h`; cursor advances by `h + spacing` |
| `ui_h_item` | `(c, w) -> Rect` | next hbox column of width `w` |
| `ui_v_item_p` | `(c, permille) -> Rect` | like `ui_v_item`, height = permille/1000 of inner height |
| `ui_h_item_p` | `(c, permille) -> Rect` | like `ui_h_item`, width = permille/1000 of inner width |
| `ui_spacer` | `(c, px)` | fixed gap along the main axis (no extra spacing) |
| `ui_grid_row` | `(c, h)` | start a grid row of height `h` (wraps a half-finished row first) |
| `ui_grid_cell` | `(c) -> Rect` | next cell; the last column absorbs rounding so rows end exactly at the inner right edge |

## API reference

### Lifecycle

| Function | Signature | Notes |
|---|---|---|
| `ui_init` | `(title: string, w: int, h: int) -> UI` | DPI-aware window with exactly `w×h` client area; `c.open == false` on failure. Calls `FreeConsole()` (console-subsystem exes would otherwise keep a console window) |
| `ui_frame` | `(c: UI) -> UI` | pump + input sampling + background fill; **returns the updated context** — assign it: `c = ui_frame(c)` |
| `ui_present` | `(c: UI)` | draw pending overlays (combo popups, tooltips) then blit the frame (call once, after widgets) |
| `ui_close` | `(c: UI)` | request close (e.g. Esc) |
| `ui_fini` | `(c: UI)` | release window/DC/bitmap/heap state |
| `ui_alert` | `(text: string, caption: string)` | blocking message box |

The canonical loop:

```aoxn
while c.open:
    c = ui_frame(c)
    if c.open:
        ...widgets...
        ui_present(c)
ui_fini(c)
```

### Widgets (call every frame)

v1:

| Function | Signature | Behavior |
|---|---|---|
| `ui_button` | `(c, x, y, w, h, s) -> bool` | true on the release frame (press + release inside) or Enter/Space when focused. Hover = accent border, press = darker face + 1px text shift |
| `ui_checkbox` | `(c, x, y, s, checked) -> bool` | returns the new checked state |
| `ui_slider` | `(c, x, y, w, value, vmin, vmax) -> int` | horizontal; drag continues outside the track (press-claim); arrows step when focused |
| `ui_progress` | `(c, x, y, w, value, vmax)` | groove + accent fill |
| `ui_label` / `ui_label_dim` | `(c, x, y, s)` | text in normal / dim color |
| `ui_label_int` | `(c, x, y, prefix, v)` | `prefix` + decimal value with **zero per-frame allocation** (arena `itoa10`) |
| `ui_title` | `(c, x, y, s)` | 26px bold heading |
| `ui_separator` | `(c, x, y, w)` | 1px line |
| `ui_panel` | `(c, x, y, w, h)` | filled panel |

v2 (v0.29.2):

| Function | Signature | Behavior |
|---|---|---|
| `ui_textbox` | `(c, x, y, w, h, text) -> TextEdit` | single-line field; `TextEdit{text, changed, enter}`. Click focuses and places the caret, typing inserts (UTF-16 surrogate pairs merged), Backspace/Delete edit, arrows/Home/End move the caret (UTF-8 aware), Enter sets `enter`. Tab moves focus away |
| `ui_toggle` | `(c, x, y, w, h, s, on) -> bool` | checkable push button (accent face + selected text when on) |
| `ui_radio` | `(c, x, y, s, group, item) -> int` | exclusive choice: `group` is the group's current value, `item` this button's value; returns the new group value |
| `ui_spinbox` | `(c, x, y, w, value, vmin, vmax) -> int` | value field + stacked up/down buttons; up/down arrows step when focused |
| `ui_combobox` | `(c, x, y, w, items: [string; N], n, cur) -> int` | drop-down; press opens a floating item list (drawn on top by `ui_present`), press on an item picks it and swallows the click, press elsewhere closes. Wheel scrolls a long list |
| `ui_listbox` | `(c, x, y, w, h, items: [string; N], n, cur, scroll) -> ListPick` | scrollable single-select list; `ListPick{cur, scroll}`. Wheel scrolls; the scrollbar drags |
| `ui_tabs` | `(c, x, y, w, labels: [string; N], n, cur) -> int` | tab strip (accent top bar on the active tab); the caller draws the page content below |
| `ui_groupbox` | `(c, x, y, w, h, title)` | thin frame with the title punched through the top line |
| `ui_scroll_begin` | `(c, x, y, w, h, scroll_y, content_h) -> int` | clipped viewport; returns the new scroll offset (wheel + scrollbar). Draw content at `y - scroll` |
| `ui_scroll_begin_h` | `(c, x, y, w, h, scroll_x, content_w) -> int` | horizontal twin (v0.46.0): bottom scrollbar (drag), clip excludes the bar row. Draw at `x - scroll_x`. Nests with the vertical variant; close with `ui_scroll_end_h` |
| `ui_font_mono` | `(c) -> UI` | point the body font at the backend's monospace face (v0.46.0). Every widget drawn after this measures/renders monospace |
| `ui_font_sans` | `(c) -> UI` | restore the sans face the backend opened at init |
| `ui_scroll_end` | `(c)` | **required** — pops the GDI clip stack |
| `ui_tooltip` | `(c, x, y, w, h, text)` | shows `text` near the cursor after hovering the rect ~0.55 s (drawn on top by `ui_present`) |

Widgets never allocate Aoxn strings per frame: text goes through a 64 KiB
bump arena that resets each frame (`utf16f`), numbers render through
`itoa10`, and the caret position is cached on (id, caret, length) so an
idle focused text box costs zero allocations. A steady-state UI leaks
nothing (Aoxn string concat leaks by design; the arena is the discipline
that keeps frame paths clean — same idea as the web server's byte buffers).
Text *editing* allocates a fresh string per keystroke, the same cost model
as `+`.

### Keyboard focus

Interactive widgets register their id every frame — **call order is tab
order**. The backend rotates the focus ring with Tab / Shift+Tab against
the previous frame's registry (Qt's focus chain model), draws an accent
ring around the focused widget, and Enter/Space activates buttons, toggles,
checkboxes and radios. Clicking a widget focuses it. Slider arrows and
spin-box arrows work on the focused widget. Disabled widgets drop out of
the chain.

### Disabled mode (Qt's setEnabled)

```aoxn
if not enable:
    ui_begin_disabled(c)
ui_button(c, x, y, w, h, "runs either way")   # drawn dimmed, ignores input
if not enable:
    ui_end_disabled(c)
```

### Theming

Colors are `COLORREF`s built with `ui_rgb(r, g, b)`. The palette
(`ui_palette_get` / `ui_palette_set`, `light_palette()` / `dark_palette()`)
has 16 named roles: `bg panel text text_dim widget widget_hover
widget_down border accent groove sel sel_text disabled_face disabled_text
tooltip_bg tooltip_text` — set it at the top of the loop to switch themes
(see `examples/ui_gallery.ax`). Custom themes are ordinary `Palette`
values; fill every field (construction requires all of them).

### Drawing primitives

`ui_fill_rect(c,x,y,w,h,color)` · `ui_frame_rect(...)` (border) ·
`ui_draw_line(c,x1,y1,x2,y2,color)` · `ui_draw_text(c,x,y,s,color)` ·
`ui_draw_text_big(...)` · `ui_measure(c,s) -> TextSize{w,h}`.

### Input

- mouse: `c.mx`, `c.my` (client px), `c.dn` / `c.dn2` (left/right button
  this frame), `ui_mouse_in(c, x, y, w, h)`, `c.wheel` / `ui_wheel(c)`
  (accumulated wheel delta this frame; 120 per notch).
- keyboard: `ui_key_down(c, vk)`, `ui_key_pressed(c, vk)` (true only on
  the frame the key went down). `vk` is a Windows virtual key — 8 =
  Backspace, 9 = Tab, 13 = Enter, 27 = Esc, 32 = Space, 35..40 =
  Home/End/arrows (no hex literals in the language, so VKs are decimal).
- text input: `ui_char_count(c)` / `ui_char_str(c)` expose the UTF-16
  units queued from `WM_CHAR` this frame as a UTF-8 string (you normally
  use `ui_textbox` instead).
- `c.w`, `c.h` = client size (live), `c.frames` = frame counter,
  `c.cap_ms` = frame-cap milliseconds (assign before the loop to change).

### Overlays

Drop-down popups and tooltips are recorded during widget calls and drawn
by `ui_present` **after** every widget, so they float above the layout
regardless of call order. While a combo popup is open, presses anywhere
(even on widgets drawn earlier in the frame) are swallowed — standard menu
behavior. A tooltip re-arms every frame while its rect is hovered.

## v3 (v0.29.3): selection, multi-line editing, menus, model/view, signals

### Text selection & the clipboard

A selection is a `Sel{anchor, caret}` pair of byte offsets (equal = just a
caret). `ui_textbox` and `ui_textedit` share the same editing core, all
pure functions in `ui.ax` (tested on every platform):

| Function | Behavior |
|---|---|
| `sel_make(caret)` / `sel_active(s)` / `sel_lo(s)` / `sel_hi(s)` | selection math |
| `edit_type(s, anchor, caret, ins) -> MEdit` | insert, or replace the selection if one exists |
| `edit_backspace` / `edit_delete` | delete the selection, or one codepoint before/after the caret |
| `edit_left/right/up/down` / `edit_home/end` | UTF-8-aware movement; `shift` extends, otherwise the selection collapses; `whole` (Ctrl) jumps to doc start/end |

Both widgets handle **Shift+arrows / mouse drag** to select, **Ctrl+A**
select all, **Ctrl+C/X/V** copy/cut/paste via the Win32 clipboard
(`ui_clip_get`/`ui_clip_set`, CF_UNICODETEXT). Rendering a selection uses
`ui_measure_sub` / `ui_draw_text_sub`, which operate on a byte range
through the per-frame arena — no substring is built, so a selected field
still allocates nothing per frame.

### Multi-line editor

`ui_textedit(c, x, y, w, h, text, anchor, caret, scroll) -> EditView{text,
anchor, caret, changed, scroll}` — Enter inserts a newline, arrows move by
line (up/down), Home/End hit the line ends, the wheel + scrollbar scroll
lines. `scroll` is the first visible line (app-owned, like `ListPick`).
Line boundaries come from a cached line-start table in the heap block
(`lines_sync` rebuilds it only when the text length or owner changes).

### Menus (Qt's QMenuBar)

```aoxn
bar = ui_menubar(c, 0, 0, c.w, ["File", "Edit", "Help"], 3, open)
if bar.open == 0:
    mp = ui_menu(c, bar, ["New", "Open", "Save", "Exit"], 4)
    open = mp.open
    if mp.pick == 3: ui_close(c)
```

`ui_menubar` returns the open title's rect + state (hovering a title while
a menu is open switches menus; outside click / Esc closes). `ui_menu`
draws the open menu's items as a floating overlay (via `ui_present`) and
returns `MenuPick{open, pick}`.

An open menu is keyboard-navigable: mouse hover or Up/Down move the
highlight (both wrap), Enter picks the highlighted item, Esc closes. The
open menu is also **modal for input** — it consumes the arrow/Enter/Esc
edges and the WM_CHAR queue for that frame, so widgets drawn after it do
not fire on the same frame. Call `ui_menubar`/`ui_menu` before the page
content (the natural order) for that to hold. A popup lists at most the
first 32 items.

### Model/view without interfaces (tree & table)

Qt's model/view shape adapted to a language with no interfaces: a model is
a **heap block behind a one-field struct**, so views can mutate it in place
(no value-copy write-back). Getters for unset cells return `""`.

| Model | Construction / access | View |
|---|---|---|
| `TableModel` | `table_model_new(rows, cols)`, `tm_set/tm_get`, `tm_set_header/tm_header`, `tm_set_colw/tm_colw`, `tm_rows/tm_cols` | `ui_table(c, x, y, w, h, m, sel_row, scroll) -> TableRet` |
| `TreeModel` | `tree_model_new(n)`, `tree_set_label/tree_label`, `tree_set_parent/tree_parent`, `tree_set_expanded/tree_expanded`, `tree_depth/tree_visible`, `tree_has_child` (O(1) — the model carries a per-node child count that `tree_set_parent` maintains, including on re-parent) | `ui_tree(c, x, y, w, h, m, sel, scroll) -> TreeRet` |

`ui_tree` toggles a node's expansion **directly on the model** when the
+/- is clicked; `ui_table` draws header + rows with per-column widths
(explicit via `tm_set_colw`, else equal shares). Both are focusable and
support wheel/scrollbar + arrow-key selection.

### Signal-slot without function pointers

The language has no callbacks, so signals are **integer channels**: emitters
name a signal id, a connection table picks a slot id, and the app drains one
queue with a single `switch` on `ev.slot` — the decoupling Qt gets from
signals/slots, minus the callables.

```aoxn
ui_connect(c, SIG_SAVE, SLOT_FILE)         # rewire at runtime by re-calling
if ui_button(c, x, y, w, h, "Save"):
    ui_emit(c, SIG_SAVE, EV_CLICK, 0, 0)   # emitter names a signal, not a handler
...
while i < ui_event_count(c):               # ONE dispatch site
    ev = ui_event(c, i)
    if ev.slot == SLOT_FILE: ...
```

`Ev{slot, sig, kind, a, b}`; the queue resets every `ui_frame` and holds 32
events (overflow drops). `ui_slot_of(signal)` reports the current binding.

## Design notes / implementation facts

- **Rendering**: GDI into a memory DC (double buffered). The window class
  uses `CS_OWNDC` and a `NULL` background brush, so nothing erases the
  window except us — no flicker. `ui_present` draws overlays, then one
  `BitBlt(SRCCOPY)`.
- **Fonts**: Segoe UI 16px regular + 26px bold via `CreateFontW` on
  Windows (Xft `Sans-12`/`Sans-20` on Linux); monospace is `ui_font_mono`
  — Consolas on Windows, the fontconfig `Monospace` alias on Linux (both
  faces are opened by the backend at init and parked on `UI.font_mono`).
  `SetBkMode(TRANSPARENT)`, `SetTextAlign(TA_TOP)`; text is measured with
  `GetTextExtentPoint32W` (Windows) / `XftTextExtentsUtf8` (Linux).
- **Stock pens**: `DC_PEN = 19`, `DC_BRUSH = 18`, `NULL_BRUSH = 5`,
  `NULL_PEN = 8`. **Do not "fix" 19 to 20** — `GetStockObject(20)` is out
  of range, `SelectObject` fails silently, and every outline disappears
  (this shipped broken in v1; v0.29.2 found it by pixel-inspecting
  rendered frames).
- **UTF-16 everywhere**: all W-APIs receive runtime-converted UTF-16LE
  (emoji/astral planes included — the demo draws 😀). Surrogate pairs are
  encoded arithmetically (the language has no bitwise operators; every
  Win32 constant is a hand-summed decimal literal, e.g.
  `WS_OVERLAPPEDWINDOW = 13565952`, `SRCCOPY = 13369376`).
- **Resize**: detected per frame via `GetClientRect`; the memory bitmap is
  recreated on change.
- **Input robustness**: `GetAsyncKeyState` bit 15 (down now) is OR-ed with
  bit 0 (pressed since the previous call) so presses shorter than one
  frame gap are still seen; exactly one `GetAsyncKeyState` call per key
  per frame (a second call clears the latch). Mouse-button state is
  sampled only while the window is foreground. `WM_CHAR` units below 32
  are dropped (Enter/Tab/Backspace stay key events).
- **Click semantics**: press edge claims the single "active" slot in the
  heap block; release fires the click only if the pointer is back inside
  the widget (standard IMGUI behavior, one drag at a time). Overlays claim
  the slot with a sentinel that swallows the whole click.
- **FreeConsole**: the compiler emits console-subsystem exes; `ui_init`
  detaches from the console so a GUI app doesn't keep a terminal open.
  Piped stdout (tests) keeps working. `print` output is invisible after
  that — log to a file if you need it.

## Heap block layout (`ui.ax` header is authoritative)

The context's `st` pointer addresses 1024 i64 slots: key snapshots
(4..515), palette (516..531), focus chain (536..538, registry 720..783),
caret (539, cache 532..535), disabled counter (540), WM_CHAR queue
(541..543), wheel (544), overlay record (545..555), tooltip hover
(556..557), layout stack (558, frames 560..719), popup item pointers
(784..815), textbox caret owner/anchor (816/819), and two v3 heap blocks:
the event bus at 817 (connect table + 32-event ring) and the line cache at
818 (line starts for multi-line editing). `ui_state_init` / `ui_state_free`
allocate and release it — the platform backend calls them at init/fini,
headless tests can too.

Since v0.46.0/v0.47.0 the tail of the block carries per-platform and
widget-layer state, all documented in the `ui.ax` header: 820/821 the
backend clip stack (`SaveDC` handles on Windows, rects on X11), 822/823 the
X11 clipboard buffer, 824 the X11 close flag, 826 the parked sans font,
827..834 the widget layer's own clip shadow (827 stack, 828 depth,
829..832 the clip in force, 833 the canvas-known flag) plus the
text-measure cache at 834, and 835..840 the Windows DC state cache
(selected brush/pen, their colors, selected font, text color).

## Platform matrix

| OS | Backend file | Link flags | Status |
|---|---|---|---|
| Windows (Win32 + GDI) | `stdlib/ui_win.ax` | `-l user32 -l gdi32` | complete, tested (`tests/ui.rs` + CI) |
| Linux (Xlib + Xft) | `stdlib/ui_x11.ax` | `-l X11 -l Xft` | complete, tested (`tests/ui.rs` + Xvfb probe in CI, v0.46.0) |

The X11 backend was removed with the other platforms in v0.30.0 and came
back in v0.46.0, rewritten against the v3 contract. Nothing about the
toolkit is platform-specific: the widget layer only ever calls `plat_*`
primitives, and the contract tests keep both backends honest. The close
request travels through shared-block slot 824 on X11 (Windows destroys the
window and polls `IsWindow`); slot 826 parks the sans font handle for
`ui_font_sans`. See `docs/platform-support.md` for the platform story.
macOS remains unreachable: the Cocoa surface needs struct-by-value externs
the language cannot express.

## Tests

- `ui_pure_helpers_utf16_rgb_ids` (all platforms): UTF-8 → UTF-16 incl.
  surrogates, COLORREF packing, i32 assembly, widget-id injectivity,
  palettes — via a driver that imports only `stdlib/ui.ax`.
- `ui_layout_text_focus_portable` (all platforms): layout engine rect math
  (vbox/hbox/grid, permille, spacers, nesting), text-editing primitives,
  UTF-8 caret boundaries, WM_CHAR queue → UTF-8, tab-order rotation,
  disabled counter, 16-role palette, overlay slots, tooltip hover timing.
- `ui_window_selfclose_smoke` (Windows): builds a real window, runs ~90
  frames, closes itself; skips with exit code 3 where no window can be
  created. Bounded by a 60s kill-timeout in the harness.
- `ui_window_v2_widgets_smoke` (Windows): textbox, radio, toggle, spin
  box, list box, combo box, tabs, group box, scroll area and tooltip in a
  real window for ~60 frames; same skip/timeout protocol.
- `ui_v3_portable` (all platforms): text selection, the line cache, edit
  operations (type-over-selection, backspace join, UTF-8 up/down with
  shift), the signal-slot event bus (connect/emit/queue/clear), and the
  heap-backed table/tree models.
- `ui_window_v3_widgets_smoke` (Windows): multi-line editor, menu bar +
  menu, tree, table and a signal-slot dispatch loop in a real window for
  ~60 frames; same skip/timeout protocol.
- `ui_win_backend_emits_c` / `ui_x11_backend_emits_c`: the backend
  typechecks and emits C. This needs **no clang and no display**, because
  `compile_paths_to_c` stops after codegen — so it is the check that also
  runs on a headless box.
- `ui_draw_layer_is_platform_neutral`: asserts `ui_draw.ax` mentions no
  Win32 symbol and never calls `target_os()`.
- `ui_draw_calls_only_implemented_primitives`: every `plat_*` the widget
  layer calls is defined by BOTH `ui_win.ax` and `ui_x11.ax` (a missing
  one would otherwise only show up as a link error).
- `ui_clip_stack_restores_and_culls` (Windows): draws known rects in known
  colors and reads the canvas back with `GetPixel`, so a broken clip is a
  failed expectation rather than a blank window nobody looks at. It pins
  the v0.47.0 regression — "after-textbox", a nested push/pop pair, a
  culled draw, two fills across a `RestoreDC`, and the measure cache
  agreeing with itself. Against the pre-v0.47.0 stdlib it fails with SEVEN
  mismatches, so it is a regression test and not decoration.

Link-time coverage lives in CI rather than the unit tests: the Windows
job builds the gallery against `-l user32 -l gdi32`; the Linux job
(v0.46.0) links and runs `examples/ui_probe_x11.ax` for 30 real frames
under Xvfb against `-l X11 -l Xft`.

## Known limits (v3) / roadmap

- single window per process; no MDI or child windows
- `ui_textbox`/`ui_textedit` have no rich text; the editor has no
  undo/redo or word-wrap (long lines clip)
- `ui_menubar` supports one menu level (no nested submenus)
- `ui_table` has no column resize/drag-reorder or cell editing;
  `ui_listbox`/`ui_combobox`/`ui_menu` take up to 32 items in a floating
  popup (`overlay_items_store` clamp — longer lists silently show their
  first 32 entries)
- full-window repaint each frame — draws the visible op once and culls what
  the clip cannot show (v0.47.0), but there is still no damage tracking, so
  an idle frame costs a `BitBlt`
- no animations/timing APIs; `cap_ms` and `plat_now_ms` (tooltip delay,
  caret blink) are the only pacing controls
- once the new module system settles, `ui`/`ui_draw`/the backends should be
  migrated like the rest of the stdlib

## Performance notes (v0.45.0)

- **Text positioning is O(n) per click/drag**: `text_pos_in_range`
  measures one codepoint at a time and accumulates widths; the textbox
  and both textedit call sites share it (the per-codepoint re-measure of
  `s[0..j)` that made long lines O(n²) is gone). Since v0.47.0 it calls
  `plat_measure_sub` directly, NOT the cached `ui_measure_sub`: those
  one-codepoint ranges never repeat, so caching them would only flush the
  entries the widgets reuse.
- **The caret x-offset is cached** at st 532..535 (owner id / caret /
  length / width) — an idle focused textbox or textedit measures nothing
  per frame; the cache misses exactly when the text or caret moved.
- **`tree_has_child` is O(1)** against the per-node child count in the
  `TreeModel` block (4 slots per node: parent, expanded, label, child
  count); the tree view used to scan the whole model per visible row.
- **One scrollbar**: list/tree/table/textedit/scroll-area all drive the
  same `sb_widget` (groove + thumb + claim/drag/release); the scrollbar
  hit column is the full widget height even when the track starts below
  a header.

## Performance notes (v0.47.0)

Measured on this dev box (i5-1135G7, GDI, 1000x700 window), min of five
interleaved runs, timed with `QueryPerformanceCounter`, against the
pre-v0.47.0 stdlib compiled from the same sources by the same compiler.

- **A draw the clip cannot show costs four slot reads.** `rect_visible` /
  `text_visible` in `ui_draw.ax` answer that before the platform is asked
  at all. A 3000 px document in a 200 px viewport (60 labels + 60
  buttons, 96 % of them scrolled out of view) went from 79 ms to 47 ms per
  60 frames: **-40 %**.
- **A repeated text measure is answered from a 64-entry cache** keyed by
  (content hash, byte range): 52.8 ms -> 0.55 ms per 20 000 measures of
  the same string. The uncached `plat_measure` behind it is unchanged
  (52.6 ms), so the cache is a pass-through, not a different answer.
- **The Windows backend remembers the DC's selected pen, brush, font and
  colors** instead of re-selecting them on every call: a 10x10 fill is
  26.7 -> 25.1 us (alternating colors) and 24.8 -> 23.0 us (same color).
- **A 100-widget scene with no clipping widgets** (40 buttons, 30
  checkboxes, 10 spin boxes, tabs, menu bar, 16 labels) is **~9 % faster**
  end to end. That scene paints identical pixels in both builds, so it is
  the honest measure of the caches.
- **The clip fix itself makes a frame do MORE work, on purpose.** A heavy
  scene measured 1.8 ms/frame before v0.47.0 and 3.5 ms/frame after, and
  that is not a regression: the old number came from a renderer that was
  silently clipping most of the frame away (see below). What is left per
  frame is the `BitBlt` (~0.8 ms for 1000x700), the background fill
  (~0.08 ms) and one GDI call per visible widget op. `TextOutW` alone is
  ~7 us; `GetAsyncKeyState`, `PeekMessageW` and `GetClientRect` are free
  (10 000 calls each measure at 0 ms), which is why the pump was left
  alone.

### The clip stack, and why v0.47.0 exists

`plat_clip_push` / `plat_clip_pop` had never restored anything. GDI's
`IntersectClipRect` only ever NARROWS a DC's region, so the old pop —
re-intersecting the popped rect — narrowed it further, and nothing ever
reset it. One `ui_textbox` anywhere in a frame therefore left the whole
window clipped to that textbox rect for the rest of the process,
including the final `BitBlt` — a blit copies only the part of the source
the source DC's clip allows. In the stock gallery the Editor tab showed
the editor and lost the textbox, three labels and the status line drawn
after it. Three changes, in this order:

1. **`ui_win.ax` saves and restores**: one `SaveDC` handle per push
   (st 820/821), `RestoreDC` on pop. That is what a push/pop stack means,
   and it is exactly what the comment on `ui_x11.ax`'s `clip_apply` had
   described all along.
2. **`ui_draw.ax` keeps its own shadow of the stack** (st 827/828 hold the
   effective rect per level, st 829..832 the one in force). Now that a pop
   really restores, the shadow can also answer "can the platform see this
   at all?" — which is what culling needs. `ui_init` and
   `ui_resize_canvas` hand it the canvas rect (st 833 flips to 1); until
   they do, nothing is culled, which is what keeps the headless portable
   tests drawing everything they always did.
3. **The backends receive the EFFECTIVE rect**, not the requested one, so
   X11's push — which replaces the clip instead of intersecting it — nests
   correctly too.

Use `ui_clip_push` / `ui_clip_pop`, not the `plat_*` pair: the shadow is
what the culling reads, and driving the backend primitive directly
desynchronizes it.

### What the DC state cache is allowed to forget

The Windows cache holds what the DC currently has selected (st 835..840).
Anything that changes the DC behind its back must call `dc_cache_drop`:
`RestoreDC` on a clip pop, and the bitmap swap in `plat_canvas_resize`.
A stale entry would paint in the wrong color, so that drop list — not the
eight drawing primitives — is what has to stay in sync.

## Rendering, end to end

Rendering cost is unchanged by design: one GDI fill/text call per visible
widget op, one `BitBlt` per frame (`plat_present`), a full-window
background fill, no damage tracking, no retained scene. v0.47.0 made the
toolkit draw what it always meant to draw and stop paying for what nobody
can see. Making an IDLE frame cheaper than a busy one — damage rects, a
display list, skipping the blit when the frame is identical to the last —
is the next real lever and is not in this version.
