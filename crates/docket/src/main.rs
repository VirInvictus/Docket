//! Docket: a plain-text task editor. One window per document, the
//! buffer is the file, autosave keeps disk always current, and an
//! external change reloads the view because the view never holds
//! unsaved state.

mod config;
mod fold;
mod sidebar;
mod sidecar;
mod styles;
mod welcome;

use gdk::Key;
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, ScrolledWindow};
use sourceview5::View;
use sourceview5::prelude::*;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const APP_ID: &str = "io.github.virinvictus.Docket";
/// The window's default width, shared with the divider math.
const DEFAULT_WIDTH: i32 = 760;
/// The sidebar column's fixed width; the editor takes the rest.
const SIDEBAR_WIDTH: i32 = 220;

/// Everything the window needs to keep between events.
struct Doc {
    window: ApplicationWindow,
    buffer: sourceview5::Buffer,
    view: View,
    path: RefCell<Option<PathBuf>>,
    /// Set while the buffer is being filled programmatically, so the
    /// changed handler neither autosaves nor races the monitor.
    loading: RefCell<bool>,
    monitor: RefCell<Option<gio::FileMonitor>>,
    folds: Rc<fold::FoldEngine>,
    renderer: fold::FoldGutterRenderer,
    /// The active filter (parsed query plus its source text) and the
    /// focus scope a sidebar project click installed.
    filter: Rc<RefCell<Option<(docket_core::query::Query, String)>>>,
    scope: Rc<RefCell<Option<usize>>>,
    search_entry: gtk4::SearchEntry,
    search_bar: gtk4::SearchBar,
    sidebar: sidebar::Sidebar,
}

thread_local! {
    /// One window per process for now (Phase 6 takes multiple).
    static DOC: RefCell<Option<Rc<Doc>>> = const { RefCell::new(None) };
}

fn main() -> glib::ExitCode {
    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE | gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    app.connect_activate(|app| activate(app, None));
    // Command-line documents arrive here through GApplication.
    app.connect_open(|app, files, _| {
        let path = files.first().and_then(|f| f.path());
        activate(app, path.as_deref());
    });
    app.run()
}

