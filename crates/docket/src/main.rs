//! Docket: a plain-text task editor. One window per document, the
//! buffer is the file, autosave keeps disk always current, and an
//! external change reloads the view because the view never holds
//! unsaved state.

mod completion;
mod config;
mod fold;
mod sidebar;
mod sidecar;
mod styles;
mod templates;
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
    /// focused project, stored as the project line's content hash so
    /// edits above it do not strand the scope.
    filter: Rc<RefCell<Option<(docket_core::query::Query, String)>>>,
    focus: Rc<RefCell<Option<u64>>>,
    search_entry: gtk4::SearchEntry,
    search_bar: gtk4::SearchBar,
    sidebar: sidebar::Sidebar,
}

thread_local! {
    /// One window per process for now (Phase 6 takes multiple).
    static DOC: RefCell<Option<Rc<Doc>>> = const { RefCell::new(None) };
}

fn main() -> glib::ExitCode {
    // `docket --template <name>` starts an untitled window from a
    // shipped template (the New-from-template menu spawns it).
    let template = std::env::args()
        .nth(1)
        .filter(|a| a == "--template")
        .and_then(|_| std::env::args().nth(2))
        .or_else(|| {
            std::env::args()
                .nth(1)
                .and_then(|a| a.strip_prefix("--template=").map(|id| id.to_string()))
        });

    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE | gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    app.connect_activate(move |app| activate(app, None, template.as_deref()));
    // Command-line documents arrive here through GApplication.
    app.connect_open(|app, files, _| {
        let path = files.first().and_then(|f| f.path());
        activate(app, path.as_deref(), None);
    });
    app.run()
}

