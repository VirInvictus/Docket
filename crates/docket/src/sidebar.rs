//! The sidebar: Home, Projects, Searches, Tags — a ListBox whose
//! rows carry a kind and the data each click applies to the editor.
//! Click semantics per the reference: Home clears scope and filter,
//! a project row scopes the editor to that project, a search row
//! applies its query, a tag row filters by the tag, a tag-value row
//! filters by the tag with the value.

use gtk4::prelude::*;
use gtk4::{ListBox, ListBoxRow};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    Home,
    Group(&'static str),
    Project { line: usize },
    Search { query: String },
    Tag { name: String },
    TagValue { name: String, value: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    pub label: String,
    pub indent: usize,
    pub header: bool,
}

/// Build the sidebar row model from a document census.
pub struct SidebarModel {
    pub rows: Vec<Row>,
}

pub fn build_model(
    text: &str,
    searches: &[(String, String)],
    include_tags: &[String],
    exclude_tags: &[String],
) -> SidebarModel {
    use docket_core::census;
    let mut rows = Vec::new();
    rows.push(Row {
        kind: RowKind::Home,
        label: "Home".into(),
        indent: 0,
        header: false,
    });

    let projects = census::project_rows(text);
    rows.push(Row {
        kind: RowKind::Group("Projects"),
        label: "Projects".into(),
        indent: 0,
        header: true,
    });
    for p in &projects {
        rows.push(Row {
            kind: RowKind::Project { line: p.line },
            label: p.name.clone(),
            indent: 1 + p.level,
            header: false,
        });
    }

    rows.push(Row {
        kind: RowKind::Group("Searches"),
        label: "Searches".into(),
        indent: 0,
        header: true,
    });
    for (label, query) in searches {
        rows.push(Row {
            kind: RowKind::Search {
                query: query.clone(),
            },
            label: label.clone(),
            indent: 1,
            header: false,
        });
    }

    rows.push(Row {
        kind: RowKind::Group("Tags"),
        label: "Tags".into(),
        indent: 0,
        header: true,
    });
    let census = census::tag_census(text, exclude_tags);
    let mut listed: Vec<String> = Vec::new();
    for (name, values) in &census {
        listed.push(name.clone());
        rows.push(Row {
            kind: RowKind::Tag { name: name.clone() },
            label: format!("@{name}"),
            indent: 1,
            header: false,
        });
        let mut sorted = values.clone();
        sorted.sort();
        for value in &sorted {
            rows.push(Row {
                kind: RowKind::TagValue {
                    name: name.clone(),
                    value: value.clone(),
                },
                label: value.clone(),
                indent: 2,
                header: false,
            });
        }
    }
    // Include-list tags appear even with zero uses.
    for tag in include_tags {
        if !listed.contains(tag) {
            rows.push(Row {
                kind: RowKind::Tag { name: tag.clone() },
                label: format!("@{tag}"),
                indent: 1,
                header: false,
            });
        }
    }
    SidebarModel { rows }
}

/// The widget: a scrollable ListBox rebuilt from a SidebarModel.
/// The row-activation callback: receives the row's kind.
pub type SidebarCallback = Rc<dyn Fn(&RowKind)>;

pub struct Sidebar {
    list: ListBox,
    on_activate: RefCell<Option<SidebarCallback>>,
}

impl Clone for Sidebar {
    fn clone(&self) -> Self {
        Sidebar {
            list: self.list.clone(),
            on_activate: RefCell::new(None),
        }
    }
}

impl Sidebar {
    pub fn new() -> Self {
        let list = ListBox::new();
        list.set_selection_mode(gtk4::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        list.add_css_class("docket-sidebar");
        let _ = Rc::new(());
        Sidebar {
            list,
            on_activate: RefCell::new(None),
        }
    }

    pub fn list(&self) -> &ListBox {
        &self.list
    }

    pub fn row_count(&self) -> usize {
        let mut n = 0;
        let mut iter = self.list.first_child();
        while let Some(row) = iter {
            n += 1;
            iter = row.next_sibling();
        }
        n
    }

    pub fn set_on_activate(&self, cb: SidebarCallback) {
        *self.on_activate.borrow_mut() = Some(cb);
    }

    fn callback_handle(&self) -> SidebarCallback {
        self.on_activate
            .borrow()
            .clone()
            .unwrap_or_else(|| Rc::new(|_| {}))
    }

    pub fn rebuild(&self, model: &SidebarModel) {
        self.list.remove_all();
        for row in &model.rows {
            let box_row = ListBoxRow::new();
            box_row.set_selectable(!row.header);
            box_row.set_activatable(!row.header);
            let label = gtk4::Label::new(Some(&row.label));
            label.set_xalign(0.0);
            label.set_margin_start((row.indent * 16) as i32);
            if row.header {
                label.add_css_class("heading");
                label.add_css_class("caption");
            }
            box_row.set_child(Some(&label));
            let kind = row.kind.clone();
            let cb = self.callback_handle();
            box_row.connect_activate(move |_| {
                cb(&kind);
            });
            self.list.append(&box_row);
        }
    }
}

impl Default for Sidebar {
    fn default() -> Self {
        Self::new()
    }
}
