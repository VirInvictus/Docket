//! Docket: a plain-text task editor. One window per document, the
//! buffer is the file, autosave keeps disk always current, and an
//! external change reloads the view because the view never holds
//! unsaved state.

mod styles;
mod welcome;

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, ScrolledWindow};
use sourceview5::View;
use sourceview5::prelude::*;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const APP_ID: &str = "io.github.virinvictus.Docket";

/// Everything the window needs to keep between events.
struct Doc {
    window: ApplicationWindow,
    buffer: sourceview5::Buffer,
    path: RefCell<Option<PathBuf>>,
    /// Set while the buffer is being filled programmatically, so the
    /// changed handler neither autosaves nor races the monitor.
    loading: RefCell<bool>,
    monitor: RefCell<Option<gio::FileMonitor>>,
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
    styles::install();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Docket")
        .default_width(760)
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
    window.set_child(Some(&scrolled));

    let doc = Rc::new(Doc {
        window: window.clone(),
        buffer,
        path: RefCell::new(None),
        loading: RefCell::new(true),
        monitor: RefCell::new(None),
    });
    DOC.with(|slot| *slot.borrow_mut() = Some(doc.clone()));

    wire_autosave(&doc);
    wire_actions(app, &window, &doc);

    match initial {
        Some(path) => load_file(&doc, path),
        None => load_welcome(&doc),
    }
    window.present();
}

/// Fill the buffer from a file, watch it, and remember it in
/// recents. The file on disk wins; the buffer is just the view.
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
    doc.window.set_title(Some(&format!("{name} — Docket")));
    watch(doc, path);
    let file = gio::File::for_path(path);
    gtk4::RecentManager::default().add_item(&file.uri());
}

/// The first-run document: an untitled buffer with the welcome text.
/// It autosaves nowhere until the user names it.
fn load_welcome(doc: &Rc<Doc>) {
    *doc.loading.borrow_mut() = true;
    doc.buffer.set_text(welcome::WELCOME);
    *doc.loading.borrow_mut() = false;
    *doc.path.borrow_mut() = None;
    *doc.monitor.borrow_mut() = None;
    doc.window.set_title(Some("Welcome — Docket"));
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
}