fn activate(app: &Application, initial: Option<&Path>) {
    use vir_gtk::portal;
    portal::init(None, None, true);
    vir_gtk::theme::install_app_stylesheet(&vir_gtk::theme::base_css(
        &vir_gtk::theme::Palette::dragon(),
    ));
    vir_gtk::theme::install_app_stylesheet(".docket-sidebar label { color: #c5c9c5; }\n");
    styles::install();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Docket")
        .default_width(DEFAULT_WIDTH)
        .default_height(980)
        .build();

    let buffer = match styles::language() {
        Some(lang) => sourceview5::Buffer::with_language(&lang),
        None => sourceview5::Buffer::default(),
    };
    if let Some(scheme) = styles::scheme() {
        buffer.set_style_scheme(Some(&scheme));
    }
    let view = View::with_buffer(&buffer);
    view.set_show_line_numbers(false);
    view.set_wrap_mode(gtk4::WrapMode::Word);
    view.set_top_margin(12);
    view.set_bottom_margin(24);
    view.set_left_margin(16);
    view.set_right_margin(24);

    let scrolled = ScrolledWindow::builder()
        .child(&view)
        .hexpand(true)
        .vexpand(true)
        .build();

    // Search bar above the editor: hidden while the filter is empty.
    let search_entry = gtk4::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search"));
    let search_bar = gtk4::SearchBar::new();
    search_bar.set_child(Some(&search_entry));
    search_bar.set_search_mode(false);
    search_bar.set_hexpand(true);

    let editor_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    editor_box.append(&search_bar);
    editor_box.append(&scrolled);

    // Sidebar on the left, editor on the right, in a pane.
    let sidebar = sidebar::Sidebar::new();
    let sidebar_scroll = ScrolledWindow::builder()
        .child(sidebar.list())
        .hexpand(false)
        .vexpand(true)
        .build();
    sidebar_scroll.set_width_request(SIDEBAR_WIDTH);
    let pane = gtk4::Paned::new(gtk4::Orientation::Horizontal);
    pane.set_start_child(Some(&editor_box));
    pane.set_end_child(Some(&sidebar_scroll));
    pane.set_shrink_start_child(false);
    // The construct-time position (the Atrium/Colophon pattern) only
    // lands when the final width matches default_width; under a
    // tiling WM the allocation is the WM's, not ours. So the divider
    // parks on the first idle after map, from the live allocation:
    // sidebar fixed at its width, editor taking the rest. Later
    // resizes keep the divider put, so the editor absorbs them.
    pane.connect_map(|pane| {
        glib::idle_add_local_once(glib::clone!(
            #[weak]
            pane,
            move || {
                let width = pane.width();
                if width > SIDEBAR_WIDTH {
                    pane.set_position(width - SIDEBAR_WIDTH);
                }
            }
        ));
    });
    window.set_child(Some(&pane));

    let folds = Rc::new(fold::FoldEngine::new(&buffer));
    let renderer = fold::FoldGutterRenderer::new();
    let gutter = sourceview5::prelude::ViewExt::gutter(&view, gtk4::TextWindowType::Left);
    gutter.insert(&renderer, 0);

    let filter: Rc<RefCell<Option<(docket_core::query::Query, String)>>> =
        Rc::new(RefCell::new(None));
    let scope: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));

    let doc = Rc::new(Doc {
        window: window.clone(),
        buffer,
        view,
        path: RefCell::new(None),
        loading: RefCell::new(true),
        monitor: RefCell::new(None),
        folds,
        renderer,
        filter,
        scope,
        search_entry: search_entry.clone(),
        search_bar: search_bar.clone(),
        sidebar: sidebar.clone(),
    });
    DOC.with(|slot| *slot.borrow_mut() = Some(doc.clone()));

    wire_autosave(&doc);
    wire_folds(&doc);
    wire_editing(&doc);
    wire_search(&doc, &search_entry, &search_bar);
    wire_sidebar(&doc, &sidebar);
    wire_actions(app, &window, &doc);

    match initial {
        Some(path) => load_file(&doc, path),
        None => load_welcome(&doc),
    }
    window.present();

    // DOCKET_DEBUG_LAYOUT=1: print the layout facts after allocation
    // and exit. Verification without pixel capture.
    if std::env::var_os("DOCKET_DEBUG_LAYOUT").is_some() {
        let d = doc.clone();
        let win = window.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(2500), move || {
            let pane = d
                .window
                .child()
                .and_downcast::<gtk4::Paned>()
                .expect("pane child");
            #[allow(deprecated)]
            let sidebar_w = pane.end_child().map(|w| w.allocated_width());
            #[allow(deprecated)]
            let editor_w = pane.start_child().map(|w| w.allocated_width());
            let rows = d.sidebar.row_count();
            let searchbar_visible = doc.search_bar.is_search_mode();
            let entries = doc.search_entry.text().to_string();
            eprintln!(
                "[layout] pane={:?} editor_w={editor_w:?} sidebar_w={sidebar_w:?} rows={rows} searchbar={searchbar_visible} entry={entries:?}",
                pane.position()
            );
            win.close();
            glib::ControlFlow::Break
        });
    }
}

/// Fold plumbing: on every buffer change, re-derive which lines are
/// foldable and which are folded, update the gutter, reapply the
/// invisible tag, and bounce the caret out of hidden ranges.
fn wire_folds(doc: &Rc<Doc>) {
    // Gutter click: toggle the fold on that header line.
    let engine = doc.folds.clone();
    let doc_for_click = doc.clone();
    doc.renderer.set_on_toggle(Box::new(move |line: u32| {
        let buffer = &doc_for_click.buffer;
        let Some(header) = buffer.iter_at_line(line as i32) else {
            return;
        };
        let Some(next) = buffer.iter_at_line(line as i32 + 1) else {
            return;
        };
        let line_text = buffer.text(&header, &next, true).to_string();
        let content = line_text.trim_end_matches(['\n', '\r']).to_string();
        let content = content.trim_start_matches('\t').to_string();
        let hash = fold::content_hash(&content);
        engine.toggle_hash(hash);
        let doc_hash_text = buffer_text(&doc_for_click).to_string();
        let key = fold::hash_hex(fold::content_hash(&doc_hash_text));
        sidecar::store(&key, &engine.folded_hashes());
        refresh_folds(&doc_for_click);
    }));

    doc.buffer.connect_changed(glib::clone!(
        #[strong]
        doc,
        move |_| {
            if !*doc.loading.borrow() {
                refresh_folds(&doc);
            }
        }
    ));
}

fn buffer_text(doc: &Rc<Doc>) -> glib::GString {
    doc.buffer
        .text(&doc.buffer.start_iter(), &doc.buffer.end_iter(), false)
}

