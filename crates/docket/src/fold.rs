//! Folding: a fold gutter renderer plus the fold engine. GSV 5 has
//! no native folding, so folds are an invisible-text tag over whole
//! line ranges and a custom left-gutter renderer that draws the
//! fold markers and handles clicks.
//!
//! State is a set of content hashes of folded header lines (the
//! reference approach: content-derived keys survive edits that
//! preserve content).

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use sourceview5::GutterRenderer;
use sourceview5::prelude::*;
use std::cell::RefCell;
use std::collections::HashSet;

/// FNV-1a 64-bit over chars; the content hash keying the sidecar.
pub fn content_hash(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub fn hash_hex(hash: u64) -> String {
    format!("{hash:016x}")
}

/// Per-document fold state: the set of folded header-line hashes,
/// derived visibility, and the invisible tag that implements it.
pub struct FoldEngine {
    pub tag: gtk4::TextTag,
    folded: RefCell<HashSet<u64>>,
}

impl FoldEngine {
    pub fn new(buffer: &sourceview5::Buffer) -> Self {
        let tag = buffer
            .create_tag(None, &[("invisible", &true)])
            .expect("invisible tag creation");
        FoldEngine {
            tag,
            folded: RefCell::new(HashSet::new()),
        }
    }

    pub fn is_folded_hash(&self, hash: u64) -> bool {
        self.folded.borrow().contains(&hash)
    }

    pub fn toggle_hash(&self, hash: u64) -> bool {
        let mut set = self.folded.borrow_mut();
        if set.remove(&hash) {
            false
        } else {
            set.insert(hash);
            true
        }
    }

    pub fn folded_hashes(&self) -> Vec<u64> {
        self.folded.borrow().iter().copied().collect()
    }

    /// Fold the given header lines' content hashes (the keyboard
    /// fold commands).
    pub fn fold_lines(&self, hashes: &[u64]) {
        self.folded.borrow_mut().extend(hashes.iter().copied());
    }

    /// Unfold the given header lines' content hashes.
    pub fn unfold_lines(&self, hashes: &[u64]) {
        let mut set = self.folded.borrow_mut();
        for h in hashes {
            set.remove(h);
        }
    }

    pub fn restore(&self, hashes: &[u64]) {
        *self.folded.borrow_mut() = hashes.iter().copied().collect();
    }

    /// Re-derive folds from the buffer: a foldable line is any item
    /// line whose next line nests deeper. Applies or removes the
    /// invisible tag over each folded subtree (header excluded).
    pub fn reapply(&self, buffer: &sourceview5::Buffer) {
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), true)
            .to_string();
        let doc_lines: Vec<&str> = text.split('\n').collect();

        buffer.remove_tag(&self.tag, &buffer.start_iter(), &buffer.end_iter());
        let mut line = 0;
        while line + 1 < doc_lines.len() {
            let line_text = doc_lines[line];
            let content = line_text.trim_start_matches('\t');
            let depth = line_text.chars().count() - content.len();
            if !content.is_empty() {
                let next = doc_lines[line + 1];
                let next_depth = next.chars().count() - next.trim_start_matches('\t').len();
                if next_depth > depth {
                    let hash = content_hash(content);
                    if self.is_folded_hash(hash) {
                        let mut last = line;
                        let mut probe = line + 1;
                        while probe < doc_lines.len() {
                            let p = doc_lines[probe];
                            let d = p.chars().count() - p.trim_start_matches('\t').len();
                            if d <= depth {
                                break;
                            }
                            last = probe;
                            probe += 1;
                        }
                        if let (Some(start), Some(mut end)) = (
                            buffer.iter_at_line(line as i32 + 1),
                            buffer.iter_at_line(last as i32),
                        ) {
                            end.forward_to_line_end();
                            buffer.apply_tag(&self.tag, &start, &end);
                        }
                    }
                }
            }
            line += 1;
        }
    }
}

/// The fold state the renderer reads: foldable lines, currently
/// folded lines, and lines whose handle dims (partially shown
/// ancestors under an active filter), all by line index.
#[derive(Default)]
struct FoldView {
    foldable: HashSet<u32>,
    folded: HashSet<u32>,
    dimmed: HashSet<u32>,
}

