//! Editor tag completion: a GtkSourceView completion provider fed
//! live by the document census. GtkSourceView 5's completion has no
//! proposal type to instantiate, so proposals are plain GObjects
//! carrying the tag name; the provider proposes on a trailing
//! partial `@tag` and replaces the word bounds on activation.

use glib::subclass::prelude::*;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use sourceview5::subclass::prelude::*;
use sourceview5::{
    CompletionCell, CompletionContext, CompletionProposal, CompletionProvider, View,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Where the tag names come from: the document census, recomputed
/// per proposal so edits never leave a stale list.
pub type TagSource = Rc<dyn Fn() -> Vec<String>>;

mod proposal_imp {
    use super::*;

    #[derive(Default)]
    pub struct TagProposal {
        pub tag: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TagProposal {
        const NAME: &'static str = "DocketTagProposal";
        type Type = super::TagProposal;
        type ParentType = glib::Object;
        type Interfaces = (CompletionProposal,);
    }

    impl ObjectImpl for TagProposal {}

    // An empty marker interface in this GtkSourceView generation:
    // all the data is ours.
    impl CompletionProposalImpl for TagProposal {}
}

glib::wrapper! {
    pub struct TagProposal(ObjectSubclass<proposal_imp::TagProposal>)
        @implements CompletionProposal;
}

impl TagProposal {
    fn new(tag: &str) -> Self {
        let p: TagProposal = glib::Object::builder().build();
        *p.imp().tag.borrow_mut() = tag.to_string();
        p
    }

    fn tag(&self) -> String {
        self.imp().tag.borrow().clone()
    }
}

mod provider_imp {
    use super::*;

    #[derive(Default)]
    pub struct TagProvider {
        pub tags: RefCell<Option<TagSource>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TagProvider {
        const NAME: &'static str = "DocketTagProvider";
        type Type = super::TagProvider;
        type ParentType = glib::Object;
        type Interfaces = (CompletionProvider,);
    }

    impl ObjectImpl for TagProvider {}

    impl CompletionProviderImpl for TagProvider {
        // Typing `@` opens the context even though it is not a word
        // character; the letters after it refine the list.
        fn is_trigger(&self, _iter: &gtk4::TextIter, c: char) -> bool {
            c == '@'
        }

        fn populate(&self, context: &CompletionContext) -> Result<gio::ListModel, glib::Error> {
            let store = gio::ListStore::new::<TagProposal>();
            // The word bounds cover only the letters after `@`; the
            // tag context is the `@` just before them.
            if let Some((mut begin, _)) = context.bounds()
                && begin.backward_char()
                && begin.char() == '@'
            {
                let word = context.word().to_string();
                let hits: Vec<TagProposal> = self
                    .matching(&word)
                    .into_iter()
                    .map(|t| TagProposal::new(&t))
                    .collect();
                store.extend_from_slice(&hits);
            }
            Ok(store.upcast())
        }

        fn refilter(&self, context: &CompletionContext, model: &gio::ListModel) {
            let Some(store) = model.downcast_ref::<gio::ListStore>() else {
                return;
            };
            let word = context.word().to_string();
            let hits: Vec<glib::Object> = self
                .matching(&word)
                .into_iter()
                .map(|t| TagProposal::new(&t).upcast())
                .collect();
            store.splice(0, store.n_items(), &hits);
        }

        fn display(
            &self,
            _context: &CompletionContext,
            proposal: &CompletionProposal,
            cell: &CompletionCell,
        ) {
            if let Some(p) = proposal.downcast_ref::<TagProposal>()
                && cell.column() == sourceview5::CompletionColumn::TypedText
            {
                cell.set_text(Some(&p.tag()));
            }
        }

        fn activate(&self, context: &CompletionContext, proposal: &CompletionProposal) {
            let Some(p) = proposal.downcast_ref::<TagProposal>() else {
                return;
            };
            let Some(buffer) = context.buffer() else {
                return;
            };
            let Some((mut begin, mut end)) = context.bounds() else {
                return;
            };
            // Replace the typed partial with the whole tag name; the
            // `@` before the bounds stays. One undo step.
            buffer.begin_user_action();
            buffer.delete(&mut begin, &mut end);
            buffer.insert(&mut begin, &p.tag());
            buffer.end_user_action();
        }
    }

    impl TagProvider {
        fn matching(&self, word: &str) -> Vec<String> {
            let source = self.tags.borrow().clone();
            let Some(source) = source else {
                return Vec::new();
            };
            source()
                .into_iter()
                .filter(|t| t.starts_with(word) && *t != word)
                .collect()
        }
    }
}

glib::wrapper! {
    pub struct TagProvider(ObjectSubclass<provider_imp::TagProvider>)
        @implements CompletionProvider;
}

impl TagProvider {
    pub fn new(tags: TagSource) -> Self {
        let p: TagProvider = glib::Object::builder().build();
        *p.imp().tags.borrow_mut() = Some(tags);
        p
    }
}

/// Attach the tag provider to the editor view.
pub fn install(view: &View, tags: TagSource) {
    let provider = TagProvider::new(tags);
    sourceview5::prelude::ViewExt::completion(view).add_provider(&provider);
}
