//! The configuration files: saved searches and the tag include and
//! exclude lists. Both live under ~/.config/docket/ and are seeded
//! from bundled defaults on first run, then re-read on every sidebar
//! rebuild (the reference hot-reloads on disk change; a re-read per
//! rebuild covers the same ground at sidebar scale).

use std::path::PathBuf;

const SEARCHES_SEED: &str = "Saved searches for Docket. Edit freely; changes apply on rebuild.\nSearches:\n\tToday @search(@today union @due <[d] tomorrow)\n\tNot Done @search(not @done except @done//*)\n";
const TAGS_SEED: &str = "Tag sidebar configuration for Docket. Include tags appear even when unused; exclude tags never appear.\nInclude Tags: @due @start @today @done\nExclude Tags: @search\n";

pub fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("docket"))
}

fn seed(path: &PathBuf, content: &str) {
    if !path.exists() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, content);
    }
}

/// The saved searches as (label, query) pairs.
pub fn searches() -> Vec<(String, String)> {
    let Some(dir) = config_dir() else {
        return Vec::new();
    };
    let path = dir.join("searches.taskpaper");
    seed(&path, SEARCHES_SEED);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    docket_core::census::search_rows(&text)
        .into_iter()
        .map(|r| (r.label, r.query))
        .collect()
}

/// (include, exclude) tag-name lists from the tags config.
pub fn tag_lists() -> (Vec<String>, Vec<String>) {
    let Some(dir) = config_dir() else {
        return (Vec::new(), Vec::new());
    };
    let path = dir.join("tags.taskpaper");
    seed(&path, TAGS_SEED);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (Vec::new(), Vec::new());
    };
    let grab = |needle: &str| -> Vec<String> {
        text.split('\n')
            .find(|l| l.trim_start_matches('\t').trim_end().starts_with(needle))
            .map(|l| {
                docket_core::tags::scan(l)
                    .into_iter()
                    .map(|t| t.name.to_string())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default()
    };
    // The line "Include Tags: @due @start..." yields no @-tag named
    // after the label itself; scan returns the tags on the line.
    let include = grab("Include Tags:");
    let exclude = grab("Exclude Tags:");
    (include, exclude)
}