fn activate(app: &Application, initial: Option<&Path>, template: Option<&str>) {
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
    let focus: Rc<RefCell<Option<u64>>> = Rc::new(RefCell::new(None));

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
        focus,
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

    // The headerbar: title and the command menu (the Quire shape).
    let header = gtk4::HeaderBar::new();
    let title = gtk4::Label::new(Some("Docket"));
    header.set_title_widget(Some(&title));
    header.set_show_title_buttons(true);
    let menu_button = gtk4::MenuButton::new();
    menu_button.set_icon_name("open-menu-symbolic");
    menu_button.set_tooltip_text(Some("Menu"));
    menu_button.set_menu_model(Some(&build_menu()));
    header.pack_end(&menu_button);
    window.set_titlebar(Some(&header));

    // Tag completion in the editor: proposals come live from the
    // document census, so edits never leave a stale list.
    {
        let doc_for_tags = Rc::downgrade(&doc);
        completion::install(
            &doc.view,
            Rc::new(move || {
                let Some(doc) = doc_for_tags.upgrade() else {
                    return Vec::new();
                };
                let text = buffer_text(&doc).to_string();
                let (_, exclude) = config::tag_lists();
                docket_core::census::tag_census(&text, &exclude)
                    .into_keys()
                    .collect()
            }),
        );
    }

    match initial {
        Some(path) => load_file(&doc, path),
        None => match template.and_then(template_by_id) {
            Some(text) => load_untitled(&doc, &text, template.unwrap_or("Template")),
            None => load_welcome(&doc),
        },
    }
    window.present();
    // The view holds the keyboard from the start: without this the
    // key controller and app accels stay inert until the first
    // click, even though typed characters still reach the buffer
    // through the input method.
    doc.view.grab_focus();

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

/// The document's full text, folded content included. The invisible
/// tag is presentation: the engine and the file always see the
/// canonical text. Only copy/cut deliberately strip it.
fn buffer_text(doc: &Rc<Doc>) -> glib::GString {
    doc.buffer
        .text(&doc.buffer.start_iter(), &doc.buffer.end_iter(), true)
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
    doc.renderer
        .set_lines(foldable, folded_lines, HashSet::new());
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
                let stamp = done_stamp();
                if let Some(new_text) = docket_core::edit::toggle_done(
                    &buffer_text(&doc_for_click),
                    line as usize,
                    stamp.as_deref(),
                ) {
                    replace_document(&doc_for_click, new_text);
                }
                gesture.set_state(gtk4::EventSequenceState::Claimed);
            } else if let Some(query) =
                tag_query_at(&content, iter.line_offset() as usize, content_off)
            {
                // A click on a @tag or its value filters by it, the
                // reference's text-click behavior: the field reveals
                // with the query but keeps the editor's focus.
                doc_for_click.search_bar.set_search_mode(true);
                doc_for_click.search_entry.set_text(&query);
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
        move |_, keyval, code, state| {
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
                    let stamp = done_stamp();
                    let mut new_text = text.clone();
                    for line in (a..=b).rev() {
                        if let Some(t) =
                            docket_core::edit::toggle_done(&new_text, line, stamp.as_deref())
                        {
                            new_text = t;
                        }
                    }
                    if new_text != text {
                        replace_document(&doc_for_key, new_text);
                    }
                    return glib::Propagation::Stop;
                }
                Key::Up | Key::Down if ctrl && alt => {
                    // Move passes the previous/next row at the
                    // selection's level; Move Branch (Shift) carries
                    // whole subtrees, the reference cluster's split.
                    let down = keyval == Key::Down;
                    if shift {
                        if move_selection(&doc_for_key, &text, down, true) {
                            return glib::Propagation::Stop;
                        }
                    } else {
                        let (a, b) = selection_lines(&doc_for_key);
                        if let Some(new_text) = docket_core::edit::move_items(&text, a, b, down) {
                            replace_document(&doc_for_key, new_text);
                            return glib::Propagation::Stop;
                        }
                    }
                    return glib::Propagation::Proceed;
                }
                Key::d | Key::D if ctrl && shift => {
                    cmd_duplicate(&doc_for_key);
                    return glib::Propagation::Stop;
                }
                Key::g | Key::G if ctrl && alt => {
                    cmd_group(&doc_for_key);
                    return glib::Propagation::Stop;
                }
                Key::Right if ctrl && alt && !shift => {
                    // Focus In: the caret's nearest enclosing project.
                    let caret_line = current_line(&doc_for_key, caret);
                    if let Some(p) = docket_core::fold::enclosing_project(&text, caret_line) {
                        *doc_for_key.focus.borrow_mut() =
                            Some(fold::content_hash(line_content(&text, p)));
                        apply_focus(&doc_for_key);
                    }
                    return glib::Propagation::Stop;
                }
                Key::Left if ctrl && alt && !shift => {
                    // Focus Out: pop to the parent project, or home.
                    let scoped = focused_line(&doc_for_key);
                    let parent = scoped.and_then(|l| docket_core::fold::parent_project(&text, l));
                    *doc_for_key.focus.borrow_mut() =
                        parent.map(|p| fold::content_hash(line_content(&text, p)));
                    apply_focus(&doc_for_key);
                    return glib::Propagation::Stop;
                }
                Key::h | Key::H if ctrl && shift && !alt => {
                    // Go Home: drop the focus, keep any search.
                    cmd_go_home(&doc_for_key);
                    return glib::Propagation::Stop;
                }
                // The fold cluster's digit chords match the hardware
                // keycode (GDK is xkb-based: evdev + 8, so the 0 key
                // is 19 and the 9 key is 18): with Shift held the
                // keyval turns into punctuation (Ctrl+Shift+0 types
                // ')'), which would never match a keyval arm.
                _ if ctrl && !alt && !shift && code == 19 => {
                    cmd_fold(&doc_for_key, FoldCmd::ExpandItems);
                    return glib::Propagation::Stop;
                }
                _ if ctrl && !alt && !shift && code == 18 => {
                    cmd_fold(&doc_for_key, FoldCmd::CollapseItems);
                    return glib::Propagation::Stop;
                }
                _ if ctrl && alt && !shift && code == 19 => {
                    cmd_fold(&doc_for_key, FoldCmd::ExpandCompletely);
                    return glib::Propagation::Stop;
                }
                _ if ctrl && alt && !shift && code == 18 => {
                    cmd_fold(&doc_for_key, FoldCmd::CollapseCompletely);
                    return glib::Propagation::Stop;
                }
                _ if ctrl && shift && !alt && code == 19 => {
                    cmd_fold(&doc_for_key, FoldCmd::ExpandLevel);
                    return glib::Propagation::Stop;
                }
                _ if ctrl && shift && !alt && code == 18 => {
                    cmd_fold(&doc_for_key, FoldCmd::CollapseLevel);
                    return glib::Propagation::Stop;
                }
                _ if ctrl && alt && shift && code == 19 => {
                    cmd_fold(&doc_for_key, FoldCmd::ExpandAllCompletely);
                    return glib::Propagation::Stop;
                }
                _ if ctrl && alt && shift && code == 18 => {
                    cmd_fold(&doc_for_key, FoldCmd::CollapseAllCompletely);
                    return glib::Propagation::Stop;
                }
                Key::a | Key::A if ctrl && shift && !alt => {
                    cmd_archive(&doc_for_key);
                    return glib::Propagation::Stop;
                }
                _ => {}
            }
            glib::Propagation::Proceed
        }
    ));
    doc.view.add_controller(key);

    // Copy/cut must never leak folded (invisible) text: intercept,
    // serialize without hidden chars, and stop the emission so the
    // default handler never overwrites the clipboard with the raw
    // selection.
    doc.view.connect_copy_clipboard(glib::clone!(
        #[strong]
        doc,
        move |view| {
            view.stop_signal_emission_by_name("copy-clipboard");
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
            view.stop_signal_emission_by_name("cut-clipboard");
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

    // Paste: full re-parse and depth-shift to the caret's depth. The
    // emission stops so the default raw paste never runs alongside
    // the prepared insert.
    doc.view.connect_paste_clipboard(glib::clone!(
        #[strong]
        doc,
        move |view| {
            view.stop_signal_emission_by_name("paste-clipboard");
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
                        // The separator newline joins the paste to
                        // the next line; at the document end it
                        // would become a trailing newline, which the
                        // format forbids.
                        let at_end = caret >= doc.buffer.char_count() as usize;
                        if at_end {
                            doc.buffer.insert_at_cursor(&paste);
                        } else {
                            doc.buffer.insert_at_cursor(&format!("{paste}\n"));
                        }
                    }
                ),
            );
        }
    ));
}