/// Recompute foldable/folded line sets, reapply the invisible tag,
/// and keep the caret out of hidden text.
fn refresh_folds(doc: &Rc<Doc>) {
    let text = buffer_text(doc).to_string();
    let lines: Vec<&str> = text.split('\n').collect();
    let mut foldable = HashSet::new();
    let mut folded_lines = HashSet::new();
    for i in 0..lines.len().saturating_sub(1) {
        let depth = lines[i].chars().take_while(|&c| c == '\t').count();
        let content = lines[i].trim_start_matches('\t');
        if content.is_empty() {
            continue;
        }
        let next_depth = lines[i + 1].chars().take_while(|&c| c == '\t').count();
        if next_depth > depth {
            foldable.insert(i as u32);
            let hash = fold::content_hash(content);
            if doc.folds.is_folded_hash(hash) {
                folded_lines.insert(i as u32);
            }
        }
    }
    doc.renderer.set_lines(foldable, folded_lines);
    doc.folds.reapply(&doc.buffer);

    // Caret guard: if the insert mark landed in hidden text, move it
    // back to the start of its visible line.
    let mark = doc.buffer.get_insert();
    let mut iter = doc.buffer.iter_at_mark(&mark);
    if iter.has_tag(&doc.folds.tag) {
        let mut visible = iter;
        while visible.has_tag(&doc.folds.tag) && visible.backward_char() {
            if !visible.has_tag(&doc.folds.tag) {
                break;
            }
        }
        if !visible.has_tag(&doc.folds.tag) {
            iter = visible;
            doc.buffer.place_cursor(&iter);
        } else {
            let mut forward = doc.buffer.iter_at_mark(&mark);
            while forward.has_tag(&doc.folds.tag) && !forward.is_end() {
                forward.forward_char();
            }
            doc.buffer.place_cursor(&forward);
        }
    }
}

