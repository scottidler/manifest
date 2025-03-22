// src/cli.rs

use clap::Parser;

/// Generate bash installation manifest from YAML spec
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// default="./manifest.yml"; specify the config path
    #[arg(short = 'C', long, default_value = "./manifest.yml")]
    pub config: String,

    /// default="."; set the cwd
    #[arg(short = 'D', long, default_value = ".")]
    pub cwd: String,

    /// default="$HOME"; specify HOME if not current
    #[arg(short = 'H', long, default_value_t = std::env::var("HOME").unwrap())]
    pub home: String,

    /// default="deb"; override pkgmgr
    #[arg(short = 'M', long)]
    pub pkgmgr: Option<String>,

    /// specify list of glob patterns to match links
    #[arg(short = 'l', long, value_name = "LINK", num_args = 0.., help = "specify list of glob patterns to match links")]
    pub link: Option<Vec<String>>,

    /// specify list of glob patterns to match ppa items
    #[arg(short = 'p', long, value_name = "PPA", num_args = 0..)]
    pub ppa: Option<Vec<String>>,

    /// specify list of glob patterns to match apt items
    #[arg(short = 'a', long, value_name = "APT", num_args = 0..)]
    pub apt: Option<Vec<String>>,

    /// specify list of glob patterns to match dnf items
    #[arg(short = 'd', long, value_name = "DNF", num_args = 0..)]
    pub dnf: Option<Vec<String>>,

    /// specify list of glob patterns to match npm items
    #[arg(short = 'n', long, value_name = "NPM", num_args = 0..)]
    pub npm: Option<Vec<String>>,

    /// specify list of glob patterns to match pip3 items
    #[arg(short = 'P', long = "pip3", value_name = "PIP3", num_args = 0..)]
    pub pip3: Option<Vec<String>>,

    /// specify list of glob patterns to match pipx items
    #[arg(short = 'x', long, value_name = "PIPX", num_args = 0..)]
    pub pipx: Option<Vec<String>>,

    /// specify list of glob patterns to match flatpak items
    #[arg(short = 'f', long, value_name = "FLATPAK", num_args = 0..)]
    pub flatpak: Option<Vec<String>>,

    /// specify list of glob patterns to match cargo crate names
    #[arg(short = 'c', long, value_name = "CARGO", num_args = 0..)]
    pub cargo: Option<Vec<String>>,

    /// specify list of glob patterns to match github repos
    #[arg(short = 'g', long, value_name = "GITHUB", num_args = 0..)]
    pub github: Option<Vec<String>>,

    /// specify list of glob patterns to match script names
    #[arg(short = 's', long, value_name = "SCRIPT", num_args = 0..)]
    pub script: Option<Vec<String>>,
}

