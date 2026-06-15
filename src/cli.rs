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

    /// Left panel path (will overwrite initial_directory from config)
    #[arg(short, long)]
    pub left: Option<String>,

    /// Right panel path (will overwrite initial_directory from config)
    #[arg(short, long)]
    pub right: Option<String>,
}