/// The editing keymap and click behaviors.
fn wire_editing(doc: &Rc<Doc>) {
    // Dash click toggles @done: a click gesture on the view checks
    // whether the press landed inside the first body character.
    let click = gtk4::GestureClick::new();
    let doc_for_click = doc.clone();
    click.set_button(1);
    click.connect_pressed(glib::clone!(
        #[strong]
        doc_for_click,
        move |gesture, _, x, y| {
            let (bx, by) = doc_for_click.view.window_to_buffer_coords(
                gtk4::TextWindowType::Text,
                x as i32,
                y as i32,
            );
            let Some(iter) = doc_for_click.view.iter_at_location(bx, by) else {
                return;
            };
            let line = iter.line();
            let Some(line_start) = doc_for_click.buffer.iter_at_line(line) else {
                return;
            };
            let mut line_end = line_start;
            line_end.forward_to_line_end();
            let line_text = doc_for_click
                .buffer
                .text(&line_start, &line_end, true)
                .to_string();
            let content_off = line_text.chars().take_while(|&c| c == '\t').count();
            let content: String = line_text
                .trim_start_matches('\t')
                .chars()
                .take_while(|&c| c != '\n')
                .collect();
            let is_task = content.starts_with("- ");
            // The dash glyph itself is the hit zone (the first body
            // character).
            let on_dash = is_task && iter.line_offset() == content_off as i32;
            if on_dash {
                let stamp = None; // the stamp preference arrives with settings
                if let Some(new_text) = docket_core::edit::toggle_done(
                    &buffer_text(&doc_for_click),
                    line as usize,
                    stamp,
                ) {
                    replace_document(&doc_for_click, new_text);
                }
                gesture.set_state(gtk4::EventSequenceState::Claimed);
            }
        }
    ));
    doc.view.add_controller(click);

    // Key editing: Enter, Tab, Shift-Tab, Backspace-at-column-zero,
    // the move cluster, duplicate, group, and done toggling.
    let key = gtk4::EventControllerKey::new();
    key.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let doc_for_key = doc.clone();
    key.connect_key_pressed(glib::clone!(
        #[strong]
        doc_for_key,
        move |_, keyval, _code, state| {
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            let alt = state.contains(gdk::ModifierType::ALT_MASK);
            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
            let text = buffer_text(&doc_for_key).to_string();
            let caret = doc_for_key.buffer.cursor_position().max(0) as usize;

            match keyval {
                Key::Return if !shift => {
                    let anchor = anchor_depth(&doc_for_key, caret);
                    if let Some(r) = docket_core::edit::smart_enter(&text, caret, true, anchor) {
                        apply_edit(&doc_for_key, &r.text, r.caret);
                        return glib::Propagation::Stop;
                    }
                }
                Key::Tab if !ctrl && !alt => {
                    let delta = if shift { -1 } else { 1 };
                    let (a, b) = selection_lines(&doc_for_key);
                    let floor = 0;
                    if let Some(new_text) =
                        docket_core::edit::shift_lines(&text, a, b, delta, floor)
                    {
                        replace_document(&doc_for_key, new_text);
                        return glib::Propagation::Stop;
                    }
                    return glib::Propagation::Stop; // Tab never inserts a tab
                }

                Key::BackSpace if caret_is_at_column_zero(&doc_for_key, caret) => {
                    let line = current_line(&doc_for_key, caret);
                    let depth = line_depth_of(&text, line);
                    let floor = 0;
                    if depth > floor
                        && let Some(new_text) =
                            docket_core::edit::shift_lines(&text, line, line, -1, floor)
                    {
                        apply_edit(&doc_for_key, &new_text, caret);
                        return glib::Propagation::Stop;
                    }
                }
                Key::d | Key::D if ctrl && !alt && !shift => {
                    let (a, b) = selection_lines(&doc_for_key);
                    let mut new_text = text.clone();
                    for line in (a..=b).rev() {
                        if let Some(t) = docket_core::edit::toggle_done(&new_text, line, None) {
                            new_text = t;
                        }
                    }
                    if new_text != text {
                        replace_document(&doc_for_key, new_text);
                    }
                    return glib::Propagation::Stop;
                }
                Key::Up | Key::Down if ctrl && alt => {
                    let down = keyval == Key::Down;
                    let branch = true; // branches carry their subtrees
                    if move_selection(&doc_for_key, &text, down, branch) {
                        return glib::Propagation::Stop;
                    }
                    return glib::Propagation::Proceed;
                }
                Key::d | Key::D if ctrl && shift => {
                    let (a, b) = selection_lines(&doc_for_key);
                    if let Some(new_text) = docket_core::edit::duplicate_block(&text, a, b) {
                        replace_document(&doc_for_key, new_text);
                    }
                    return glib::Propagation::Stop;
                }
                Key::g | Key::G if ctrl && alt => {
                    let (a, b) = selection_lines(&doc_for_key);
                    if let Some((new_text, _parent)) = docket_core::edit::group_lines(&text, a, b) {
                        replace_document(&doc_for_key, new_text);
                        return glib::Propagation::Stop;
                    }
                }
                _ => {}
            }
            glib::Propagation::Proceed
        }
    ));
    doc.view.add_controller(key);

    // Copy/cut must never leak folded (invisible) text: intercept
    // and serialize without hidden chars.
    doc.view.connect_copy_clipboard(glib::clone!(
        #[strong]
        doc,
        move |view| {
            let (start, end) = visible_selection(view, &doc.buffer);
            if start == end {
                return;
            }
            let text = doc.buffer.slice(&start, &end, false);
            doc.view.display().clipboard().set_text(&text);
        }
    ));
    doc.view.connect_cut_clipboard(glib::clone!(
        #[strong]
        doc,
        move |view| {
            let (start, end) = visible_selection(view, &doc.buffer);
            if start == end {
                return;
            }
            let text = doc.buffer.slice(&start, &end, false);
            doc.view.display().clipboard().set_text(&text);
            let mut cut_start = start;
            let mut cut_end = end;
            doc.buffer
                .delete_interactive(&mut cut_start, &mut cut_end, true);
        }
    ));

    // Paste: full re-parse and depth-shift to the caret's depth.
    doc.view.connect_paste_clipboard(glib::clone!(
        #[strong]
        doc,
        move |_| {
            doc.view.display().clipboard().read_text_async(
                gio::Cancellable::NONE,
                glib::clone!(
                    #[strong]
                    doc,
                    move |result| {
                        let Ok(Some(clip)) = result else { return };
                        let caret = doc.buffer.cursor_position() as usize;
                        let line = current_line(&doc, caret);
                        let depth = line_depth_of(&buffer_text(&doc), line);
                        let paste = docket_core::edit::prepare_paste(&clip, depth);
                        if paste.is_empty() {
                            return;
                        }
                        doc.buffer.insert_at_cursor(&format!("{paste}\n"));
                    }
                ),
            );
        }
    ));
}

/// The depth of the next visible line after the caret's line, per
/// the reference depth formula (None when the caret's line is last).
fn anchor_depth(doc: &Rc<Doc>, caret: usize) -> Option<i64> {
    let text = buffer_text(doc).to_string();
    let line = current_line(doc, caret);
    let next = line + 1;
    if next >= text.split('\n').count() {
        return None;
    }
    let depth_of = |l: usize| {
        text.split('\n')
            .nth(l)
            .map(|s| s.chars().take_while(|&c| c == '\t').count())
            .unwrap_or(0)
    };
    Some(depth_of(next) as i64)
}

