use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Filename to load configuration from
    #[arg(short, long)]
    pub config: Option<String>,

    /// Name of path to theme
    #[arg(short, long)]
    pub theme: Option<String>,
}
