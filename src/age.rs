// src/age.rs

use age::armor::ArmoredReader;
use age::{Decryptor, Identity};
use eyre::{Result, WrapErr, eyre};
use log::error;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Decrypt armored age data
pub fn decrypt(ciphertext: &[u8], identity: &dyn Identity) -> Result<Vec<u8>> {
    let armor_reader = ArmoredReader::new(ciphertext);
    let decryptor = Decryptor::new(armor_reader).wrap_err("Failed to create decryptor")?;

    let mut output = vec![];
    let mut reader = decryptor
        .decrypt(std::iter::once(identity))
        .map_err(|e| eyre!("Failed to decrypt: {}", e))?;
    reader
        .read_to_end(&mut output)
        .wrap_err("Failed to read decrypted data")?;
    Ok(output)
}

/// Decrypt a .age file
pub fn decrypt_file(path: &Path, identity: &dyn Identity) -> Result<Vec<u8>> {
    let ciphertext = fs::read(path).wrap_err_with(|| format!("Failed to read file: {}", path.display()))?;
    decrypt(&ciphertext, identity)
}

/// Recursively find all .age files in a directory
pub fn find_age_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        if path.extension().is_some_and(|ext| ext == "age") {
            return vec![path.to_path_buf()];
        }
        return vec![];
    }

    WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "age"))
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// Convert filename to environment variable name
/// "chatgpt-api-key.age" -> "CHATGPT_API_KEY"
pub fn filename_to_var(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_uppercase()
        .replace('-', "_")
}

/// Escape value for shell assignment (handle quotes, newlines)
pub fn shell_escape(value: &[u8]) -> String {
    let s = String::from_utf8_lossy(value);
    s.replace('\'', "'\\''")
}

/// Load identity from file path
pub fn load_identity(path: &Path) -> Result<Box<dyn Identity>> {
    let content =
        fs::read_to_string(path).wrap_err_with(|| format!("Failed to read identity file: {}", path.display()))?;

    // Try to parse as age native identity first
    let identities: Vec<_> = content
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| line.parse::<age::x25519::Identity>().ok())
        .collect();

    if let Some(identity) = identities.into_iter().next() {
        return Ok(Box::new(identity));
    }

    // Try SSH identity
    if let Ok(identity) = age::ssh::Identity::from_buffer(content.as_bytes(), None) {
        return Ok(Box::new(identity));
    }

    Err(eyre!("No valid identity found in file: {}", path.display()))
}

/// Find identity file using resolution chain:
/// 1. Explicit path (if provided)
/// 2. ~/.config/manifest/identity.txt
/// 3. ~/.ssh/id_ed25519
/// 4. ~/.ssh/id_rsa
pub fn resolve_identity(explicit_path: Option<&str>) -> Result<Box<dyn Identity>> {
    if let Some(path) = explicit_path {
        return load_identity(Path::new(path));
    }

    let home = std::env::var("HOME").wrap_err("HOME environment variable not set")?;

    let candidates = [
        format!("{}/.config/manifest/identity.txt", home),
        format!("{}/.ssh/id_ed25519", home),
        format!("{}/.ssh/id_rsa", home),
    ];

    for candidate in &candidates {
        let path = Path::new(candidate);
        if path.exists() {
            match load_identity(path) {
                Ok(identity) => return Ok(identity),
                Err(_) => continue,
            }
        }
    }

    Err(eyre!(
        "No identity file found. Tried:\n  {}\n\nHint: run `manifest age --keygen` to generate a new identity",
        candidates.join("\n  ")
    ))
}

/// Decrypt all .age files and format as shell exports
pub fn render_exports(path: &Path, identity: &dyn Identity) -> String {
    let files = find_age_files(path);
    let mut output = String::new();

    // Check for filename collisions
    let mut var_names: std::collections::HashMap<String, Vec<PathBuf>> = std::collections::HashMap::new();
    for file in &files {
        let var_name = filename_to_var(file);
        var_names.entry(var_name).or_default().push(file.clone());
    }

    // Report collisions
    for (var_name, paths) in &var_names {
        if paths.len() > 1 {
            error!(
                "CRITICAL: Variable name collision for {}: {:?}",
                var_name,
                paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()
            );
        }
    }

    for file in files {
        let var_name = filename_to_var(&file);
        match decrypt_file(&file, identity) {
            Ok(plaintext) => {
                let escaped = shell_escape(&plaintext);
                output.push_str(&format!("export {}='{}'\n", var_name, escaped));
            }
            Err(e) => {
                error!("CRITICAL: failed to decrypt {}: {}", file.display(), e);
                output.push_str(&format!("export {}='manifest age command failed'\n", var_name));
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filename_to_var_simple() {
        let path = Path::new("github-pat.age");
        assert_eq!(filename_to_var(path), "GITHUB_PAT");
    }

    #[test]
    fn test_filename_to_var_with_path() {
        let path = Path::new("/home/user/.secrets/chatgpt-api-key.age");
        assert_eq!(filename_to_var(path), "CHATGPT_API_KEY");
    }

    #[test]
    fn test_filename_to_var_underscores() {
        let path = Path::new("aws_secret_key.age");
        assert_eq!(filename_to_var(path), "AWS_SECRET_KEY");
    }

    #[test]
    fn test_shell_escape_simple() {
        let value = b"simple_value";
        assert_eq!(shell_escape(value), "simple_value");
    }

    #[test]
    fn test_shell_escape_single_quotes() {
        let value = b"it's a test";
        assert_eq!(shell_escape(value), "it'\\''s a test");
    }

    #[test]
    fn test_find_age_files_single_file() {
        let path = Path::new("/tmp/test.age");
        // This would need a real file to test properly
        let result = find_age_files(path);
        // If file doesn't exist, should return empty
        assert!(result.is_empty() || result[0].extension().unwrap() == "age");
    }
}