/// The first and last line of the selection (a caret selects its own
/// line).
fn selection_lines(doc: &Rc<Doc>) -> (usize, usize) {
    let mut a = doc.buffer.iter_at_mark(&doc.buffer.get_insert()).line() as usize;
    let mut b = doc
        .buffer
        .iter_at_mark(&doc.buffer.selection_bound())
        .line() as usize;
    if b < a {
        std::mem::swap(&mut a, &mut b);
    }
    (a, b)
}

fn current_line(doc: &Rc<Doc>, caret: usize) -> usize {
    let text = buffer_text(doc);
    let mut count = 0;
    for (i, c) in text.chars().enumerate() {
        if i >= caret {
            break;
        }
        if c == '\n' {
            count += 1;
        }
    }
    count
}

fn line_depth_of(text: &str, line: usize) -> i64 {
    text.split('\n')
        .nth(line)
        .map(|s| s.chars().take_while(|&c| c == '\t').count() as i64)
        .unwrap_or(0)
}

fn caret_is_at_column_zero(doc: &Rc<Doc>, caret: usize) -> bool {
    let text = buffer_text(doc);
    caret == 0 || text.chars().nth(caret.saturating_sub(1)) == Some('\n')
}

/// Apply a whole-document edit: swap the buffer text inside one undo
/// group, keeping the caret clamped.
fn apply_edit(doc: &Rc<Doc>, new_text: &str, caret: usize) {
    doc.buffer.begin_user_action();
    *doc.loading.borrow_mut() = true;
    doc.buffer.set_text(new_text);
    *doc.loading.borrow_mut() = false;
    let clamped = caret.min(doc.buffer.char_count() as usize);
    doc.buffer
        .place_cursor(&doc.buffer.iter_at_offset(clamped as i32));
    doc.buffer.end_user_action();
}

/// Same, caret recomputed later by the fold refresh.
fn replace_document(doc: &Rc<Doc>, new_text: String) {
    doc.buffer.begin_user_action();
    *doc.loading.borrow_mut() = true;
    doc.buffer.set_text(&new_text);
    *doc.loading.borrow_mut() = false;
    doc.buffer.end_user_action();
    refresh_folds(doc);
}

/// Move the selected block up/down. Branch moves carry whole
/// subtrees; returns false when the move is a no-op.
fn move_selection(doc: &Rc<Doc>, text: &str, down: bool, branch: bool) -> bool {
    let (mut a, mut b) = selection_lines(doc);
    if branch {
        a = docket_core::edit::branch_start(text, a);
        b = docket_core::edit::subtree_end(text, b);
    }
    let line_count = text.split('\n').count();
    if down {
        // The neighbor block starts after our subtree ends; the move
        // lands after the neighbor's subtree.
        let neighbor = docket_core::edit::subtree_end(text, b) + 1;
        if neighbor >= line_count {
            return false;
        }
        let neighbor_end = docket_core::edit::subtree_end(text, neighbor);
        let Some(new_text) = docket_core::edit::move_block(text, a, b, neighbor_end + 1) else {
            return false;
        };
        replace_document(doc, new_text);
        return true;
    }
    // Up: the neighbor is the line before our branch start; we land
    // before that neighbor's branch.
    if a == 0 {
        return false;
    }
    let neighbor_start = docket_core::edit::branch_start(text, a - 1);
    let Some(new_text) = docket_core::edit::move_block(text, a, b, neighbor_start) else {
        return false;
    };
    replace_document(doc, new_text);
    true
}

/// The selection as (start, end) iters, or caret..caret.
fn visible_selection(
    view: &View,
    buffer: &sourceview5::Buffer,
) -> (gtk4::TextIter, gtk4::TextIter) {
    let mut start = buffer.iter_at_mark(&buffer.selection_bound());
    let mut end = buffer.iter_at_mark(&buffer.get_insert());
    if start.offset() > end.offset() {
        std::mem::swap(&mut start, &mut end);
    }
    let _ = view;
    (start, end)
}

/// Fill the buffer from a file, watch it, and remember it in
/// recents. The file on disk wins; the buffer is just the view.
/// Fold state restores from the sidecar keyed by the content hash.
fn load_file(doc: &Rc<Doc>, path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("docket: cannot read {}", path.display());
        return;
    };
    *doc.loading.borrow_mut() = true;
    doc.buffer.set_text(&text);
    *doc.loading.borrow_mut() = false;
    *doc.path.borrow_mut() = Some(path.to_path_buf());
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Docket".into());
    doc.window.set_title(Some(&format!("{name} - Docket")));
    watch(doc, path);
    let file = gio::File::for_path(path);
    gtk4::RecentManager::default().add_item(&file.uri());
    let key = fold::hash_hex(fold::content_hash(&text));
    doc.folds.restore(&sidecar::load(&key));
    refresh_folds(doc);
    rebuild_sidebar(doc);
}

