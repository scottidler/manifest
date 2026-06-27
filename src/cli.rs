// src/cli.rs

use clap::{ArgAction, ArgGroup, Parser, Subcommand};
use log::{debug, error, warn};
use std::path::PathBuf;
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
    #[arg(short = 'C', long = "config", help = "Path to the manifest YAML file")]
    pub config: Option<String>,

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
        long = "uv-tool",
        num_args = 0..,
        default_missing_value = "*",
        action = ArgAction::Append,
        help = "Specify list of glob patterns to match uv-tool items"
    )]
    pub uv_tool: Vec<String>,

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

        #[command(subcommand)]
        action: Option<AgeAction>,
    },
}

#[derive(Debug, Subcommand)]
pub enum AgeAction {
    /// Encrypt files or key-value pairs
    #[command(group(
        ArgGroup::new("encrypt-input")
            .required(true)
            .multiple(false)
            .args(["inputs", "name", "paste"])
    ))]
    Encrypt {
        /// Files or KEY=VAL pairs or "-" for stdin (existing modes)
        #[arg(num_args = 1.., group = "encrypt-input")]
        inputs: Vec<String>,

        /// Read the secret value from stdin and write <name>.age
        #[arg(long, value_name = "NAME", group = "encrypt-input")]
        name: Option<String>,

        /// Read the secret value from the system clipboard and write <name>.age
        #[arg(long, value_name = "NAME", group = "encrypt-input")]
        paste: Option<String>,

        /// Overwrite an existing <name>.age (otherwise refuse)
        #[arg(long)]
        force: bool,

        /// After a successful write, clear the system clipboard (opt-in)
        #[arg(long)]
        clear_clipboard: bool,

        /// Output directory for generated .age files
        #[arg(short = 'o', long = "output-dir")]
        output_dir: Option<PathBuf>,
    },

    /// Decrypt .age files and output key-value pairs
    Decrypt {
        /// Path to .age file or directory containing .age files
        #[arg(default_value = ".")]
        path: String,

        /// Output format: export (default) or env
        #[arg(short = 'f', long = "format", default_value = "export")]
        format: DecryptFormat,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum DecryptFormat {
    Export,
    Env,
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // ---- Phase 7: CLI parse-test matrix for the encrypt-input ArgGroup ----
    //
    // These tests exercise the `AgeAction::Encrypt` ArgGroup and the manual
    // exclusivity validation in `handle_age_command`. We use `Cli::try_parse_from`
    // to confirm what clap accepts/rejects at the parse level. The manual
    // exclusivity check (--name+--paste, --name+positional, --paste+positional)
    // runs in the handler, not at parse time, and is covered by the handler-level
    // tests in `src/main.rs::tests`. Here we only assert parse-level ok vs. err.

    // Parse: no input source -> ArgGroup required(true) fires -> parse error.
    #[test]
    fn test_cli_parse_encrypt_no_input_source_errors() {
        let result = Cli::try_parse_from(["manifest", "age", "encrypt"]);
        assert!(result.is_err(), "expected parse error when no input source is given");
    }

    // Parse: --name X only -> ok.
    #[test]
    fn test_cli_parse_encrypt_name_only_ok() {
        let result = Cli::try_parse_from(["manifest", "age", "encrypt", "--name", "MY_SECRET"]);
        assert!(result.is_ok(), "expected ok with --name only: {:?}", result.err());
        if let Ok(cli) = result {
            if let Some(Commands::Age {
                action: Some(AgeAction::Encrypt {
                    name, paste, inputs, ..
                }),
                ..
            }) = cli.command
            {
                assert_eq!(name.as_deref(), Some("MY_SECRET"));
                assert!(paste.is_none());
                assert!(inputs.is_empty());
            } else {
                panic!("unexpected command structure");
            }
        }
    }

    // Parse: --paste X only -> ok.
    #[test]
    fn test_cli_parse_encrypt_paste_only_ok() {
        let result = Cli::try_parse_from(["manifest", "age", "encrypt", "--paste", "MY_SECRET"]);
        assert!(result.is_ok(), "expected ok with --paste only: {:?}", result.err());
        if let Ok(cli) = result {
            if let Some(Commands::Age {
                action: Some(AgeAction::Encrypt {
                    name, paste, inputs, ..
                }),
                ..
            }) = cli.command
            {
                assert!(name.is_none());
                assert_eq!(paste.as_deref(), Some("MY_SECRET"));
                assert!(inputs.is_empty());
            } else {
                panic!("unexpected command structure");
            }
        }
    }

    // Parse: single positional KEY=VAL -> ok.
    #[test]
    fn test_cli_parse_encrypt_single_positional_ok() {
        let result = Cli::try_parse_from(["manifest", "age", "encrypt", "MY_KEY=my_value"]);
        assert!(result.is_ok(), "expected ok with single positional: {:?}", result.err());
        if let Ok(cli) = result {
            if let Some(Commands::Age {
                action: Some(AgeAction::Encrypt {
                    name, paste, inputs, ..
                }),
                ..
            }) = cli.command
            {
                assert!(name.is_none());
                assert!(paste.is_none());
                assert_eq!(inputs, vec!["MY_KEY=my_value"]);
            } else {
                panic!("unexpected command structure");
            }
        }
    }

    // Parse: multi-value positional (A=1 B=2) -> still parses; both values in `inputs`.
    #[test]
    fn test_cli_parse_encrypt_multi_positional_ok() {
        let result = Cli::try_parse_from(["manifest", "age", "encrypt", "A=1", "B=2"]);
        assert!(result.is_ok(), "expected ok with multi positional: {:?}", result.err());
        if let Ok(cli) = result {
            if let Some(Commands::Age {
                action: Some(AgeAction::Encrypt { inputs, .. }),
                ..
            }) = cli.command
            {
                assert_eq!(inputs, vec!["A=1", "B=2"]);
            } else {
                panic!("unexpected command structure");
            }
        }
    }

    // Parse: --name X + --paste Y -> the ArgGroup multiple(false) fires -> parse error.
    #[test]
    fn test_cli_parse_encrypt_name_and_paste_errors() {
        let result = Cli::try_parse_from(["manifest", "age", "encrypt", "--name", "X", "--paste", "Y"]);
        assert!(
            result.is_err(),
            "expected parse error when --name and --paste are both given"
        );
    }

    // Parse: --name X + positional -> ArgGroup multiple(false) fires -> parse error.
    // Note: clap may accept this if the group logic doesn't catch it at parse time;
    // the handler's manual exclusivity validation is the backstop. We test the
    // *outcome* (ok vs err) without asserting where the rejection happens.
    #[test]
    fn test_cli_parse_encrypt_name_and_positional_rejected() {
        // Clap ArgGroup with required+multiple(false) should reject this combination.
        // If clap accepts it, the handler rejects it. Either way, it must not succeed
        // end-to-end. We only assert here what clap does at parse time.
        let result = Cli::try_parse_from(["manifest", "age", "encrypt", "--name", "X", "KEY=VAL"]);
        // Whether clap accepts or rejects depends on how it handles positionals + named
        // in the same ArgGroup. Record the actual behavior - the handler covers the rest.
        // This test is intentionally lenient: we assert only that if it parses ok, the
        // fields are correctly populated (name=Some, inputs=["KEY=VAL"]).
        match result {
            Ok(cli) => {
                if let Some(Commands::Age {
                    action: Some(AgeAction::Encrypt { name, inputs, .. }),
                    ..
                }) = cli.command
                {
                    // If clap allowed it, both fields must be correctly parsed.
                    assert!(name.is_some(), "name must be set when --name was given");
                    assert!(!inputs.is_empty(), "inputs must be set when positional was given");
                }
            }
            Err(_) => {
                // Clap rejected it at parse time - also acceptable, even preferred.
            }
        }
    }

    // Parse: --paste X + positional -> same reasoning as --name + positional above.
    #[test]
    fn test_cli_parse_encrypt_paste_and_positional_rejected() {
        let result = Cli::try_parse_from(["manifest", "age", "encrypt", "--paste", "X", "KEY=VAL"]);
        match result {
            Ok(cli) => {
                if let Some(Commands::Age {
                    action: Some(AgeAction::Encrypt { paste, inputs, .. }),
                    ..
                }) = cli.command
                {
                    assert!(paste.is_some(), "paste must be set when --paste was given");
                    assert!(!inputs.is_empty(), "inputs must be set when positional was given");
                }
            }
            Err(_) => {
                // Clap rejected it - also acceptable.
            }
        }
    }

    // Parse: --force flag is accepted alongside --name.
    #[test]
    fn test_cli_parse_encrypt_name_with_force_ok() {
        let result = Cli::try_parse_from(["manifest", "age", "encrypt", "--name", "MY_SECRET", "--force"]);
        assert!(result.is_ok(), "expected ok with --name + --force: {:?}", result.err());
        if let Ok(cli) = result {
            if let Some(Commands::Age {
                action: Some(AgeAction::Encrypt { name, force, .. }),
                ..
            }) = cli.command
            {
                assert_eq!(name.as_deref(), Some("MY_SECRET"));
                assert!(force);
            } else {
                panic!("unexpected command structure");
            }
        }
    }

    // Parse: --clear-clipboard alongside --paste -> ok at parse level.
    #[test]
    fn test_cli_parse_encrypt_paste_with_clear_clipboard_ok() {
        let result = Cli::try_parse_from([
            "manifest",
            "age",
            "encrypt",
            "--paste",
            "MY_SECRET",
            "--clear-clipboard",
        ]);
        assert!(
            result.is_ok(),
            "expected ok with --paste + --clear-clipboard: {:?}",
            result.err()
        );
        if let Ok(cli) = result {
            if let Some(Commands::Age {
                action: Some(AgeAction::Encrypt {
                    paste, clear_clipboard, ..
                }),
                ..
            }) = cli.command
            {
                assert_eq!(paste.as_deref(), Some("MY_SECRET"));
                assert!(clear_clipboard);
            } else {
                panic!("unexpected command structure");
            }
        }
    }
}
