//! Shipped list templates (the Quire mirror): a planning skeleton,
//! a weekly dashboard, and a packing list. Headless on purpose:
//! every template must parse, round-trip idempotently, and carry
//! no broken structure.

pub const TEMPLATES: &[(&str, &str)] = &[
    (
        "Project plan",
        include_str!("../resources/templates/project-plan.taskpaper"),
    ),
    (
        "This week",
        include_str!("../resources/templates/week.taskpaper"),
    ),
    (
        "Packing list",
        include_str!("../resources/templates/packing.taskpaper"),
    ),
];

#[cfg(test)]
mod tests {
    use docket_core::Document;

    /// A shipped template is a working document: it parses,
    /// round-trips byte-for-byte from the second pass, and
    /// demonstrates at least one project.
    #[test]
    fn templates_are_well_formed_documents() {
        for (name, text) in super::TEMPLATES {
            let doc = Document::load(text);
            let once = doc.serialize();
            assert_eq!(
                Document::load(&once).serialize(),
                once,
                "{name} does not round-trip"
            );
            let kinds: Vec<_> = doc.items.iter().map(|i| i.kind()).collect();
            assert!(
                kinds.contains(&docket_core::ItemKind::Project),
                "{name} shows no project"
            );
        }
    }
}