/// The first-run document: an untitled buffer with the welcome text.
/// It autosaves nowhere until the user names it.
fn load_welcome(doc: &Rc<Doc>) {
    *doc.loading.borrow_mut() = true;
    doc.buffer.set_text(welcome::WELCOME);
    *doc.loading.borrow_mut() = false;
    *doc.path.borrow_mut() = None;
    *doc.monitor.borrow_mut() = None;
    doc.window.set_title(Some("Welcome - Docket"));
    rebuild_sidebar(doc);
}

/// Autosave: every buffer change writes the file, so the disk is
/// always the truth and there is no dirty state to lose.
fn wire_autosave(doc: &Rc<Doc>) {
    doc.buffer.connect_changed(glib::clone!(
        #[strong]
        doc,
        move |_| {
            if *doc.loading.borrow() {
                return;
            }
            if let Some(path) = doc.path.borrow().as_ref() {
                let text = doc
                    .buffer
                    .text(&doc.buffer.start_iter(), &doc.buffer.end_iter(), false);
                *doc.loading.borrow_mut() = true;
                let _ = std::fs::write(path, text.as_str());
                *doc.loading.borrow_mut() = false;
            }
        }
    ));
}

/// Watch the file for external changes. The buffer never holds
/// unsaved state, so an external change always reloads; the cursor
/// and scroll position stay where they were, clamped.
fn watch(doc: &Rc<Doc>, path: &Path) {
    let file = gio::File::for_path(path);
    let monitor = file
        .monitor_file(gio::FileMonitorFlags::empty(), gio::Cancellable::NONE)
        .ok();
    if let Some(monitor) = &monitor {
        let file_for_signal = file.clone();
        monitor.connect_changed(glib::clone!(
            #[strong]
            doc,
            #[strong]
            file_for_signal,
            move |_, _, _, _| {
                if *doc.loading.borrow() {
                    return;
                }
                let Some(path) = file_for_signal.path() else {
                    return;
                };
                let Ok(text) = std::fs::read_to_string(path) else {
                    return;
                };
                let current =
                    doc.buffer
                        .text(&doc.buffer.start_iter(), &doc.buffer.end_iter(), false);
                if current.as_str() == text {
                    return;
                }
                let mut cursor = doc.buffer.cursor_position();
                let mut upper = doc
                    .buffer
                    .iter_at_mark(&doc.buffer.selection_bound())
                    .offset();
                *doc.loading.borrow_mut() = true;
                doc.buffer.set_text(&text);
                *doc.loading.borrow_mut() = false;
                let len = doc.buffer.char_count();
                cursor = cursor.min(len);
                upper = upper.min(len);
                doc.buffer.select_range(
                    &doc.buffer.iter_at_offset(cursor),
                    &doc.buffer.iter_at_offset(upper),
                );
            }
        ));
    }
    *doc.monitor.borrow_mut() = monitor;
}

fn wire_actions(app: &Application, window: &ApplicationWindow, doc: &Rc<Doc>) {
    let open = gio::SimpleAction::new("open", None);
    open.connect_activate(glib::clone!(
        #[strong]
        window,
        #[strong]
        doc,
        move |_, _| {
            let dialog = gtk4::FileDialog::new();
            let filter = gtk4::FileFilter::new();
            filter.set_name(Some("TaskPaper lists"));
            filter.add_pattern("*.taskpaper");
            filter.add_mime_type("text/plain");
            dialog.set_default_filter(Some(&filter));
            let win = window.clone();
            dialog.open(
                Some(&win),
                gio::Cancellable::NONE,
                glib::clone!(
                    #[strong]
                    doc,
                    move |result| {
                        if let Ok(file) = result
                            && let Some(path) = file.path()
                        {
                            load_file(&doc, &path);
                        }
                    }
                ),
            );
        }
    ));
    app.add_action(&open);
    app.set_accels_for_action("app.open", &["<Control>o"]);

    let save_as = gio::SimpleAction::new("save-as", None);
    save_as.connect_activate(glib::clone!(
        #[strong]
        window,
        #[strong]
        doc,
        move |_, _| {
            let dialog = gtk4::FileDialog::new();
            dialog.set_title("Save list as");
            let win = window.clone();
            dialog.save(
                Some(&win),
                gio::Cancellable::NONE,
                glib::clone!(
                    #[strong]
                    doc,
                    move |result| {
                        if let Ok(file) = result
                            && let Some(path) = file.path()
                        {
                            let text = doc.buffer.text(
                                &doc.buffer.start_iter(),
                                &doc.buffer.end_iter(),
                                false,
                            );
                            if std::fs::write(&path, text.as_str()).is_ok() {
                                load_file(&doc, &path);
                            }
                        }
                    }
                ),
            );
        }
    ));
    app.add_action(&save_as);
    app.set_accels_for_action("app.save-as", &["<Control><Shift>s"]);

    let quit = gio::SimpleAction::new("quit", None);
    quit.connect_activate(glib::clone!(
        #[strong]
        app,
        move |_, _| app.quit()
    ));
    app.add_action(&quit);
    app.set_accels_for_action("app.quit", &["<Control>q"]);

    // Search opens on Ctrl+F (the reference cluster's Cmd+F); the
    // sidebar's query rows reveal it too.
    let start_search = gio::SimpleAction::new("start-search", None);
    start_search.connect_activate(glib::clone!(
        #[strong]
        doc,
        move |_, _| {
            doc.search_bar.set_search_mode(true);
            doc.search_entry.grab_focus();
        }
    ));
    app.add_action(&start_search);
    app.set_accels_for_action("app.start-search", &["<Control>f"]);
}

