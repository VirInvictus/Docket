//! The first-run start page. Docket's own words: a working task
//! list that demos the format, the folding, the searches, and the
//! sidebar in one screen. Generated with live dates so the seeded
//! Today search returns hits the moment the app opens.

use docket_core::dates;

/// Today's date in the reference minimal form.
fn day(offset: i64) -> String {
    let now = dates::now();
    let target = dates::parse(&format!("today {offset:+} days"), now)
        .map(|d| d.0)
        .unwrap_or(now);
    dates::format(target)
}

/// The start page document, dated against now.
pub fn welcome() -> String {
    let today = day(0);
    let tomorrow = day(1);
    let weekend = day(3);

    format!(
        "Welcome to Docket\n\
\n\
A plain-text task list that folds, filters, and completes like an\n\
outliner. Everything you see is the file itself: no database, no\n\
account, any editor can open it.\n\
\n\
Start here:\n\
\t- Press Ctrl+Shift+F and type @today to see the filter bite\n\
\t- Click any tag in this text to filter by it\n\
\t- Click the fold arrows in the gutter to collapse a section\n\
\t- Toggle a task done with Ctrl+D, done rows strike through\n\
\t- Tab indents, Ctrl+Alt+arrows move rows, double-click a sidebar\n\
\t  project to focus it\n\
\t- The menu in the headerbar holds every command\n\
\n\
The format in one screen:\n\
\tProjects end with a colon:\n\
\t\t- tasks start with a dash and nest by indentation\n\
\t\t- finished tasks carry @done and strike through\n\
\tNotes are any other line: they hold prose, plans, ideas\n\
\tTags annotate anything: @priority(1), @context(home), @due({tomorrow})\n\
\n\
Searches:\n\
\tToday @search(@today union @due <[d] tomorrow)\n\
\tOpen @search(not @done except @done//*)\n\
\tHigh priority @search(@priority <=[n] 1)\n\
\n\
A tiny project, live:\n\
\tPlan a weekend away:\n\
\t\t- check the weather @start({today}) @context(home)\n\
\t\t- pick the destination @context(home) @due({weekend})\n\
\t\t- book the transport @priority(1) @due({tomorrow})\n\
\t\t- pack bags @done({today})\n\
\tNotes carry the details:\n\
\t\tLeave on the {weekend}. The file is the database: Docket\n\
\t\tnever takes the text away from you.\n\
\n\
Where to go next:\n\
\t~/.config/docket/ holds your saved searches and tag settings, and\n\
\tpreferences.taskpaper has the done-stamp preference. The query\n\
\tlanguage does dates, durations, paths, and set algebra - every\n\
\tsearch in the sidebar is a live query over this file.\n"
    )
}

#[cfg(test)]
mod tests {
    use docket_core::{Document, Query, census};

    #[test]
    fn the_start_page_is_valid_taskpaper() {
        let doc = Document::load(&super::welcome());
        // Parseable and idempotent: the start page must be a
        // well-formed .taskpaper file.
        let once = doc.serialize();
        assert_eq!(Document::load(&once).serialize(), once);
        // It demonstrates each of the four kinds.
        let kinds: Vec<_> = doc.items.iter().map(|i| i.kind()).collect();
        assert!(kinds.contains(&docket_core::ItemKind::Project));
        assert!(kinds.contains(&docket_core::ItemKind::Task));
        assert!(kinds.contains(&docket_core::ItemKind::Note));
    }

    #[test]
    fn the_start_page_searches_all_parse() {
        // Every embedded saved search is a live query; a parse
        // failure would ship a broken sidebar row.
        for row in census::search_rows(&super::welcome()) {
            assert!(
                Query::parse(&row.query).is_ok(),
                "embedded search {:?} does not parse",
                row.label
            );
        }
    }

    #[test]
    fn the_today_search_hits_the_start_page() {
        // The point of the live dates: the seeded Today search
        // returns real rows on first open.
        let doc = Document::load(&super::welcome());
        let results = Query::parse("@today union @due <[d] tomorrow")
            .unwrap()
            .run(&doc);
        assert!(!results.is_empty());
    }
}
