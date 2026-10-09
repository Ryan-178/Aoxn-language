//! stdlib UI tests.
//!
//! Two layers:
//! - stdlib/ui.ax (the portable half: UTF-16 encoding, COLORREF packing,
//!   widget ids, i32 reads, palettes, arena helpers) runs on EVERY platform
//!   — it declares no platform externs;
//! - stdlib/ui_win.ax (Windows backend) gets a window smoke test that
//!   creates a real window, runs a bounded frame loop and closes itself.
//!   It is Windows-only by construction: importing the backend pulls Win32
//!   references into uncalled external functions, which is exactly why the
//!   backend lives in its own file (the web suite's sock_win.ax pattern).

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn abs(rel: &str) -> String {
    // forward slashes: backslashes would read as escape sequences inside
    // the Aoxn import string literal
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(rel)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aoxn-ui-{}-{}", name, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// clang is required to turn emitted C into an executable. On a machine
/// without it (this dev box included) the runnable tests cannot run at all,
/// so they report a skip instead of failing — same pattern as the
/// self-hosting tests in tests/pipeline.rs.
fn have_clang() -> bool {
    aoxn::find_clang().is_some()
}

/// run an exe with a hard timeout so a wedged frame loop can never hang CI
fn run_with_timeout(exe: &PathBuf, secs: u64) -> (Option<i32>, String) {
    let mut child = Command::new(exe).stdout(std::process::Stdio::piped()).spawn().expect("spawn failed");
    let deadline = Instant::now() + Duration::from_secs(secs);
    let code = loop {
        match child.try_wait().expect("try_wait failed") {
            Some(status) => break status.code(),
            None => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };
    // stdout was piped; after exit read whatever the child printed
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        use std::io::Read;
        let _ = stdout.read_to_string(&mut out);
    }
    (code, out)
}

#[test]
fn ui_pure_helpers_utf16_rgb_ids() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }

    // exercises the platform-independent half of stdlib/ui.ax: UTF-8 ->
    // UTF-16 (ASCII, 2/3-byte and astral), COLORREF packing, i32 assembly,
    // widget id injectivity, palette structs. No window is created, so this
    // compiles and runs on every CI platform (the Win32 externs are only
    // declared; the gated call paths are folded away at O3).
    let dir = temp_dir("pure");
    let src = dir.join("ui_pure.ax");
    let exe = dir.join("ui_pure.exe");
    std::fs::write(
        &src,
        format!(
            "import * from \"{}\"\n\n{}",
            abs("stdlib/ui.ax"),
            r#"def main() -> int:
    print(ui_rgb(1, 2, 3))
    p = ui_utf16("A")
    print(load_u8(p, 0))
    print(load_u8(p, 1))
    p = ui_utf16("ö")
    print(load_u8(p, 0))
    p = ui_utf16("€")
    print(load_u8(p, 0) + load_u8(p, 1) * 256)
    p = ui_utf16("😀")
    print(load_u8(p, 0) + load_u8(p, 1) * 256)
    print(load_u8(p, 2) + load_u8(p, 3) * 256)
    buf = malloc(8)
    store_u8(buf, 0, 240)
    store_u8(buf, 1, 255)
    store_u8(buf, 2, 255)
    store_u8(buf, 3, 255)
    print(i32_at(buf))
    print(ui_wid_id(3, 7))
    print(light_palette().accent)
    print(dark_palette().text)
    return 0
"#
        ),
    )
    .unwrap();

    aoxn::build_paths_opts(&[src.display().to_string()], &exe, true, &[], &[])
        .expect("ui pure-logic driver failed to compile");
    let (code, out) = run_with_timeout(&exe, 60);
    assert_eq!(code, Some(0), "ui pure driver exited abnormally");
    let lines: Vec<&str> = out.lines().collect();
    let expected = [
        "197121",      // rgb(1,2,3) = 1 + 2*256 + 3*65536
        "65", "0",     // "A" -> 41 00
        "246",         // U+00F6 -> F6 00
        "8364",        // U+20AC (euro) -> AC 20
        "55357",       // U+1F600 -> D83D (high surrogate)
        "56832",       //           DE00 (low surrogate)
        "-16",         // FF FF FF F0 little-endian i32
        "12884901895", // 3 * 2^32 + 7
        "14120960",    // light accent rgb(0,120,215)
        "15461355",    // dark text rgb(235,235,235) = 235 * 65793
    ];
    assert_eq!(lines.len(), expected.len(), "unexpected output: {out:?}");
    for (got, want) in lines.iter().zip(expected.iter()) {
        assert_eq!(got, want, "ui pure output mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn ui_window_selfclose_smoke() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }

    // creates a real window, runs ~90 frames (~1.5s at the 15ms frame cap),
    // then closes itself cleanly. Exit code 3 = no window (headless CI
    // session) -> the test skips.
    let dir = temp_dir("selfclose");
    let src = dir.join("ui_selfclose.ax");
    let exe = dir.join("ui_selfclose.exe");
    std::fs::write(
        &src,
        format!(
            "import * from \"{}\"\n\n{}",
            abs("stdlib/ui_win.ax"),
            r#"def main() -> int:
    c = ui_init("ui selfclose", 320, 200)
    n = 0
    clicks = 0
    while c.open:
        c = ui_frame(c)
        if not c.open:
            break
        n = n + 1
        if ui_button(c, 12, 40, 100, 30, "noop"):
            clicks = clicks + 1
        ui_label(c, 12, 12, "self close test")
        ui_present(c)
        if n >= 90:
            ui_close(c)
    ui_fini(c)
    if c.w == 0 and c.h == 0:
        print("ui-window: none")
        return 3
    print("ui-selfclose-ok frames=" + str(n) + " clicks=" + str(clicks))
    return 0
"#
        ),
    )
    .unwrap();

    let libs: Vec<String> = vec!["user32".to_string(), "gdi32".to_string()];
    aoxn::build_paths_opts(&[src.display().to_string()], &exe, true, &libs, &[])
        .expect("ui selfclose driver failed to compile");
    let (code, out) = run_with_timeout(&exe, 60);
    let code = code.expect("selfclose driver timed out (killed)");
    if code == 3 {
        // no desktop/window available on this host: the driver reported and
        // skipped cleanly
        assert!(out.contains("ui-window: none"), "skip without report: {out:?}");
        eprintln!("skipped: no window could be created on this host");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    assert_eq!(code, 0, "selfclose driver failed: {out:?}");
    assert!(out.contains("ui-selfclose-ok frames="), "missing ok line: {out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// v2 portable half: layout engine (vbox/hbox/grid/nesting), text editing
/// primitives, UTF-16 queue -> UTF-8, focus chain, disabled mode, the
/// 16-color palette, wheel/overlay slots and tooltip hover timing. Pure
/// logic over the shared heap block — no window, every platform.
#[test]
fn ui_layout_text_focus_portable() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }

    let dir = temp_dir("v2pure");
    let src = dir.join("ui_v2pure.ax");
    let exe = dir.join("ui_v2pure.exe");
    std::fs::write(
        &src,
        format!(
            "import * from \"{}\"\n\n{}",
            abs("stdlib/ui.ax"),
            r#"def main() -> int:
    c = ui_new()
    c = ui_state_init(c)
    ui_vbox_begin(c, 10, 20, 100, 200, 4, 2)
    r = ui_v_item(c, 10)
    print(r.x)
    print(r.y)
    print(r.w)
    r = ui_v_item(c, 20)
    print(r.y)
    ui_spacer(c, 6)
    r = ui_v_item(c, 10)
    print(r.y)
    ui_layout_end(c)
    ui_hbox_begin(c, 0, 0, 1000, 50, 0, 0)
    r = ui_h_item_p(c, 400)
    print(r.w)
    r = ui_h_item_p(c, 600)
    print(r.x)
    print(r.w)
    ui_layout_end(c)
    ui_grid_begin(c, 0, 0, 100, 100, 3, 0, 2)
    ui_grid_row(c, 8)
    g = ui_grid_cell(c)
    print(g.x)
    print(g.w)
    g = ui_grid_cell(c)
    print(g.x)
    g = ui_grid_cell(c)
    print(g.w)
    g = ui_grid_cell(c)
    print(g.y)
    ui_layout_end(c)
    ui_hbox_begin(c, 0, 0, 200, 100, 0, 0)
    r = ui_h_item(c, 50)
    ui_vbox_begin(c, r.x, r.y, r.w, r.h, 0, 4)
    r2 = ui_v_item(c, 10)
    print(r2.w)
    print(r2.h)
    ui_layout_end(c)
    ui_layout_end(c)
    print(str_insert("ac", 1, "b"))
    print(str_remove("abc", 1, 2))
    print(str_sub("hello", 1, 3))
    print(caret_left("abc", 2))
    print(caret_right("abc", 1))
    print(caret_left("aé", 2))
    print(caret_right("aé", 1))
    ui_char_push(c, 65)
    ui_char_push(c, 55296)
    ui_char_push(c, 56832)
    ui_char_push(c, 66)
    s = ui_char_str(c)
    print(len(s))
    print(ui_char_count(c))
    ui_focus_reg(c, 111)
    ui_focus_reg(c, 222)
    ui_focus_reg(c, 333)
    focus_swap(c)
    ui_focus(c, 111)
    ui_focus_next(c, False)
    print(ui_focus_id(c))
    ui_focus_next(c, True)
    print(ui_focus_id(c))
    ui_focus_next(c, True)
    print(ui_focus_id(c))
    d = 0
    if ui_disabled(c):
        d = 1
    print(d)
    ui_begin_disabled(c)
    d = 0
    if ui_disabled(c):
        d = 1
    print(d)
    ui_end_disabled(c)
    d = 0
    if ui_disabled(c):
        d = 1
    print(d)
    p = ui_palette_get(c)
    print(p.sel_text)
    print(p.tooltip_bg)
    print(ui_wheel(c))
    print(ui_overlay_kind(c))
    print(ui_hover_ms(c, 7, 1000))
    print(ui_hover_ms(c, 7, 1600))
    ui_hover_reset(c)
    print(ui_hover_ms(c, 7, 1700))
    items = ["x", "y"]
    overlay_items_store(c, items, 2)
    print(overlay_item_str(c, 1))
    ui_state_free(c)
    return 0
"#
        ),
    )
    .unwrap();

    aoxn::build_paths_opts(&[src.display().to_string()], &exe, true, &[], &[])
        .expect("ui v2 portable driver failed to compile");
    let (code, out) = run_with_timeout(&exe, 60);
    assert_eq!(code, Some(0), "ui v2 portable driver exited abnormally: {out:?}");
    let lines: Vec<&str> = out.lines().collect();
    let expected = [
        "14", "24", "92", // vbox item 1 (margin 4, spacing 2)
        "36",             // vbox item 2
        "64",             // vbox item 3 after a 6px spacer
        "400",            // h_item_p 400/1000 of 1000
        "400", "600",     // h_item_p 600: x after the first, its width
        "0", "32",        // grid cell 0
        "34",             // grid cell 1 (32 + spacing 2)
        "32",             // grid cell 2 (last column absorbs rounding)
        "10",             // grid cell 3 wrapped to the second row
        "50", "10",       // nested vbox inside an hbox item
        "abc",            // str_insert
        "ac",             // str_remove
        "el",             // str_sub
        "1", "2",         // caret_left / caret_right on ASCII
        "1", "3",         // caret moves land on UTF-8 boundaries (é = 2 bytes)
        "6", "4",         // WM_CHAR queue -> UTF-8: A + U+1F600 + B = 6 bytes
        "222", "111", "333", // tab chain: forward, back, back wraps
        "0", "1", "0",    // disabled nesting counter
        "16777215",       // sel_text = white
        "15793404",       // tooltip_bg = rgb(252,252,240)
        "0", "0",         // wheel + overlay slots start empty
        "0", "600", "0",  // tooltip hover timing + reset
        "y",              // overlay item pointers survive the copy
    ];
    assert_eq!(lines.len(), expected.len(), "unexpected output: {out:?}");
    for (got, want) in lines.iter().zip(expected.iter()) {
        assert_eq!(got, want, "ui v2 portable output mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// v2 widgets in a real window: textbox, radio, toggle, spin box, list box,
/// combo box, tabs, group box, scroll area and tooltip run for ~60 frames
/// and close themselves. Skips (exit 3) where no window can be created.
#[cfg(windows)]
#[test]
fn ui_window_v2_widgets_smoke() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }

    let dir = temp_dir("v2smoke");
    let src = dir.join("ui_v2smoke.ax");
    let exe = dir.join("ui_v2smoke.exe");
    // 40 items: past the 32-slot popup clamp — the combobox must build its
    // popup record (and scroll math) against the clamped count without
    // ever reading an out-of-range overlay slot
    std::fs::write(
        &src,
        format!(
            "import * from \"{}\"\n\n{}",
            abs("stdlib/ui_win.ax"),
            r#"def main() -> int:
    c = ui_init("ui v2 smoke", 640, 480)
    items = ["one", "two", "three"]
    big = ["i0", "i1", "i2", "i3", "i4", "i5", "i6", "i7", "i8", "i9", "i10", "i11", "i12", "i13", "i14", "i15", "i16", "i17", "i18", "i19", "i20", "i21", "i22", "i23", "i24", "i25", "i26", "i27", "i28", "i29", "i30", "i31", "i32", "i33", "i34", "i35", "i36", "i37", "i38", "i39"]
    tabs = ["A", "B"]
    txt = "hi"
    n = 0
    while c.open:
        c = ui_frame(c)
        if not c.open:
            break
        n = n + 1
        te = ui_textbox(c, 10, 10, 200, 26, txt)
        txt = te.text
        ui_radio(c, 10, 50, "r1", 1, 0)
        ui_radio(c, 10, 76, "r2", 1, 1)
        ui_toggle(c, 10, 106, 120, 26, "t", True)
        ui_spinbox(c, 10, 142, 100, 1, 0, 5)
        ui_listbox(c, 10, 182, 180, 90, items, 3, 0, 0)
        ui_combobox(c, 10, 284, 180, big, 40, 0)
        ui_tabs(c, 10, 326, 200, tabs, 2, 0)
        ui_groupbox(c, 10, 368, 200, 80, "g")
        sc = ui_scroll_begin(c, 230, 10, 200, 150, 0, 600)
        ui_label(c, 240, 12 - sc, "x")
        ui_scroll_end(c)
        c = ui_font_mono(c)
        sc2 = ui_scroll_begin_h(c, 230, 180, 180, 70, 0, 360)
        ui_label(c, 240 - sc2, 182, "0123456789 wide content")
        ui_scroll_end_h(c)
        c = ui_font_sans(c)
        ui_tooltip(c, 230, 260, 80, 24, "tip")
        ui_present(c)
        if n >= 60:
            ui_close(c)
    ui_fini(c)
    if c.w == 0 and c.h == 0:
        print("ui-window: none")
        return 3
    print("ui-v2-ok frames=" + str(n) + " text=" + txt)
    return 0
"#
        ),
    )
    .unwrap();

    let libs: Vec<String> = vec!["user32".to_string(), "gdi32".to_string()];
    aoxn::build_paths_opts(&[src.display().to_string()], &exe, true, &libs, &[])
        .expect("ui v2 smoke driver failed to compile");
    let (code, out) = run_with_timeout(&exe, 60);
    let code = code.expect("v2 smoke driver timed out (killed)");
    if code == 3 {
        assert!(out.contains("ui-window: none"), "skip without report: {out:?}");
        eprintln!("skipped: no window could be created on this host");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    assert_eq!(code, 0, "v2 smoke driver failed: {out:?}");
    assert!(out.contains("ui-v2-ok frames="), "missing ok line: {out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// v3 portable half: text selection, the line cache, edit operations
/// (type-over-selection, backspace join, UTF-8 up/down with shift), the
/// signal-slot event bus (connect/emit/queue/clear), and the heap-backed
/// table/tree models. Pure logic over the shared heap block — no window,
/// every platform.
#[test]
fn ui_v3_portable() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }

    let dir = temp_dir("v3pure");
    let src = dir.join("ui_v3pure.ax");
    let exe = dir.join("ui_v3pure.exe");
    std::fs::write(
        &src,
        format!(
            "import * from \"{}\"\n\n{}",
            abs("stdlib/ui.ax"),
            r#"def main() -> int:
    c = ui_new()
    c = ui_state_init(c)
    s = sel_make(5)
    print(sel_active(s))
    s2 = Sel(anchor=2, caret=7)
    print(sel_lo(s2))
    print(sel_hi(s2))
    buf = malloc(16)
    nb = utf16_write_sub("aébc", 1, 3, buf)
    print(nb)
    print(load_u8(buf, 0) + load_u8(buf, 1) * 256)
    lines_sync(c, 1, "ab" + "\n" + "cde" + "\n" + "")
    print(line_count(c))
    print(line_start(c, 1))
    print(line_end(c, 1))
    print(line_of(c, 4))
    e = edit_type(c, "hello", 1, 3, "XY")
    print(e.text)
    print(e.caret)
    e = edit_backspace(c, "abc", 3, 3)
    print(e.text)
    e = edit_backspace(c, "abc", 1, 3)
    print(e.text)
    e = edit_delete(c, "abc", 1, 1)
    print(e.text)
    lines_sync(c, 2, "ab" + "\n" + "cdef")
    m = edit_up(c, "ab" + "\n" + "cdef", 4, 4, False)
    print(m.caret)
    m = edit_down(c, "ab" + "\n" + "cdef", 1, 1, True)
    print(m.caret)
    print(m.anchor)
    ui_connect(c, 7, 42)
    ui_emit(c, 7, 1, 2, 3)
    ui_emit(c, 8, 9, 0, 0)
    print(ui_event_count(c))
    ev = ui_event(c, 0)
    print(ev.slot)
    print(ev.sig)
    print(ev.kind)
    print(ev.b)
    ev2 = ui_event(c, 1)
    print(ev2.slot)
    ui_events_clear(c)
    print(ui_event_count(c))
    tm = table_model_new(2, 3)
    tm_set_header(tm, 0, "name")
    tm_set(tm, 1, 2, "z")
    print(tm_rows(tm))
    print(tm_cols(tm))
    print(tm_header(tm, 0))
    print(tm_get(tm, 1, 2))
    tm_set_colw(tm, 2, 80)
    print(tm_colw(tm, 2))
    tr = tree_model_new(4)
    tree_set_label(tr, 0, "root")
    tree_set_label(tr, 1, "kid")
    tree_set_parent(tr, 1, 0)
    tree_set_parent(tr, 2, 1)
    tree_set_expanded(tr, 0, False)
    print(tree_label(tr, 1))
    print(tree_depth(tr, 2))
    print(tree_visible(tr, 2))
    tree_set_expanded(tr, 0, True)
    tree_set_expanded(tr, 1, True)
    print(tree_visible(tr, 2))
    print(tree_has_child(tr, 0))
    print(tree_has_child(tr, 1))
    print(tree_has_child(tr, 3))
    tree_set_parent(tr, 2, 0)
    print(tree_has_child(tr, 1))
    print(tree_has_child(tr, 0))
    print(tree_nth_visible(tr, 1))
    print(tree_nth_visible(tr, 99))
    print(overlay_item_str(c, 40))
    print(overlay_item_str(c, -1))
    ui_state_free(c)
    return 0
"#
        ),
    )
    .unwrap();

    aoxn::build_paths_opts(&[src.display().to_string()], &exe, true, &[], &[])
        .expect("ui v3 portable driver failed to compile");
    let (code, out) = run_with_timeout(&exe, 60);
    assert_eq!(code, Some(0), "ui v3 portable driver exited abnormally: {out:?}");
    let lines: Vec<&str> = out.lines().collect();
    let expected = [
        "false",          // sel_active(sel_make(5))
        "2", "7",         // sel_lo / sel_hi
        "2", "233",       // utf16_write_sub("aébc", 1, 3): bytes, U+00E9 LE byte0
        "3",              // line_count("ab\ncde\n") = 3
        "3", "6",         // line_start(1)=3, line_end(1)=6
        "1",              // line_of(4) = 1
        "hXYlo", "3",     // edit_type over selection [1,3)
        "ab",             // backspace at 3
        "a",              // backspace selection [1,3)
        "ac",             // delete at 1
        "1",              // edit_up caret (col 1 on line 0)
        "4", "1",         // edit_down shift: caret 4, anchor stays 1
        "2",              // event count (signal 7 connected, 8 not)
        "42", "7", "1", "3", // ev0: slot 42, sig 7, kind 1, b 3
        "0",              // ev1.slot = 0 (signal 8 unconnected)
        "0",              // events cleared
        "2", "3",         // table rows / cols
        "name", "z",      // header / cell
        "80",             // col width
        "kid",            // tree label
        "2",              // tree_depth(2) (2 -> 1 -> 0)
        "false",          // visible before expansion
        "true",           // visible after expanding ancestors
        "true",           // tree_has_child(0): node 1 points at it
        "true",           // tree_has_child(1): node 2 points at it
        "false",          // tree_has_child(3): leaf
        "false",          // after reparenting 2 -> 0, node 1 is a leaf
        "true",           // ...and node 0 has both remaining children
        "1",              // tree_nth_visible(1) = node 1 (all expanded)
        "-1",             // tree_nth_visible past the end
        "",               // overlay_item_str clamps out-of-range to ""
        "",               // ...on both sides
    ];
    assert_eq!(lines.len(), expected.len(), "unexpected output: {out:?}");
    for (got, want) in lines.iter().zip(expected.iter()) {
        assert_eq!(got, want, "ui v3 portable output mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// v3 widgets in a real window: multi-line editor, menu bar + menu, tree,
/// table and a signal-slot dispatch loop, ~60 frames then self-close.
/// Skips (exit 3) where no window can be created.
#[cfg(windows)]
#[test]
fn ui_window_v3_widgets_smoke() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }

    let dir = temp_dir("v3smoke");
    let src = dir.join("ui_v3smoke.ax");
    let exe = dir.join("ui_v3smoke.exe");
    std::fs::write(
        &src,
        format!(
            "import * from \"{}\"\n\n{}",
            abs("stdlib/ui_win.ax"),
            r#"def main() -> int:
    c = ui_init("ui v3 smoke", 900, 600)
    tm = table_model_new(4, 2)
    tm_set_header(tm, 0, "name")
    tm_set_header(tm, 1, "val")
    tm_set(tm, 0, 0, "a")
    tm_set(tm, 0, 1, "1")
    tm_set(tm, 1, 0, "b")
    tm_set(tm, 1, 1, "2")
    tr = tree_model_new(3)
    tree_set_label(tr, 0, "root")
    tree_set_label(tr, 1, "kid")
    tree_set_parent(tr, 1, 0)
    tree_set_label(tr, 2, "leaf")
    tree_set_parent(tr, 2, 1)
    tree_set_expanded(tr, 0, True)
    tree_set_expanded(tr, 1, True)
    menus = ["File", "Help"]
    items = ["New", "Open", "Exit"]
    doc = "line one\nline two\nline three"
    anchor = 0
    caret = 0
    escroll = 0
    tsel = 0
    tscroll = 0
    trsel = 2
    trscroll = 0
    menu_open = -1
    name = "hi"
    picks = 0
    ui_connect(c, 1, 100)
    n = 0
    while c.open:
        c = ui_frame(c)
        if not c.open:
            break
        n = n + 1
        bar = ui_menubar(c, 0, 0, c.w, menus, 2, menu_open)
        menu_open = bar.open
        if bar.open == 0:
            mp = ui_menu(c, bar, items, 3)
            menu_open = mp.open
            if mp.pick >= 0:
                picks = picks + 1
                ui_emit(c, 1, 1, mp.pick, 0)
        ev = ui_textedit(c, 20, 40, 520, 240, doc, anchor, caret, escroll)
        doc = ev.text
        anchor = ev.anchor
        caret = ev.caret
        escroll = ev.scroll
        te = ui_textbox(c, 20, 300, 240, 28, name)
        name = te.text
        tr2 = ui_tree(c, 560, 40, 200, 200, tr, trsel, trscroll)
        trsel = tr2.sel
        trscroll = tr2.scroll
        tb = ui_table(c, 560, 260, 320, 200, tm, tsel, tscroll)
        tsel = tb.sel
        tscroll = tb.scroll
        k = ui_event_count(c)
        i = 0
        while i < k:
            e0 = ui_event(c, i)
            if e0.slot == 100:
                picks = picks + 100
            i = i + 1
        ui_present(c)
        if n >= 60:
            ui_close(c)
    ui_fini(c)
    if c.w == 0 and c.h == 0:
        print("ui-window: none")
        return 3
    print("ui-v3-ok frames=" + str(n) + " picks=" + str(picks) + " doc=" + str(len(doc)))
    return 0
"#
        ),
    )
    .unwrap();

    let libs: Vec<String> = vec!["user32".to_string(), "gdi32".to_string()];
    aoxn::build_paths_opts(&[src.display().to_string()], &exe, true, &libs, &[])
        .expect("ui v3 smoke driver failed to compile");
    let (code, out) = run_with_timeout(&exe, 60);
    let code = code.expect("v3 smoke driver timed out (killed)");
    if code == 3 {
        assert!(out.contains("ui-window: none"), "skip without report: {out:?}");
        eprintln!("skipped: no window could be created on this host");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    assert_eq!(code, 0, "v3 smoke driver failed: {out:?}");
    assert!(out.contains("ui-v3-ok frames="), "missing ok line: {out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// v0.47.0 — the clip stack
//
// Everything above asserts that a frame does not crash. This one asserts that
// it PAINTS: the driver draws known rects in known colors and reads the
// canvas back with GetPixel, so a broken clip is a failed expectation rather
// than a blank window nobody looks at.
//
// It is here because of a bug that every smoke test passed straight through:
// plat_clip_pop could not restore anything (GDI's IntersectClipRect only ever
// narrows), so after ONE ui_textbox anywhere in a frame the whole window —
// including the final BitBlt, which copies only what the source clip allows —
// stayed clipped to that textbox rect for the rest of the process. The three
// cases below are exactly that: "after-textbox", the nested push/pop pair, and
// the culled draw the widget layer now skips on its own.
// ---------------------------------------------------------------------------
#[test]
#[cfg(windows)]
fn ui_clip_stack_restores_and_culls() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }

    let dir = temp_dir("clip");
    let src = dir.join("ui_clip.ax");
    let exe = dir.join("ui_clip.exe");
    std::fs::write(
        &src,
        format!(
            "import * from \"{}\"\n\nextern def GetPixel(dc: int, x: int, y: int) -> int\n\n{}\n",
            abs("stdlib/ui_win.ax"),
            r#"def check(name: string, got: int, want: int) -> int:
    if got != want:
        print("FAIL " + name + " got=" + str(got) + " want=" + str(want))
        return 1
    print("PASS " + name + " " + str(got))
    return 0

def main() -> int:
    c = ui_init("clip", 400, 300)
    if not c.open:
        print("ui-window: none")
        return 3
    c.cap_ms = 0
    red = ui_rgb(255, 0, 0)
    grn = ui_rgb(0, 255, 0)
    blu = ui_rgb(0, 0, 255)
    wht = ui_rgb(255, 255, 255)
    pal = ui_palette_get(c)
    fails = 0
    # a plain fill reaches the canvas
    c = ui_frame(c)
    ui_fill_rect(c, 10, 10, 50, 50, red)
    ui_present(c)
    fails = fails + check("fill", GetPixel(c.mem_dc, 20, 20), red)
    # a frame rect paints its border and leaves its inside alone
    ui_frame_rect(c, 100, 10, 50, 50, blu)
    ui_present(c)
    fails = fails + check("frame-border", GetPixel(c.mem_dc, 100, 10), blu)
    fails = fails + check("frame-inside", GetPixel(c.mem_dc, 120, 30), pal.bg)
    # the regression: a widget drawn AFTER a clipping one must still paint
    c = ui_frame(c)
    ui_textbox(c, 24, 40, 280, 30, "hello")
    ui_present(c)
    c = ui_frame(c)
    ui_fill_rect(c, 200, 150, 60, 60, grn)
    ui_present(c)
    fails = fails + check("after-textbox", GetPixel(c.mem_dc, 210, 160), grn)
    # nested clips: the inner pop gives back the OUTER clip, the outer pop
    # gives back the canvas
    c = ui_frame(c)
    ui_clip_push(c, 0, 0, 100, 300)
    ui_fill_rect(c, 0, 0, 100, 300, wht)
    ui_clip_push(c, 0, 0, 100, 100)
    ui_fill_rect(c, 0, 0, 100, 100, red)
    ui_clip_pop(c)
    ui_fill_rect(c, 0, 150, 100, 50, grn)
    ui_present(c)
    fails = fails + check("nested-inner", GetPixel(c.mem_dc, 50, 50), red)
    fails = fails + check("nested-outer", GetPixel(c.mem_dc, 50, 170), grn)
    c = ui_frame(c)
    ui_clip_pop(c)
    ui_fill_rect(c, 200, 200, 60, 60, blu)
    ui_present(c)
    fails = fails + check("after-outer-pop", GetPixel(c.mem_dc, 210, 210), blu)
    # a draw the clip cannot show never reaches the canvas
    c = ui_frame(c)
    ui_clip_push(c, 0, 0, 50, 50)
    ui_fill_rect(c, 300, 100, 40, 40, grn)
    ui_draw_text(c, 300, 100, "offscreen", grn)
    ui_clip_pop(c)
    fails = fails + check("culled", GetPixel(c.mem_dc, 310, 110), pal.bg)
    ui_fill_rect(c, 300, 100, 40, 40, grn)
    fails = fails + check("drawn-after-cull", GetPixel(c.mem_dc, 310, 110), grn)
    # the DC state cache must not survive a RestoreDC with a stale answer:
    # two fills in a row must each take their own color
    c = ui_frame(c)
    ui_clip_push(c, 0, 0, 400, 300)
    ui_clip_pop(c)
    ui_fill_rect(c, 10, 250, 40, 40, red)
    ui_fill_rect(c, 60, 250, 40, 40, grn)
    ui_present(c)
    fails = fails + check("cache-red", GetPixel(c.mem_dc, 20, 260), red)
    fails = fails + check("cache-green", GetPixel(c.mem_dc, 70, 260), grn)
    # the measure cache answers a repeat with the same numbers, and still
    # tells two different strings apart
    c = ui_frame(c)
    a = ui_measure(c, "measure me")
    b = ui_measure(c, "measure me")
    d = ui_measure(c, "measure you")
    fails = fails + check("measure-stable", b.w, a.w)
    if a.w == d.w:
        print("FAIL measure-distinct a=" + str(a.w) + " d=" + str(d.w))
        fails = fails + 1
    else:
        print("PASS measure-distinct " + str(a.w) + "/" + str(d.w))
    print("DONE fails=" + str(fails))
    ui_fini(c)
    return 0
"#
        ),
    )
    .unwrap();

    let libs: Vec<String> = vec!["user32".to_string(), "gdi32".to_string()];
    aoxn::build_paths_opts(&[src.display().to_string()], &exe, true, &libs, &[])
        .expect("ui clip driver failed to compile");
    let (code, out) = run_with_timeout(&exe, 60);
    let code = code.expect("clip driver timed out (killed)");
    if code == 3 {
        assert!(out.contains("ui-window: none"), "skip without report: {out:?}");
        eprintln!("skipped: no window could be created on this host");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    // every mismatch, not just the first: one drifted line per run is a slow
    // way to read a table of expectations
    let failed: Vec<&str> = out.lines().filter(|l| l.starts_with("FAIL")).collect();
    assert!(failed.is_empty(), "clip/draw expectations failed: {failed:?}\nfull:\n{out}");
    assert!(
        out.contains("DONE fails=0"),
        "clip driver reported failures: {out}"
    );
    assert_eq!(code, 0, "clip driver failed: {out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Layout depth accounting past the 8-level cap, and itoa10's arena escape.
///
/// The first half is about a stack that used to lie. `lay_push` REFUSED the
/// 9th box by returning without pushing, but `ui_layout_end` decremented the
/// depth anyway — so one over-deep nesting popped its own 8th box, and every
/// later end unwound a level too deep: the rest of the frame's widgets landed
/// in the wrong box, with nothing anywhere reporting it. The push and its end
/// now balance against an overflow counter that an app or a test can read.
///
/// The second half pins the arena-exhaustion path of itoa10: the digits must
/// still come out right when the bump arena has no room, since that malloc
/// escape is what the whole "steady-state frames allocate nothing" property
/// leans on.
#[test]
fn ui_layout_overflow_and_arena_escape() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }

    let dir = temp_dir("layovf");
    let src = dir.join("ui_layovf.ax");
    let exe = dir.join("ui_layovf.exe");
    std::fs::write(
        &src,
        format!(
            "import * from \"{}\"\n\n{}",
            abs("stdlib/ui.ax"),
            r#"def main() -> int:
    c = ui_new()
    c = ui_state_init(c)
    # eight real levels
    i = 0
    while i < 8:
        ui_vbox_begin(c, 0, 0, 100, 100, 0, 0)
        i = i + 1
    print(str(ui_layout_depth(c)) + " " + str(ui_layout_overflow(c)))
    # the 9th and 10th must be REFUSED and must not disturb the 8 live ones
    ui_vbox_begin(c, 0, 0, 100, 100, 0, 0)
    ui_hbox_begin(c, 0, 0, 100, 100, 0, 0)
    print(str(ui_layout_depth(c)) + " " + str(ui_layout_overflow(c)))
    # two matching ends drain the overflow WITHOUT popping a live box: this
    # is the assertion that fails on the old code (depth would read 6)
    ui_layout_end(c)
    ui_layout_end(c)
    print(str(ui_layout_depth(c)) + " " + str(ui_layout_overflow(c)))
    # and the innermost live box still owns the cursor
    r = ui_v_item(c, 7)
    print(r.y)
    # unwinding the eight real levels empties the stack
    i = 0
    while i < 8:
        ui_layout_end(c)
        i = i + 1
    print(str(ui_layout_depth(c)) + " " + str(ui_layout_overflow(c)))
    # an extra end at depth 0 is a no-op, not an underflow
    ui_layout_end(c)
    print(str(ui_layout_depth(c)) + " " + str(ui_layout_overflow(c)))
    # itoa10 renders the digits both in the arena and via the escape. The two
    # paths must return the SAME KIND of buffer: ui_label_int draws the result
    # through plat_text_sub, which re-encodes it as UTF-8, so an escape that
    # returned UTF-16 drew a one-digit number once the arena filled.
    p1 = itoa10(c, -4321)
    s1 = as_string(p1)
    n1 = len(s1)
    print(s1 + "/" + str(n1))
    st_set(c, 3, st_get(c, 2))
    p2 = itoa10(c, -4321)
    s2 = as_string(p2)
    print(s2 + "/" + str(len(s2)))
    print("same " + str(s1 == s2))
    st_set(c, 3, st_get(c, 2) + 65536)
    print(as_string(itoa10(c, 0)))
    ui_state_free(c)
    return 0
"#
        ),
    )
    .unwrap();

    aoxn::build_paths_opts(&[src.display().to_string()], &exe, true, &[], &[])
        .expect("ui layout-overflow driver failed to compile");
    let (code, out) = run_with_timeout(&exe, 60);
    assert_eq!(
        code,
        Some(0),
        "ui layout-overflow driver exited abnormally: {out:?}"
    );
    let lines: Vec<&str> = out.lines().collect();
    let expected = [
        "8 0",       // eight real levels, no overflow
        "8 2",       // two refused pushes: depth untouched, overflow counted
        "8 0",       // two ends drain the overflow without popping a live box
        "0",         // the innermost live box still owns the cursor
        "0 0",       // unwinding the eight real levels empties the stack
        "0 0",       // an extra end at depth 0 is a no-op
        "-4321/5",   // itoa10 in the arena: the digits AND their length
        "-4321/5",   // ...and through the escape with the arena exhausted
        "same true", // ...identical text, not UTF-16 in one arm of the if
        "0",         // zero is still "0", not ""
    ];
    assert_eq!(lines.len(), expected.len(), "unexpected output: {out:?}");
    for (got, want) in lines.iter().zip(expected.iter()) {
        assert_eq!(got, want, "layout-overflow output mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The press-claim helpers must behave exactly like the fifteen widget bodies
/// they replaced, because every interactive widget now routes through them.
///
/// This drives the claim state machine directly, which the window smoke tests
/// cannot: they assert a frame does not crash, not that a click lands. What is
/// pinned is the four things the copies each got subtly right, and one they
/// did not (the overlay sentinel must never be stolen):
///   - a press INSIDE claims and focuses;
///   - a release on the next frame reports the click, and only then;
///   - a release OUTSIDE still frees the claim (a drag that ends off-widget
///     must not leave the slot stuck forever, which would wedge every
///     subsequent press);
///   - a press edge while slot 0 is -1 (an open popup ate the click) is
///     refused, so a widget drawn under the overlay cannot steal the release.
#[cfg(windows)]
#[test]
fn ui_press_claim_cycle() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }

    let dir = temp_dir("press");
    let src = dir.join("ui_press.ax");
    let exe = dir.join("ui_press.exe");
    std::fs::write(
        &src,
        format!(
            "import * from \"{}\"\n\n{}",
            abs("stdlib/ui_win.ax"),
            r#"def main() -> int:
    c = ui_init("ui press", 320, 200)
    if not c.open:
        print("ui-window: none")
        return 3
    c.cap_ms = 0
    a = ui_wid_id(10, 10)
    b = ui_wid_id(100, 100)
    # --- a press INSIDE claims and takes focus, and is not yet a click.
    # Each step is its own statement: a print that CONCATENATES a mutating
    # call with a read of the same slot does not sequence them the way the
    # source reads, so the slot is read into a local first.
    st_set(c, 0, 0)
    c.dn_prev = False
    c.dn = True
    took = ui_press_claim(c, a, True)
    holds = ui_pressed(c, a)
    print("claim " + str(took) + " " + str(holds))
    print("click1 " + str(ui_press(c, a, True)))
    print("held " + str(ui_pressed(c, a)))
    # --- a second widget must NOT steal a held claim
    stole = ui_press_claim(c, b, True)
    print("steal " + str(stole) + " " + str(ui_pressed(c, a)))
    # --- release on the next frame IS the click, and frees the slot
    c.dn_prev = True
    c.dn = False
    hit = ui_press(c, a, True)
    free0 = st_get(c, 0)
    print("click2 " + str(hit) + " " + str(free0))
    # --- a release OUTSIDE frees the claim but is not a click: this is the
    # case that used to wedge slot 0 at the widget id forever
    st_set(c, 0, 0)
    c.dn_prev = False
    c.dn = True
    ui_press_claim(c, a, True)
    c.dn_prev = True
    c.dn = False
    miss = ui_press(c, a, False)
    free1 = st_get(c, 0)
    print("outside " + str(miss) + " " + str(free1))
    # --- the overlay sentinel (-1) is never stolen
    st_set(c, 0, -1)
    c.dn_prev = False
    c.dn = True
    sn0 = ui_press_claim(c, a, True)
    sn1 = st_get(c, 0)
    print("sentinel " + str(sn0) + " " + str(sn1))
    print("sentinel-click " + str(ui_press(c, a, True)))
    # --- wheel + clamp: notches down, never below zero, never past the end
    print(str(ui_clamp_scroll(5, 3)) + " " + str(ui_clamp_scroll(-4, 3)) + " " + str(ui_clamp_scroll(2, 3)))
    ui_fini(c)
    return 0
"#
        ),
    )
    .unwrap();

    let libs: Vec<String> = vec!["user32".to_string(), "gdi32".to_string()];
    aoxn::build_paths_opts(&[src.display().to_string()], &exe, true, &libs, &[])
        .expect("ui press driver failed to compile");
    let (code, out) = run_with_timeout(&exe, 60);
    let code = code.expect("press driver timed out (killed)");
    if code == 3 {
        assert!(out.contains("ui-window: none"), "skip without report: {out:?}");
        eprintln!("skipped: no window could be created on this host");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    assert_eq!(code, 0, "press driver failed: {out:?}");
    let lines: Vec<&str> = out.lines().collect();
    let expected = [
        "claim true true",      // a press inside claims and focuses
        "click1 false",         // ...and is not yet a click (same edge frame)
        "held true",            // the slot stays claimed for the whole hold
        "steal false true",     // a second widget cannot steal a held claim
        "click2 true 0",        // the release frame is the click, and frees it
        "outside false 0",      // a release outside frees the claim, no click
        "sentinel false -1",    // an overlay-eaten press is not stolen
        "sentinel-click false", // ...so the widget under it never fires
        "3 0 2",                // clamp: over, under, in range
    ];
    assert_eq!(lines.len(), expected.len(), "unexpected output: {out:?}");
    for (got, want) in lines.iter().zip(expected.iter()) {
        assert_eq!(got, want, "press-cycle output mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `ui_font_height` must be a stable, font-derived constant — the text box
/// used to measure its WHOLE field every frame purely to read `ts.h`, which
/// the v0.47.0 measure cache turned into an O(n) content hash per frame to
/// learn something that does not depend on the text at all.
///
/// So: one call, and the cached value must be identical on every later call
/// (a cache that recomputed would defeat the point), positive, and no larger
/// than the height of a real glyph run.
#[cfg(windows)]
#[test]
fn ui_font_height_is_a_cached_constant() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }

    let dir = temp_dir("fonth");
    let src = dir.join("ui_fonth.ax");
    let exe = dir.join("ui_fonth.exe");
    std::fs::write(
        &src,
        format!(
            "import * from \"{}\"\n\n{}",
            abs("stdlib/ui_win.ax"),
            r#"def main() -> int:
    c = ui_init("ui font height", 320, 200)
    if not c.open:
        print("ui-window: none")
        return 3
    c.cap_ms = 0
    # first call measures and caches; later calls must return the same number
    h1 = ui_font_height(c)
    h2 = ui_font_height(c)
    h3 = ui_font_height(c)
    print("stable " + str(h1 == h2 and h2 == h3))
    print("positive " + str(h1 > 0))
    # a wide string must not change it
    wide = ui_measure(c, "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM")
    print("bounded " + str(h1 <= wide.h))
    # a font switch must invalidate it (the height belongs to the face)
    c = ui_font_mono(c)
    print("cleared " + str(st_get(c, 830)))
    h4 = ui_font_height(c)
    print("recovered " + str(h4 > 0))
    ui_fini(c)
    return 0
"#
        ),
    )
    .unwrap();

    let libs: Vec<String> = vec!["user32".to_string(), "gdi32".to_string()];
    aoxn::build_paths_opts(&[src.display().to_string()], &exe, true, &libs, &[])
        .expect("ui font-height driver failed to compile");
    let (code, out) = run_with_timeout(&exe, 60);
    let code = code.expect("font-height driver timed out (killed)");
    if code == 3 {
        assert!(out.contains("ui-window: none"), "skip without report: {out:?}");
        eprintln!("skipped: no window could be created on this host");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    assert_eq!(code, 0, "font-height driver failed: {out:?}");
    let lines: Vec<&str> = out.lines().collect();
    let expected = [
        "stable true",    // cached: three calls, one measurement
        "positive true",  // a real font has a real line height
        "bounded true",   // not taller than a 40-glyph measure
        "cleared 0",      // ui_font_mono drops the old face's height
        "recovered true", // ...and the next call re-measures in the new one
    ];
    assert_eq!(lines.len(), expected.len(), "unexpected output: {out:?}");
    for (got, want) in lines.iter().zip(expected.iter()) {
        assert_eq!(got, want, "font-height output mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// v0.30.0 — Windows-only toolkit
//
// The toolkit is three files: ui.ax (portable core), ui_draw.ax (the
// platform-neutral widget layer) and the Win32/GDI backend (ui_win.ax).
// Aoxn is a Windows-only language, so there is exactly one backend and these
// tests pin the two properties the split still has to hold:
//
//   1. The backend typechecks and emits C. `compile_paths_to_c` stops after
//      codegen, so this runs with NO clang and NO display.
//   2. The widget layer is genuinely platform-neutral: it must not mention a
//      single Win32 symbol — a widget that reaches for one would drag the
//      whole layer into the backend.
// ---------------------------------------------------------------------------

/// Typecheck + emit C for a program that imports `backend`. Needs neither
/// clang nor a window server, so it runs everywhere.
fn emit_c_with_backend(backend: &str, body: &str) -> String {
    let dir = temp_dir(&format!("backend-{}", backend));
    let src = dir.join("app.ax");
    std::fs::write(
        &src,
        format!(
            "import * from \"{}\"\n\n{}",
            abs(&format!("stdlib/{backend}")),
            body
        ),
    )
    .unwrap();
    let c = aoxn::compile_paths_to_c(&[src.display().to_string()], true)
        .unwrap_or_else(|d| panic!("{backend} failed to compile: {d:?}"));
    let _ = std::fs::remove_dir_all(&dir);
    c
}

#[test]
fn ui_win_backend_emits_c() {
    let c = emit_c_with_backend(
        "ui_win.ax",
        r#"def main() -> int:
    c = ui_init("t", 200, 100)
    while c.open:
        c = ui_frame(c)
        ui_button(c, 10, 10, 80, 24, "ok")
        ui_present(c)
    ui_fini(c)
    return 0
"#,
    );
    assert!(c.contains("CreateWindowExW"), "Win32 windowing missing");
    assert!(c.contains("BitBlt"), "GDI blit missing");
}

/// The X11 backend must typecheck and emit C with no X11 library present —
/// the externs are prototypes, so this runs on every platform (Linux CI
/// runs it too, before the Xvfb probe exercises the real link).
#[test]
fn ui_x11_backend_emits_c() {
    let c = emit_c_with_backend(
        "ui_x11.ax",
        r#"def main() -> int:
    c = ui_init("t", 200, 100)
    while c.open:
        c = ui_frame(c)
        ui_button(c, 10, 10, 80, 24, "ok")
        ui_present(c)
    ui_fini(c)
    return 0
"#,
    );
    assert!(c.contains("XOpenDisplay"), "X11 windowing missing");
    assert!(c.contains("XftDrawStringUtf8"), "Xft text missing");
    assert!(c.contains("XQueryKeymap"), "keymap sweep missing");
}

/// The whole point of ui_draw.ax: one widget layer over one backend. The
/// Win32 vocabulary must not appear in it — if it does, a widget has grown a
/// platform dependency and the split stops meaning anything.
#[test]
fn ui_draw_layer_is_platform_neutral() {
    let src = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stdlib").join("ui_draw.ax"),
    )
    .expect("stdlib/ui_draw.ax must exist");
    // strip comments so prose about Win32 in the header does not count
    let code: String = src
        .lines()
        .map(|l| match l.find('#') {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n");
    for sym in [
        // Win32 / GDI
        "SelectObject", "GetStockObject", "SetDCBrushColor", "SetDCPenColor", "CreateWindowExW",
        "RegisterClassW", "GetAsyncKeyState", "GetClientRect", "BitBlt", "TextOutW",
        "CreateCompatibleDC", "CreateCompatibleBitmap", "GetTickCount64", "IntersectClipRect",
        "SaveDC", "RestoreDC", "MessageBoxW", "OpenClipboard",
    ] {
        assert!(
            !code.contains(sym),
            "ui_draw.ax must stay platform-neutral but references {sym}"
        );
    }
    // and it must not branch on the host OS either
    assert!(
        !code.contains("target_os()"),
        "ui_draw.ax must not branch on target_os() — that is the backend's job"
    );
}

/// Every primitive the widget layer CALLS must exist in BOTH backends — a
/// missing one only shows up as a link error at build time (v0.46.0: the
/// check covers the X11 backend too, so a new plat_* can no longer land
/// in ui_win.ax only).
#[test]
fn ui_draw_calls_only_implemented_primitives() {
    let draw = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stdlib").join("ui_draw.ax"),
    )
    .unwrap();
    let mut called: Vec<String> = Vec::new();
    for line in draw.lines() {
        let code = match line.find('#') {
            Some(i) => &line[..i],
            None => line,
        };
        let mut idx = 0;
        while let Some(p) = code[idx..].find("plat_") {
            let at = idx + p;
            let rest = &code[at..];
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            called.push(rest[..end].to_string());
            idx = at + end;
        }
    }
    called.sort();
    called.dedup();
    assert!(!called.is_empty(), "no plat_* calls found in ui_draw.ax");
    for backend in ["ui_win.ax", "ui_x11.ax"] {
        let src = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("stdlib")
                .join(backend),
        )
        .unwrap();
        for f in &called {
            assert!(
                src.contains(&format!("def {f}(")),
                "ui_draw.ax calls {f}() but {backend} does not define it"
            );
        }
    }
}