/// The searchbar: live filtering with parse errors surfaced, Escape
/// clearing, and the sidebar rebuilt alongside (the sidebar reflects
/// whatever the searchbar shows).
fn wire_search(doc: &Rc<Doc>, entry: &gtk4::SearchEntry, bar: &gtk4::SearchBar) {
    // The text completion last acted on: backspacing into a
    // completed tag is a deletion and must not resurrect it.
    let last_text: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    entry.connect_search_changed(glib::clone!(
        #[strong]
        doc,
        #[strong]
        last_text,
        move |entry: &gtk4::SearchEntry| {
            let text = entry.text().to_string();
            let prev = last_text.replace(text.clone());
            let deleting = prev.starts_with(&text) && prev.len() > text.len();
            let trimmed = text.trim().to_string();
            if trimmed.is_empty() {
                *doc.filter.borrow_mut() = None;
                entry.remove_css_class("error");
                refresh_folds(&doc);
                rebuild_sidebar(&doc);
                return;
            }
            // Inline tag completion: extend a trailing partial @tag
            // from the document census. The suggested suffix stays
            // selected, so the next keystroke replaces it; the
            // set_text re-enters this handler with the completed
            // text, which parses and filters there.
            if !deleting && let Some((at, name)) = tag_completion(&doc, &text) {
                let mut completed: String = text.chars().take(at).collect();
                completed.push('@');
                completed.push_str(&name);
                let typed = text.chars().skip(at + 1).count();
                entry.set_text(&completed);
                entry.select_region(
                    (at + 1 + typed) as i32,
                    (at + 1 + name.chars().count()) as i32,
                );
                return;
            }
            match docket_core::query::Query::parse(&trimmed) {
                Ok(query) => {
                    entry.remove_css_class("error");
                    *doc.filter.borrow_mut() = Some((query, trimmed));
                    apply_filter(&doc);
                }
                Err(_) => {
                    entry.add_css_class("error");
                    *doc.filter.borrow_mut() = None;
                    refresh_folds(&doc);
                }
            }
        }
    ));

    entry.connect_stop_search(glib::clone!(
        #[strong]
        doc,
        #[strong]
        entry,
        #[strong]
        bar,
        move |_| {
            entry.set_text("");
            *doc.filter.borrow_mut() = None;
            bar.set_search_mode(false);
            refresh_folds(&doc);
            rebuild_sidebar(&doc);
        }
    ));
}

/// The completion proposal for the searchbar's current text: tag
/// names from the document census, minus the excluded ones.
fn tag_completion(doc: &Rc<Doc>, text: &str) -> Option<(usize, String)> {
    let doc_text = buffer_text(doc).to_string();
    let (_, exclude) = config::tag_lists();
    let names: Vec<String> = docket_core::census::tag_census(&doc_text, &exclude)
        .into_keys()
        .collect();
    docket_core::census::search_tag_completion(text, &names)
}