/// The filter query for a click at char offset `off` within a line
/// whose content starts at `content_off`: the tag's name when the
/// click lands on `@name`, or the tag plus value when it lands on
/// the parenthesized value. None off-tag.
fn tag_query_at(content: &str, off: usize, content_off: usize) -> Option<String> {
    if off < content_off {
        return None;
    }
    let rel = off - content_off;
    let byte = content.char_indices().nth(rel).map(|(i, _)| i)?;
    for t in docket_core::tags::scan(content) {
        if byte < t.start || byte > t.end {
            continue;
        }
        let name_end = t.start + 1 + t.name.len();
        let bare = format!("@{}", t.name);
        if byte <= name_end {
            return Some(bare);
        }
        return match t.value {
            Some(v) if !v.is_empty() => Some(format!("{bare} contains[l] \"{v}\"")),
            _ => Some(bare),
        };
    }
    None
}

/// Apply a fold command: fold the first list's header lines and
/// unfold the second's (the By Level commands set both sides),
/// persist to the sidecar, refresh.
fn apply_fold_command(doc: &Rc<Doc>, fold: &[usize], unfold: &[usize]) {
    let text = buffer_text(doc).to_string();
    let hashes = |ls: &[usize]| {
        ls.iter()
            .map(|&l| fold::content_hash(line_content(&text, l)))
            .collect::<Vec<u64>>()
    };
    doc.folds.fold_lines(&hashes(fold));
    doc.folds.unfold_lines(&hashes(unfold));
    let key = fold::hash_hex(fold::content_hash(&text));
    sidecar::store(&key, &doc.folds.folded_hashes());
    refresh_folds(doc);
}

/// The fold keyboard cluster, mirroring the reference's Outline
/// menu: the four Items commands act on the branches owning the
/// selection (topmost selected items with children), with the
/// leaf fallback to the nearest foldable ancestor; the Completely
/// variants extend into whole subtrees; the All commands and the
/// By Level pair are document-scoped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FoldCmd {
    ExpandItems,
    ExpandCompletely,
    CollapseItems,
    CollapseCompletely,
    ExpandLevel,
    CollapseLevel,
    ExpandAllCompletely,
    CollapseAllCompletely,
}

/// The topmost selected lines: a walk that skips selected subtrees.
fn topmost_selected(text: &str, a: usize, b: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = a;
    while i <= b {
        out.push(i);
        i = docket_core::edit::subtree_end(text, i) + 1;
    }
    out
}

/// The nearest foldable ancestor above `line` (an item with
/// children whose subtree covers it).
fn nearest_foldable_ancestor(text: &str, line: usize) -> Option<usize> {
    let headers = docket_core::fold::header_lines(text);
    (0..line).rev().find(|&k| {
        docket_core::edit::subtree_end(text, k) >= line && headers.iter().any(|&(hl, _)| hl == k)
    })
}

