// src/cli.rs

use clap::{ArgAction, Parser, Subcommand};
use log::{debug, error, warn};
use std::process::Command;

fn check_hash(program: &str) -> bool {
    debug!("check_hash: checking for program {}", program);
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {}", program))
        .output();
    match output {
        Ok(o) => {
            let found = !o.stdout.is_empty();
            debug!("check_hash: program {} found = {}", program, found);
            found
        }
        Err(e) => {
            warn!("check_hash: error checking {}: {}", program, e);
            false
        }
    }
}

fn get_pkgmgr() -> String {
    if check_hash("dpkg") {
        debug!("get_pkgmgr: detected dpkg");
        "deb".to_string()
    } else if check_hash("rpm") {
        debug!("get_pkgmgr: detected rpm");
        "rpm".to_string()
    } else if check_hash("brew") {
        debug!("get_pkgmgr: detected brew");
        "brew".to_string()
    } else {
        error!("unknown pkg mgr");
        "unknown".to_string()
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "manifest",
    version = env!("GIT_DESCRIBE"),
    about = "Generate a Bash script from a YAML manifest describing your config.",
    after_help = "Logs are written to: ~/.local/share/manifest/logs/manifest.log"
)]
pub struct Cli {
    #[arg(
        short = 'C',
        long = "config",
        default_value = "manifest.yml",
        help = "Path to the manifest YAML file"
    )]
    pub config: String,

    #[arg(short = 'H', long = "home", default_value = "", help = "Specify HOME if not current")]
    pub home: String,

    #[arg(
        short = 'M',
        long = "pkgmgr",
        default_value = "",
        help = "Override package manager; e.g. 'deb', 'rpm', 'brew'",
        default_value_t = get_pkgmgr()
    )]
    pub pkgmgr: String,

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
        short = 'G',
        long = "git-crypt",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        value_name = "GITHUB",
        help = "Specify list of glob patterns to match git-crypt repos"
    )]
    pub git_crypt: Vec<String>,

    #[arg(
        short = 's',
        long = "script",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match script items"
    )]
    pub script: Vec<String>,

    #[arg(
        value_name = "PATH",
        default_value = ".",
        help = "Optional positional path to operate on; defaults to the current working directory"
    )]
    pub path: String,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Encrypt and decrypt secrets using age encryption
    Age {
        /// Encrypt a file (output to stdout)
        #[arg(short = 'e', long = "encrypt", value_name = "FILE")]
        encrypt: Option<String>,

        /// Decrypt .age files in PATH, output shell exports
        #[arg(short = 'd', long = "decrypt", value_name = "PATH", default_missing_value = ".", num_args = 0..=1)]
        decrypt: Option<String>,

        /// Identity file for encryption/decryption
        #[arg(short = 'i', long = "identity", value_name = "FILE")]
        identity: Option<String>,

        /// Recipient public key for encryption (alternative to identity)
        #[arg(short = 'r', long = "recipient", value_name = "KEY")]
        recipient: Option<String>,

        /// Generate a new age identity
        #[arg(long = "keygen")]
        keygen: bool,

        /// Show public key from identity
        #[arg(long = "public-key")]
        public_key: bool,
    },
}

impl Cli {
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
            || !self.git_crypt.is_empty()
            || !self.script.is_empty()
    }
}
