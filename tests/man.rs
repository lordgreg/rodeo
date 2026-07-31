//! Guards the checked-in man page against drift.

#[test]
fn man_page_matches_the_cli_definition() {
    let generated = rodeo::cli::man_page().expect("man page renders");
    let checked_in = std::fs::read("docs/rodeo.1").expect("docs/rodeo.1 is checked in");

    assert_eq!(
        String::from_utf8_lossy(&generated),
        String::from_utf8_lossy(&checked_in),
        "docs/rodeo.1 is out of date — run `cargo run --example gen_man`"
    );
}