fn cmd_fold(doc: &Rc<Doc>, cmd: FoldCmd) {
    let text = buffer_text(doc).to_string();
    let headers = docket_core::fold::header_lines(&text);
    let is_foldable = |l: usize| headers.iter().any(|&(hl, _)| hl == l);

    // (fold these, unfold these, park the caret at fold[0]) — the
    // park happens only on the leaf fallback of a Collapse, where
    // the reference relocates the caret to the collapsed ancestor.
    let (fold_set, unfold_set, park) = match cmd {
        FoldCmd::ExpandItems | FoldCmd::CollapseItems => {
            let (a, b) = selection_lines(doc);
            let owners: Vec<usize> = topmost_selected(&text, a, b)
                .into_iter()
                .filter(|&l| is_foldable(l))
                .collect();
            let (collapse, park) = match owners.first() {
                Some(_) => (true, false),
                None => match nearest_foldable_ancestor(&text, a) {
                    Some(_) => (true, true),
                    None => return,
                },
            };
            let target = owners
                .first()
                .copied()
                .or_else(|| nearest_foldable_ancestor(&text, a));
            match target {
                Some(t) if collapse => (vec![t], vec![], park),
                Some(t) => (vec![], vec![t], false),
                None => return,
            }
        }
        FoldCmd::ExpandCompletely | FoldCmd::CollapseCompletely => {
            let (a, b) = selection_lines(doc);
            let owners: Vec<usize> = topmost_selected(&text, a, b)
                .into_iter()
                .filter(|&l| is_foldable(l))
                .collect();
            let targets = match owners.first() {
                Some(&first) => docket_core::fold::subtree_header_lines(&text, first),
                None => match nearest_foldable_ancestor(&text, a) {
                    Some(a_line) => docket_core::fold::subtree_header_lines(&text, a_line),
                    None => return,
                },
            };
            match cmd {
                FoldCmd::CollapseCompletely => (targets, vec![], false),
                _ => (vec![], targets, false),
            }
        }
        FoldCmd::ExpandLevel | FoldCmd::CollapseLevel => {
            // One global level, derived document-wide: the deepest
            // currently folded header, or the deepest header when
            // nothing is folded. Expand reveals one more level,
            // Collapse one fewer; headers at or beyond the level
            // fold.
            let mut level = headers.iter().map(|&(_, d)| d).max().unwrap_or(0);
            for hash in doc.folds.folded_hashes() {
                if let Some(l) = fold_hash_line(&text, hash)
                    && let Some(&(_, d)) = headers.iter().find(|&&(hl, _)| hl == l)
                {
                    level = level.max(d);
                }
            }
            let target = match cmd {
                FoldCmd::ExpandLevel => level + 1,
                _ => level.saturating_sub(1),
            };
            let at_or_beyond: Vec<usize> = headers
                .iter()
                .filter(|&&(_, d)| d >= target)
                .map(|&(l, _)| l)
                .collect();
            let below: Vec<usize> = headers
                .iter()
                .filter(|&&(_, d)| d < target)
                .map(|&(l, _)| l)
                .collect();
            // Both directions share the polarity: headers at or
            // beyond the target fold, headers below it unfold. Only
            // the target differs (level + 1 vs level - 1).
            (at_or_beyond, below, false)
        }
        FoldCmd::ExpandAllCompletely | FoldCmd::CollapseAllCompletely => {
            let all = docket_core::fold::all_header_lines(&text);
            match cmd {
                FoldCmd::CollapseAllCompletely => (all, vec![], false),
                _ => (vec![], all, false),
            }
        }
    };
    apply_fold_command(doc, &fold_set, &unfold_set);

    if park && let Some(iter) = doc.buffer.iter_at_line(fold_set[0] as i32) {
        let mut end = iter;
        end.forward_to_line_end();
        doc.buffer.place_cursor(&end);
        doc.view.scroll_to_iter(&mut end, 0.0, true, 0.0, 0.0);
    }
}

/// Resolve a fold hash back to its current line.
fn fold_hash_line(text: &str, hash: u64) -> Option<usize> {
    text.split('\n')
        .enumerate()
        .find(|(_, line)| fold::content_hash(line.trim_start_matches('\t')) == hash)
        .map(|(i, _)| i)
}

/// The template text for a menu id, case-insensitive.
fn template_by_id(id: &str) -> Option<String> {
    templates::TEMPLATES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(id))
        .map(|(_, text)| (*text).to_string())
}

fn cmd_archive(doc: &Rc<Doc>) {
    let text = buffer_text(doc).to_string();
    if let Some((new_text, _caret)) = docket_core::edit::archive_done(&text) {
        replace_document(doc, new_text);
    }
}

fn cmd_duplicate(doc: &Rc<Doc>) {
    let text = buffer_text(doc).to_string();
    let (a, b) = selection_lines(doc);
    if let Some(new_text) = docket_core::edit::duplicate_block(&text, a, b) {
        replace_document(doc, new_text);
    }
}

