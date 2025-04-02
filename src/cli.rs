// src/cli.rs

use clap::{ArgAction, Parser};

/// A single struct using Clap Derive, closely matching the Python argparse approach.
///
/// Behavior of each argument:
/// - If the user runs `--link` with no arguments => `link=["*"]`.
/// - If the user omits `--link` => `link=[]`.
/// - If the user types `--link foo bar` => `link=["foo","bar"]`.
///
/// And so on for the other sub-commands (`ppa`, `apt`, `dnf`, `npm`, `pip3`, `pipx`,
/// `flatpak`, `cargo`, `github`, and `script`).
#[derive(Debug, Parser)]
#[command(
    name = "manifest",
    version,
    about = "Generate a Bash script from a YAML manifest describing your config."
)]
pub struct Cli {
    /// The path to the YAML config file
    #[arg(
        short = 'C',
        long = "config",
        default_value = "manifest.yml",
        help = "Path to the manifest YAML file"
    )]
    pub config: String,

    /// The path to treat as CWD
    #[arg(
        short = 'D',
        long = "cwd",
        default_value = ".",
        help = "Set the working directory"
    )]
    pub cwd: String,

    /// The HOME directory override
    #[arg(
        short = 'H',
        long = "home",
        default_value = "",
        help = "Specify HOME if not current"
    )]
    pub home: String,

    /// Package manager override; e.g. 'deb', 'rpm', or 'brew'
    #[arg(
        short = 'M',
        long = "pkgmgr",
        default_value = "",
        help = "Override package manager; e.g. 'deb', 'rpm', 'brew'"
    )]
    pub pkgmgr: String,

    /// If the user runs `--link` with zero arguments => link=["*"], otherwise
    /// specify patterns like `--link foo bar`
    #[arg(
        short = 'l',
        long = "link",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match links"
    )]
    pub link: Vec<String>,

    #[arg(
        short = 'p',
        long = "ppa",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match ppa items"
    )]
    pub ppa: Vec<String>,

    #[arg(
        short = 'a',
        long = "apt",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match apt items"
    )]
    pub apt: Vec<String>,

    #[arg(
        short = 'd',
        long = "dnf",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match dnf items"
    )]
    pub dnf: Vec<String>,

    #[arg(
        short = 'n',
        long = "npm",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match npm items"
    )]
    pub npm: Vec<String>,

    #[arg(
        short = 'P',
        long = "pip3",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match pip3 items"
    )]
    pub pip3: Vec<String>,

    #[arg(
        short = 'x',
        long = "pipx",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match pipx items"
    )]
    pub pipx: Vec<String>,

    #[arg(
        short = 'f',
        long = "flatpak",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match flatpak items"
    )]
    pub flatpak: Vec<String>,

    #[arg(
        short = 'c',
        long = "cargo",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match cargo crate names"
    )]
    pub cargo: Vec<String>,

    #[arg(
        short = 'g',
        long = "github",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match GitHub repos"
    )]
    pub github: Vec<String>,

    #[arg(
        short = 's',
        long = "script",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match script items"
    )]
    pub script: Vec<String>,
}

impl Cli {
    /// Returns true if any of the sub-commands for partial usage were specified.
    /// If false, that implies "complete" mode.
    pub fn any_section_specified(&self) -> bool {
        !self.link.is_empty()
            || !self.ppa.is_empty()
            || !self.apt.is_empty()
            || !self.dnf.is_empty()
            || !self.npm.is_empty()
            || !self.pip3.is_empty()
            || !self.pipx.is_empty()
            || !self.flatpak.is_empty()
            || !self.cargo.is_empty()
            || !self.github.is_empty()
            || !self.script.is_empty()
    }
}
