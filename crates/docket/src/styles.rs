//! Bundled language spec + style scheme. Same pattern as Quire: the
//! files ship in `resources/styles/` and are extracted at startup to
//! the user data dir, where the GtkSourceView managers can load
//! them. Must run after GTK is initialised and before the first
//! buffer asks for a language: the default managers snapshot their
//! search paths on first use.

use std::path::PathBuf;

const LANG: &[u8] = include_bytes!("../resources/styles/docket.lang");
const SCHEME: &[u8] = include_bytes!("../resources/styles/docket-dark.xml");

pub fn install() {
    let Some(base) = data_dir() else {
        return;
    };
    write_out(&base.join("lang"), "docket.lang", LANG);
    write_out(&base.join("styles"), "docket-dark.xml", SCHEME);

    let lm = sourceview5::LanguageManager::default();
    let mut paths: Vec<String> = lm.search_path().iter().map(|p| p.to_string()).collect();
    if let Some(dir) = base.join("lang").to_str() {
        paths.push(dir.to_string());
    }
    let refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    lm.set_search_path(&refs);

    let ssm = sourceview5::StyleSchemeManager::default();
    if let Some(dir) = base.join("styles").to_str() {
        ssm.prepend_search_path(dir);
    }
}

/// The `docket` language, if it loaded.
pub fn language() -> Option<sourceview5::Language> {
    sourceview5::LanguageManager::default().language("docket")
}

/// The Kanagawa Dragon scheme, if it loaded.
pub fn scheme() -> Option<sourceview5::StyleScheme> {
    sourceview5::StyleSchemeManager::default().scheme("docket-dark")
}

fn write_out(dir: &std::path::Path, name: &str, bytes: &[u8]) {
    let path = dir.join(name);
    let stale = std::fs::read(&path)
        .map(|old| old.len() != bytes.len())
        .unwrap_or(true);
    if stale && std::fs::create_dir_all(dir).is_ok() {
        let _ = std::fs::write(&path, bytes);
    }
}

fn data_dir() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("docket"))
}