fn cmd_group(doc: &Rc<Doc>) {
    let text = buffer_text(doc).to_string();
    let (a, b) = selection_lines(doc);
    if let Some((new_text, _parent)) = docket_core::edit::group_lines(&text, a, b) {
        replace_document(doc, new_text);
    }
}

fn cmd_delete_items(doc: &Rc<Doc>) {
    let text = buffer_text(doc).to_string();
    let (a, mut b) = selection_lines(doc);
    // A collapsed item's hidden descendants go with it.
    if let Some(content) = text.split('\n').nth(b)
        && doc
            .folds
            .is_folded_hash(fold::content_hash(content.trim_start_matches('\t')))
    {
        b = docket_core::edit::subtree_end(&text, b);
    }
    if let Some((new_text, caret_line)) = docket_core::edit::delete_items(&text, a, b) {
        let caret = buffer_text(doc)
            .split('\n')
            .take(caret_line)
            .map(|l| l.chars().count() + 1)
            .sum::<usize>();
        replace_document(doc, new_text);
        let clamped = caret.min(doc.buffer.char_count() as usize);
        doc.buffer
            .place_cursor(&doc.buffer.iter_at_offset(clamped as i32));
    }
}

fn cmd_select_branch(doc: &Rc<Doc>) {
    let text = buffer_text(doc).to_string();
    let (a, b) = selection_lines(doc);
    let start = docket_core::edit::branch_start(&text, a);
    let end = docket_core::edit::subtree_end(&text, b);
    let Some(start_iter) = doc.buffer.iter_at_line(start as i32) else {
        return;
    };
    // The selection ends at offset 0 of the next item, or at the
    // last line's end at the buffer edge.
    let end_iter = doc.buffer.iter_at_line(end as i32 + 1).unwrap_or_else(|| {
        let mut it = doc.buffer.iter_at_line(end as i32).unwrap();
        it.forward_to_line_end();
        it
    });
    doc.buffer.select_range(&start_iter, &end_iter);
}

fn cmd_format(doc: &Rc<Doc>, kind: docket_core::ItemKind) {
    let text = buffer_text(doc).to_string();
    let (a, b) = selection_lines(doc);
    if let Some(new_text) = docket_core::edit::format_items(&text, a, b, kind) {
        replace_document(doc, new_text);
    }
}

fn cmd_focus_in(doc: &Rc<Doc>) {
    let text = buffer_text(doc).to_string();
    let caret = doc.buffer.cursor_position().max(0) as usize;
    let caret_line = current_line(doc, caret);
    if let Some(p) = docket_core::fold::enclosing_project(&text, caret_line) {
        *doc.focus.borrow_mut() = Some(fold::content_hash(line_content(&text, p)));
        apply_focus(doc);
    }
}

fn cmd_focus_out(doc: &Rc<Doc>) {
    let text = buffer_text(doc).to_string();
    let scoped = focused_line(doc);
    let parent = scoped.and_then(|l| docket_core::fold::parent_project(&text, l));
    *doc.focus.borrow_mut() = parent.map(|p| fold::content_hash(line_content(&text, p)));
    apply_focus(doc);
}

fn cmd_go_home(doc: &Rc<Doc>) {
    *doc.focus.borrow_mut() = None;
    if doc.filter.borrow().is_some() {
        apply_filter(doc);
    } else {
        refresh_folds(doc);
    }
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
    autosave(doc);
}

/// Same, caret recomputed later by the fold refresh.
fn replace_document(doc: &Rc<Doc>, new_text: String) {
    doc.buffer.begin_user_action();
    *doc.loading.borrow_mut() = true;
    doc.buffer.set_text(&new_text);
    *doc.loading.borrow_mut() = false;
    doc.buffer.end_user_action();
    autosave(doc);
    refresh_folds(doc);
}

/// Write the buffer to the document's file. The changed handler
/// skips this while `loading` is set (programmatic set_text must not
/// double-write), so the whole-document edits above call it
/// explicitly: the disk is the truth the moment an edit lands.
fn autosave(doc: &Rc<Doc>) {
    if let Some(path) = doc.path.borrow().as_ref() {
        let text = doc
            .buffer
            .text(&doc.buffer.start_iter(), &doc.buffer.end_iter(), true);
        *doc.loading.borrow_mut() = true;
        let _ = std::fs::write(path, text.as_str());
        *doc.loading.borrow_mut() = false;
    }
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
    doc.buffer.set_text(&welcome::welcome());
    *doc.loading.borrow_mut() = false;
    *doc.path.borrow_mut() = None;
    *doc.monitor.borrow_mut() = None;
    doc.window.set_title(Some("Welcome - Docket"));
    rebuild_sidebar(doc);
}

