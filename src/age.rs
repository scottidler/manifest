// src/age.rs

use age::armor::{ArmoredReader, ArmoredWriter, Format};
use age::{Decryptor, Encryptor, Identity, Recipient};
use eyre::{Result, WrapErr, eyre};
use log::error;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

// ============ ENCRYPTION ============

/// Encrypt data to armored age format
pub fn encrypt(plaintext: &[u8], recipient: &dyn Recipient) -> Result<Vec<u8>> {
    let encryptor = Encryptor::with_recipients(std::iter::once(recipient))
        .map_err(|_| eyre!("Failed to create encryptor: no recipients provided"))?;
    let mut output = vec![];
    let armor_writer = ArmoredWriter::wrap_output(&mut output, Format::AsciiArmor)?;
    let mut writer = encryptor.wrap_output(armor_writer)?;
    writer.write_all(plaintext)?;
    writer.finish()?.finish()?;
    Ok(output)
}

/// Encrypt a file, return armored ciphertext
pub fn encrypt_file(path: &Path, recipient: &dyn Recipient) -> Result<Vec<u8>> {
    let plaintext =
        fs::read(path).wrap_err_with(|| format!("Failed to read file for encryption: {}", path.display()))?;
    encrypt(&plaintext, recipient)
}

/// Encrypt from stdin
pub fn encrypt_stdin(recipient: &dyn Recipient) -> Result<Vec<u8>> {
    let mut plaintext = Vec::new();
    std::io::stdin()
        .read_to_end(&mut plaintext)
        .wrap_err("Failed to read from stdin")?;
    encrypt(&plaintext, recipient)
}

// ============ DECRYPTION ============

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

/// Load recipient (public key) from a public key string
pub fn parse_recipient(public_key: &str) -> Result<Box<dyn Recipient + Send>> {
    // Try age native public key
    if let Ok(recipient) = public_key.parse::<age::x25519::Recipient>() {
        return Ok(Box::new(recipient));
    }

    // Try SSH public key
    if let Ok(recipient) = public_key.parse::<age::ssh::Recipient>() {
        return Ok(Box::new(recipient));
    }

    Err(eyre!(
        "Invalid public key format: {}. Expected age or SSH public key.",
        public_key
    ))
}

/// Get public key from an identity (used in Phase 3 --public-key)
#[allow(dead_code)]
pub fn get_public_key(identity_path: &Path) -> Result<String> {
    let content = fs::read_to_string(identity_path)
        .wrap_err_with(|| format!("Failed to read identity: {}", identity_path.display()))?;

    // Try age native identity
    for line in content.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Ok(identity) = line.parse::<age::x25519::Identity>() {
            return Ok(identity.to_public().to_string());
        }
    }

    // For SSH keys, we need to read the .pub file
    let pub_path = identity_path.with_extension("pub");
    if pub_path.exists() {
        let pub_content = fs::read_to_string(&pub_path)?;
        return Ok(pub_content.trim().to_string());
    }

    Err(eyre!(
        "Could not extract public key from identity: {}",
        identity_path.display()
    ))
}

/// Resolve recipient for encryption
/// 1. Explicit -r/--recipient argument (public key string)
/// 2. Public key derived from identity file
pub fn resolve_recipient(
    explicit_recipient: Option<&str>,
    identity_path: Option<&str>,
) -> Result<Box<dyn Recipient + Send>> {
    // If explicit recipient provided, use it
    if let Some(pubkey) = explicit_recipient {
        return parse_recipient(pubkey);
    }

    // Otherwise, derive from identity
    let identity_path = if let Some(path) = identity_path {
        PathBuf::from(path)
    } else {
        // Use default identity path
        let home = std::env::var("HOME").wrap_err("HOME environment variable not set")?;
        let candidates = [
            format!("{}/.config/manifest/identity.txt", home),
            format!("{}/.ssh/id_ed25519", home),
            format!("{}/.ssh/id_rsa", home),
        ];

        candidates
            .iter()
            .find(|p| Path::new(p).exists())
            .map(PathBuf::from)
            .ok_or_else(|| eyre!("No identity file found for deriving public key"))?
    };

    // Read identity to get public key
    let content = fs::read_to_string(&identity_path)?;

    // Try age native identity
    for line in content.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Ok(identity) = line.parse::<age::x25519::Identity>() {
            return Ok(Box::new(identity.to_public()));
        }
    }

    // Try SSH - need the .pub file
    let pub_path = identity_path.with_extension("pub");
    if pub_path.exists() {
        let pub_content = fs::read_to_string(&pub_path)?;
        return parse_recipient(pub_content.trim());
    }

    Err(eyre!(
        "Could not derive recipient from identity: {}",
        identity_path.display()
    ))
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

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        // Generate a test identity
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public();

        let plaintext = b"test secret value";

        // Encrypt
        let ciphertext = encrypt(plaintext, &recipient).unwrap();

        // Verify it's armored
        let ciphertext_str = String::from_utf8_lossy(&ciphertext);
        assert!(ciphertext_str.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"));

        // Decrypt
        let decrypted = decrypt(&ciphertext, &identity).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_parse_recipient_age_key() {
        // Valid age public key format
        let identity = age::x25519::Identity::generate();
        let pubkey = identity.to_public().to_string();

        let result = parse_recipient(&pubkey);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_recipient_invalid() {
        let result = parse_recipient("invalid-key");
        assert!(result.is_err());
    }
}