/// The click callback type: a header line index.
pub type ToggleCallback = Box<dyn Fn(u32)>;

pub struct FoldRenderer {
    view_state: RefCell<FoldView>,
    /// Clicks are forwarded through this closure so the engine and
    /// the sidecar stay outside the GObject.
    pub on_toggle: RefCell<Option<ToggleCallback>>,
}

impl Default for FoldRenderer {
    fn default() -> Self {
        FoldRenderer {
            view_state: RefCell::new(FoldView::default()),
            on_toggle: RefCell::new(None),
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for FoldRenderer {
    const NAME: &'static str = "DocketFoldRenderer";
    type Type = FoldGutterRenderer;
    type ParentType = GutterRenderer;
}

impl ObjectImpl for FoldRenderer {
    fn constructed(&self) {
        self.parent_constructed();
        let obj = self.obj();
        obj.set_alignment_mode(sourceview5::GutterRendererAlignmentMode::Cell);
        obj.set_xpad(8);
        obj.set_visible(true);
    }
}

impl WidgetImpl for FoldRenderer {
    // Reserve the gutter column; without a width the renderer draws
    // into a zero-width slot and nothing shows.
    fn measure(&self, orientation: gtk4::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
        match orientation {
            gtk4::Orientation::Horizontal => (22, 22, -1, -1),
            _ => self.parent_measure(orientation, for_size),
        }
    }
}

impl sourceview5::subclass::gutter_renderer::GutterRendererImpl for FoldRenderer {
    fn snapshot_line(
        &self,
        snapshot: &gtk4::Snapshot,
        _lines: &sourceview5::GutterLines,
        line: u32,
    ) {
        if !self.view_state.borrow().foldable.contains(&line) {
            return;
        }
        let (folded, dimmed) = {
            let state = self.view_state.borrow();
            (state.folded.contains(&line), state.dimmed.contains(&line))
        };
        let marker = if folded { "\u{25be}" } else { "\u{25b8}" };
        let obj = self.obj();
        let layout = obj.create_pango_layout(Some(marker));
        layout.set_font_description(Some(&gtk4::pango::FontDescription::from_string("10")));
        let (w, h) = layout.pixel_size();
        let (_, natural, _, _) = obj.measure(gtk4::Orientation::Vertical, -1);
        let y = natural as f32 / 2.0 - h as f32 / 2.0;
        snapshot.save();
        snapshot.translate(&gtk4::graphene::Point::new(4.0, y.max(1.0).round()));
        let color = if dimmed { "#625e5a" } else { "#8ba4b0" };
        snapshot.append_layout(&layout, &gdk::RGBA::parse(color).unwrap());
        snapshot.restore();
        let _ = w;
    }

    fn query_activatable(&self, iter: &gtk4::TextIter, _area: &gdk::Rectangle) -> bool {
        let line = iter.line() as u32;
        self.view_state.borrow().foldable.contains(&line)
    }

    fn activate(
        &self,
        iter: &gtk4::TextIter,
        _area: &gdk::Rectangle,
        _button: u32,
        _state: gdk::ModifierType,
        _n_press: i32,
    ) {
        let line = iter.line() as u32;
        if let Some(cb) = self.on_toggle.borrow().as_ref() {
            cb(line);
        }
    }
}

glib::wrapper! {
    pub struct FoldGutterRenderer(ObjectSubclass<FoldRenderer>)
        @extends GutterRenderer, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl FoldGutterRenderer {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn set_on_toggle(&self, cb: Box<dyn Fn(u32)>) {
        self.imp().on_toggle.replace(Some(cb));
    }

    pub fn set_lines(&self, foldable: HashSet<u32>, folded: HashSet<u32>, dimmed: HashSet<u32>) {
        {
            let mut state = self.imp().view_state.borrow_mut();
            state.foldable = foldable;
            state.folded = folded;
            state.dimmed = dimmed;
        }
        self.queue_draw();
    }
}
