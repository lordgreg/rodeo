//! Regenerates the man page from the clap definition.
//!
//!     cargo run --example gen_man
//!
//! `tests/man.rs` fails if the checked-in page no longer matches, so `--help`
//! and `rodeo.1` cannot drift apart.

use clap::CommandFactory;

fn main() -> std::io::Result<()> {
    let page = rodeo::cli::man_page()?;
    let path = std::path::Path::new("docs/rodeo.1");
    std::fs::write(path, page)?;
    println!(
        "wrote {} for {}",
        path.display(),
        rodeo::cli::Args::command().get_name()
    );
    Ok(())
}