/// Apply the active filter: matches plus their ancestor chains stay
/// visible, everything else hides. The filter bypasses folds, per
/// the reference.
fn apply_filter(doc: &Rc<Doc>) {
    let Some((query, _)) = doc.filter.borrow().as_ref().map(|f| f.clone()) else {
        return;
    };
    let text = buffer_text(doc).to_string();
    let line_count = text.split('\n').count();
    let doc_for_run = docket_core::Document::load(&text);
    let matches = query.run_scoped_at(&doc_for_run, None, docket_core::dates::now());
    let mut visible: HashSet<usize> = matches.iter().copied().collect();
    let mut dimmed: HashSet<u32> = HashSet::new();
    for &m in &matches {
        let mut cur = m;
        while let Some(p) = doc_for_run.parent_of(cur) {
            visible.insert(p);
            dimmed.insert(p as u32);
            cur = p;
        }
    }

    // Hide every non-visible line (its newline included).
    doc.buffer.remove_tag(
        &doc.folds.tag,
        &doc.buffer.start_iter(),
        &doc.buffer.end_iter(),
    );
    let mut i = 0;
    while i < line_count {
        if visible.contains(&i) {
            i += 1;
            continue;
        }
        let Some(start) = doc.buffer.iter_at_line(i as i32) else {
            break;
        };
        let mut end = start;
        while i < line_count && !visible.contains(&i) {
            if let Some(next) = doc.buffer.iter_at_line(i as i32) {
                end = next;
                end.forward_to_line_end();
                end.forward_char();
            }
            i += 1;
        }
        doc.buffer.apply_tag(&doc.folds.tag, &start, &end);
    }

    // Gutter: dimmed markers on ancestors, nothing elsewhere.
    let mut foldable: HashSet<u32> = HashSet::new();
    let mut folded: HashSet<u32> = HashSet::new();
    for (i, line) in text.split('\n').enumerate() {
        let content = line.trim_start_matches('\t');
        if content.is_empty() || i + 1 >= line_count {
            continue;
        }
        let depth = line.chars().take_while(|&c| c == '\t').count();
        let next_depth = text
            .split('\n')
            .nth(i + 1)
            .map(|l| l.chars().take_while(|&c| c == '\t').count())
            .unwrap_or(0);
        if next_depth > depth {
            foldable.insert(i as u32);
            if dimmed.contains(&(i as u32)) {
                folded.insert(i as u32);
            }
        }
    }
    doc.renderer.set_lines(foldable, folded);
    guard_caret(doc);
}

/// Bump the caret out of any invisible range it landed in.
fn guard_caret(doc: &Rc<Doc>) {
    let mark = doc.buffer.get_insert();
    let iter = doc.buffer.iter_at_mark(&mark);
    if !iter.has_tag(&doc.folds.tag) {
        return;
    }
    // Walk back to the last visible char; if none, walk forward.
    let mut back = iter;
    while back.has_tag(&doc.folds.tag) {
        if back.is_start() {
            break;
        }
        back.backward_char();
    }
    if !back.has_tag(&doc.folds.tag) && back.offset() != iter.offset() {
        doc.buffer.place_cursor(&back);
        return;
    }
    let mut forward = iter;
    while forward.has_tag(&doc.folds.tag) && !forward.is_end() {
        forward.forward_char();
    }
    doc.buffer.place_cursor(&forward);
}

/// The sidebar model from the current document and config files.
fn rebuild_sidebar(doc: &Rc<Doc>) {
    let text = buffer_text(doc).to_string();
    let (include, exclude) = config::tag_lists();
    let searches = config::searches();
    let model = sidebar::build_model(&text, &searches, &include, &exclude);
    doc.sidebar.rebuild(&model);
}

fn wire_sidebar(doc: &Rc<Doc>, sidebar: &sidebar::Sidebar) {
    let doc_cb = doc.clone();
    sidebar.set_on_activate(Rc::new(move |kind| {
        let entry = doc_cb.search_entry.clone();
        let bar = doc_cb.search_bar.clone();
        match kind {
            sidebar::RowKind::Home => {
                *doc_cb.scope.borrow_mut() = None;
                *doc_cb.filter.borrow_mut() = None;
                entry.set_text("");
                bar.set_search_mode(false);
                refresh_folds(&doc_cb);
            }
            sidebar::RowKind::Project { line } => {
                *doc_cb.scope.borrow_mut() = Some(*line);
                refresh_folds(&doc_cb);
            }
            sidebar::RowKind::Search { query } => {
                bar.set_search_mode(true);
                entry.set_text(query);
            }
            sidebar::RowKind::Tag { name } => {
                // The reference reveals the field with the active
                // query whenever a filter exists, but never steals
                // focus from the editor.
                bar.set_search_mode(true);
                entry.set_text(&format!("@{name}"));
            }
            sidebar::RowKind::TagValue { name, value } => {
                bar.set_search_mode(true);
                entry.set_text(&format!("@{name} contains[l] \"{value}\""));
            }
            sidebar::RowKind::Group(_) => {}
        }
    }));
    rebuild_sidebar(doc);
}