/// Load template text into an untitled buffer (no file, no watch);
/// the save-as dialog names it.
fn load_untitled(doc: &Rc<Doc>, text: &str, name: &str) {
    *doc.loading.borrow_mut() = true;
    doc.buffer.set_text(text);
    *doc.loading.borrow_mut() = false;
    *doc.path.borrow_mut() = None;
    *doc.monitor.borrow_mut() = None;
    doc.window.set_title(Some(&format!("{name} - Docket")));
    rebuild_sidebar(doc);
}

/// Autosave: every buffer change writes the file, so the disk is
/// always the truth and there is no dirty state to lose. Programmatic
/// whole-document edits suppress the handler via `loading` and call
/// [`autosave`] themselves.
fn wire_autosave(doc: &Rc<Doc>) {
    doc.buffer.connect_changed(glib::clone!(
        #[strong]
        doc,
        move |_| {
            if *doc.loading.borrow() {
                return;
            }
            autosave(&doc);
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
                        .text(&doc.buffer.start_iter(), &doc.buffer.end_iter(), true);
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
                                true,
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

    // Search opens on Ctrl+Shift+F, mirroring the reference cluster's
    // Cmd+Shift+F Begin Editor Search; the sidebar's query rows reveal
    // it too. Ctrl+F stays reserved.
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
    app.set_accels_for_action("app.start-search", &["<Control><Shift>f"]);

    // File > New: a fresh single-document window is a fresh process
    // (the app is NON_UNIQUE).
    let new_list = gio::SimpleAction::new("new", None);
    new_list.connect_activate(move |_, _| {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let _ = std::process::Command::new(exe).spawn();
    });
    app.add_action(&new_list);
    app.set_accels_for_action("app.new", &["<Control>n"]);

    // New from template: spawns a fresh process loading the shipped
    // template untitled (the same window-per-document rule).
    let template_action = gio::SimpleAction::new("template", Some(glib::VariantTy::STRING));
    template_action.connect_activate(move |_, payload| {
        if let Some(id) = payload.and_then(|v| v.str()) {
            let Ok(exe) = std::env::current_exe() else {
                return;
            };
            let _ = std::process::Command::new(exe)
                .arg("--template")
                .arg(id)
                .spawn();
        }
    });
    app.add_action(&template_action);

    // The rest of the command surface: same helpers the keymap
    // calls, reachable from the menu.
    let archive = gio::SimpleAction::new("archive-done", None);
    archive.connect_activate(glib::clone!(
        #[strong]
        doc,
        move |_, _| cmd_archive(&doc)
    ));
    app.add_action(&archive);
    app.set_accels_for_action("app.archive-done", &["<Control><Shift>a"]);

    let duplicate = gio::SimpleAction::new("duplicate", None);
    duplicate.connect_activate(glib::clone!(
        #[strong]
        doc,
        move |_, _| cmd_duplicate(&doc)
    ));
    app.add_action(&duplicate);
    app.set_accels_for_action("app.duplicate", &["<Control><Shift>d"]);

    let group = gio::SimpleAction::new("group", None);
    group.connect_activate(glib::clone!(
        #[strong]
        doc,
        move |_, _| cmd_group(&doc)
    ));
    app.add_action(&group);
    app.set_accels_for_action("app.group", &["<Control><Alt>g"]);

    let delete_items = gio::SimpleAction::new("delete-items", None);
    delete_items.connect_activate(glib::clone!(
        #[strong]
        doc,
        move |_, _| cmd_delete_items(&doc)
    ));
    app.add_action(&delete_items);
    app.set_accels_for_action("app.delete-items", &["<Control><Alt><Shift>k"]);

    let select_branch = gio::SimpleAction::new("select-branch", None);
    select_branch.connect_activate(glib::clone!(
        #[strong]
        doc,
        move |_, _| cmd_select_branch(&doc)
    ));
    app.add_action(&select_branch);
    app.set_accels_for_action("app.select-branch", &["<Control><Shift>b"]);

    for (name, kind) in [
        ("format-project", docket_core::ItemKind::Project),
        ("format-task", docket_core::ItemKind::Task),
        ("format-note", docket_core::ItemKind::Note),
    ] {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(glib::clone!(
            #[strong]
            doc,
            move |_, _| cmd_format(&doc, kind)
        ));
        app.add_action(&action);
    }

    let focus_in = gio::SimpleAction::new("focus-in", None);
    focus_in.connect_activate(glib::clone!(
        #[strong]
        doc,
        move |_, _| cmd_focus_in(&doc)
    ));
    app.add_action(&focus_in);
    app.set_accels_for_action("app.focus-in", &["<Control><Alt>Right"]);

    let focus_out = gio::SimpleAction::new("focus-out", None);
    focus_out.connect_activate(glib::clone!(
        #[strong]
        doc,
        move |_, _| cmd_focus_out(&doc)
    ));
    app.add_action(&focus_out);
    app.set_accels_for_action("app.focus-out", &["<Control><Alt>Left"]);

    let go_home = gio::SimpleAction::new("go-home", None);
    go_home.connect_activate(glib::clone!(
        #[strong]
        doc,
        move |_, _| cmd_go_home(&doc)
    ));
    app.add_action(&go_home);
    app.set_accels_for_action("app.go-home", &["<Control><Shift>h"]);

    for (name, cmd, accel) in [
        ("expand-items", FoldCmd::ExpandItems, "<Control>0"),
        (
            "expand-completely",
            FoldCmd::ExpandCompletely,
            "<Control><Alt>0",
        ),
        ("collapse-items", FoldCmd::CollapseItems, "<Control>9"),
        (
            "collapse-completely",
            FoldCmd::CollapseCompletely,
            "<Control><Alt>9",
        ),
        ("expand-level", FoldCmd::ExpandLevel, "<Control><Shift>0"),
        (
            "collapse-level",
            FoldCmd::CollapseLevel,
            "<Control><Shift>9",
        ),
        (
            "expand-all-completely",
            FoldCmd::ExpandAllCompletely,
            "<Control><Alt><Shift>0",
        ),
        (
            "collapse-all-completely",
            FoldCmd::CollapseAllCompletely,
            "<Control><Alt><Shift>9",
        ),
    ] {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(glib::clone!(
            #[strong]
            doc,
            move |_, _| cmd_fold(&doc, cmd)
        ));
        app.add_action(&action);
        if !accel.is_empty() {
            app.set_accels_for_action(&format!("app.{name}"), &[accel]);
        }
    }
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
                    // A query that fails to parse yields an empty
                    // result; the error style surfaces the failure.
                    hide_all(&doc);
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

/// The completion-date stamp the done toggles apply: today's date
/// in the reference minimal form when the preference is on.
fn done_stamp() -> Option<String> {
    if !config::stamp_done() {
        return None;
    }
    let now = docket_core::dates::now();
    let today = docket_core::dates::parse("today", now)
        .map(|d| d.0)
        .unwrap_or(now);
    Some(docket_core::dates::format(today))
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
/// the reference, and evaluates within the focused project when one
/// is set (the sidebar's project click or the focus keys).
fn apply_filter(doc: &Rc<Doc>) {
    let Some((query, _)) = doc.filter.borrow().as_ref().map(|f| f.clone()) else {
        return;
    };
    let scope = focused_line(doc);
    apply_query(doc, &query, scope);
}

/// Recompute visibility after a focus change without a searchbar
/// filter: the focused subtree stays, everything else hides. With
/// no focus the document shows whole.
fn apply_focus(doc: &Rc<Doc>) {
    let scope = focused_line(doc);
    match scope {
        None => refresh_folds(doc),
        Some(_) => match docket_core::query::Query::parse("//") {
            Ok(all) => apply_query(doc, &all, scope),
            Err(_) => refresh_folds(doc),
        },
    }
}

/// The focused project's current line, resolved from its stored
/// content hash so edits above it do not strand the scope.
fn focused_line(doc: &Rc<Doc>) -> Option<usize> {
    let hash = (*doc.focus.borrow())?;
    let text = buffer_text(doc);
    text.split('\n')
        .enumerate()
        .find(|(_, line)| fold::content_hash(line.trim_start_matches('\t')) == hash)
        .map(|(i, _)| i)
}

/// The item content of one line, tabs stripped.
fn line_content(text: &str, line: usize) -> &str {
    text.split('\n')
        .nth(line)
        .map(|l| l.trim_start_matches('\t'))
        .unwrap_or("")
}

/// The shared visibility pass behind filtering and focus: matches
/// plus their ancestor chains stay visible, everything else hides.
fn apply_query(doc: &Rc<Doc>, query: &docket_core::query::Query, scope: Option<usize>) {
    let text = buffer_text(doc).to_string();
    let line_count = text.split('\n').count();
    let doc_for_run = docket_core::Document::load(&text);
    let matches = query.run_scoped_at(&doc_for_run, scope, docket_core::dates::now());
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

    // Gutter: actual folds keep their state; partially shown
    // ancestors dim their handle per the spec.
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
            if doc.folds.is_folded_hash(fold::content_hash(content)) {
                folded.insert(i as u32);
            }
        }
    }
    doc.renderer.set_lines(foldable, folded, dimmed);
    guard_caret(doc);
}

/// Hide the whole document: a failed query's empty result, per the
/// spec's failure semantics. The error style on the entry carries
/// the explanation.
fn hide_all(doc: &Rc<Doc>) {
    doc.buffer.remove_tag(
        &doc.folds.tag,
        &doc.buffer.start_iter(),
        &doc.buffer.end_iter(),
    );
    doc.buffer.apply_tag(
        &doc.folds.tag,
        &doc.buffer.start_iter(),
        &doc.buffer.end_iter(),
    );
    doc.renderer
        .set_lines(HashSet::new(), HashSet::new(), HashSet::new());
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
    // Saved searches: the global config file plus the document's own
    // embedded `Searches:` rows (the spec's either/or), config first.
    let mut searches = config::searches();
    for row in docket_core::census::search_rows(&text) {
        if !searches.iter().any(|(label, _)| *label == row.label) {
            searches.push((row.label, row.query));
        }
    }
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
                *doc_cb.focus.borrow_mut() = None;
                *doc_cb.filter.borrow_mut() = None;
                entry.set_text("");
                bar.set_search_mode(false);
                refresh_folds(&doc_cb);
            }
            sidebar::RowKind::Project { line } => {
                // A project click focuses: the subtree stays,
                // everything else hides (reference sidebar behavior).
                let text = buffer_text(&doc_cb).to_string();
                *doc_cb.focus.borrow_mut() = Some(fold::content_hash(line_content(&text, *line)));
                apply_focus(&doc_cb);
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

/// The hamburger menu: the command surface, grouped the way the
/// reference groups its menus. Accelerators render from the app's
/// registered accels.
fn build_menu() -> gio::Menu {
    let menu = gio::Menu::new();

    let file = gio::Menu::new();
    file.append(Some("New list"), Some("app.new"));
    file.append(Some("Open"), Some("app.open"));
    file.append(Some("Save list as"), Some("app.save-as"));
    file.append(Some("Quit"), Some("app.quit"));
    menu.append_section(Some("List"), &file);

    let template_section = gio::Menu::new();
    for (name, _) in templates::TEMPLATES {
        let escaped = name.replace('\'', "\\'");
        template_section.append(Some(name), Some(&format!("app.template('{escaped}')")));
    }
    menu.append_section(Some("New from template"), &template_section);

    let item = gio::Menu::new();
    item.append(Some("Archive done"), Some("app.archive-done"));
    item.append(Some("Duplicate"), Some("app.duplicate"));
    item.append(Some("Group"), Some("app.group"));
    item.append(Some("Delete items"), Some("app.delete-items"));
    item.append(Some("Select branch"), Some("app.select-branch"));
    let format = gio::Menu::new();
    format.append(Some("Project"), Some("app.format-project"));
    format.append(Some("Task"), Some("app.format-task"));
    format.append(Some("Note"), Some("app.format-note"));
    item.append_submenu(Some("Format as"), &format);
    menu.append_section(Some("Item"), &item);

    let outline = gio::Menu::new();
    outline.append(Some("Focus in"), Some("app.focus-in"));
    outline.append(Some("Focus out"), Some("app.focus-out"));
    outline.append(Some("Go home"), Some("app.go-home"));
    let folds = gio::Menu::new();
    folds.append(Some("Expand items"), Some("app.expand-items"));
    folds.append(
        Some("Expand items completely"),
        Some("app.expand-completely"),
    );
    folds.append(Some("Collapse items"), Some("app.collapse-items"));
    folds.append(
        Some("Collapse items completely"),
        Some("app.collapse-completely"),
    );
    folds.append(Some("Expand all by level"), Some("app.expand-level"));
    folds.append(Some("Collapse all by level"), Some("app.collapse-level"));
    folds.append(
        Some("Expand all completely"),
        Some("app.expand-all-completely"),
    );
    folds.append(
        Some("Collapse all completely"),
        Some("app.collapse-all-completely"),
    );
    outline.append_submenu(Some("Folds"), &folds);
    menu.append_section(Some("Outline"), &outline);

    let view = gio::Menu::new();
    view.append(Some("Search"), Some("app.start-search"));
    menu.append_section(Some("Search"), &view);

    menu
}
