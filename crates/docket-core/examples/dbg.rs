fn main() {
    let q = docket_core::Query::parse("P.child::*").unwrap();
    println!("{q:#?}");
}
