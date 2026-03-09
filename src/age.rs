// src/age.rs

use age::armor::{ArmoredReader, ArmoredWriter, Format};
use age::secrecy::ExposeSecret;
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

/// Convert environment variable name to filename
/// "CHATGPT_API_KEY" -> "chatgpt-api-key.age"
pub fn var_to_filename(var_name: &str) -> String {
    format!("{}.age", var_name.to_lowercase().replace('_', "-"))
}

/// Escape value for env file format (systemd EnvironmentFile compatible)
/// Values containing whitespace, #, ", ', or \ are double-quoted with escaping
pub fn env_escape(value: &[u8]) -> String {
    // Strip trailing newline if present
    let value = if value.ends_with(b"\n") { &value[..value.len() - 1] } else { value };

    let s = match std::str::from_utf8(value) {
        Ok(s) => s,
        Err(_) => {
            // Non-UTF8: hex encode, always quote
            let hex: String = value.iter().map(|b| format!("\\x{:02x}", b)).collect();
            return format!("\"{}\"", hex);
        }
    };

    let needs_quoting = s.starts_with('#')
        || s.starts_with(';')
        || s.chars()
            .any(|c| c.is_whitespace() || matches!(c, '#' | '"' | '\'' | '\\' | '\n'));

    if !needs_quoting {
        return s.to_string();
    }

    let mut escaped = String::with_capacity(s.len() + 2);
    escaped.push('"');
    for c in s.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            c => escaped.push(c),
        }
    }
    escaped.push('"');
    escaped
}

/// Escape value for shell assignment (handle quotes, newlines, special chars)
/// Uses ANSI-C quoting ($'...') for values with special characters
pub fn shell_escape(value: &[u8]) -> String {
    // Strip trailing newline if present (common for single-value secrets)
    let value = if value.ends_with(b"\n") { &value[..value.len() - 1] } else { value };

    // Check if valid UTF-8
    let s = match std::str::from_utf8(value) {
        Ok(s) => s,
        Err(_) => {
            // Non-UTF8: encode as hex
            let hex: String = value.iter().map(|b| format!("\\x{:02x}", b)).collect();
            return format!("$'{}'", hex);
        }
    };

    // Check if we need special escaping
    let needs_escape = s.chars().any(|c| matches!(c, '\'' | '\\' | '\n' | '\r' | '\t' | '\0'));

    if needs_escape {
        // Use ANSI-C quoting
        let escaped: String = s
            .chars()
            .map(|c| match c {
                '\'' => "\\'".to_string(),
                '\\' => "\\\\".to_string(),
                '\n' => "\\n".to_string(),
                '\r' => "\\r".to_string(),
                '\t' => "\\t".to_string(),
                '\0' => "\\0".to_string(),
                c => c.to_string(),
            })
            .collect();
        format!("$'{}'", escaped)
    } else {
        // Simple single-quoted string
        s.to_string()
    }
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

// ============ IDENTITY MANAGEMENT ============

/// Generate a new age identity and save to default location
pub fn generate_identity() -> Result<String> {
    let home = std::env::var("HOME").wrap_err("HOME environment variable not set")?;
    let identity_dir = format!("{}/.config/manifest", home);
    let identity_path = format!("{}/identity.txt", identity_dir);

    // Check if identity already exists
    if Path::new(&identity_path).exists() {
        return Err(eyre!(
            "Identity file already exists: {}\nTo generate a new one, first remove or rename the existing file.",
            identity_path
        ));
    }

    // Create directory if needed
    fs::create_dir_all(&identity_dir).wrap_err_with(|| format!("Failed to create directory: {}", identity_dir))?;

    // Generate identity
    let identity = age::x25519::Identity::generate();
    let public_key = identity.to_public().to_string();

    // Write identity file with public key comment
    let content = format!(
        "# created: {}\n# public key: {}\n{}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        public_key,
        identity.to_string().expose_secret()
    );

    fs::write(&identity_path, content).wrap_err_with(|| format!("Failed to write identity: {}", identity_path))?;

    // Set restrictive permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        fs::set_permissions(&identity_path, perms)?;
    }

    Ok(format!(
        "Identity saved to: {}\nPublic key: {}",
        identity_path, public_key
    ))
}

/// Get public key from an identity
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
                // shell_escape returns either a plain string (for simple values)
                // or $'...' format (for values with special chars)
                if escaped.starts_with("$'") {
                    output.push_str(&format!("export {}={}\n", var_name, escaped));
                } else {
                    output.push_str(&format!("export {}='{}'\n", var_name, escaped));
                }
            }
            Err(e) => {
                error!("CRITICAL: failed to decrypt {}: {}", file.display(), e);
                output.push_str(&format!("export {}='manifest age command failed'\n", var_name));
            }
        }
    }
    output
}

/// Decrypt all .age files and format as env file (KEY=val, no export prefix)
pub fn render_env(path: &Path, identity: &dyn Identity) -> String {
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
                let escaped = env_escape(&plaintext);
                output.push_str(&format!("{}={}\n", var_name, escaped));
            }
            Err(e) => {
                error!("CRITICAL: failed to decrypt {}: {}", file.display(), e);
                output.push_str(&format!("{}=\"manifest age command failed\"\n", var_name));
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
        assert_eq!(shell_escape(value), "$'it\\'s a test'");
    }

    #[test]
    fn test_shell_escape_newline() {
        let value = b"line1\nline2";
        assert_eq!(shell_escape(value), "$'line1\\nline2'");
    }

    #[test]
    fn test_shell_escape_trailing_newline() {
        // Trailing newlines should be stripped
        let value = b"secret\n";
        assert_eq!(shell_escape(value), "secret");
    }

    #[test]
    fn test_shell_escape_backslash() {
        let value = b"path\\to\\file";
        assert_eq!(shell_escape(value), "$'path\\\\to\\\\file'");
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

    // var_to_filename tests
    #[test]
    fn test_var_to_filename_simple() {
        assert_eq!(var_to_filename("GITHUB_PAT"), "github-pat.age");
    }

    #[test]
    fn test_var_to_filename_multi_word() {
        assert_eq!(var_to_filename("CHATGPT_API_KEY"), "chatgpt-api-key.age");
    }

    #[test]
    fn test_var_to_filename_roundtrip() {
        let var_name = "FOO_BAR";
        let filename = var_to_filename(var_name);
        assert_eq!(filename_to_var(Path::new(&filename)), var_name);
    }

    // env_escape tests
    #[test]
    fn test_env_escape_simple() {
        assert_eq!(env_escape(b"simple_value"), "simple_value");
    }

    #[test]
    fn test_env_escape_with_spaces() {
        assert_eq!(env_escape(b"value with spaces"), "\"value with spaces\"");
    }

    #[test]
    fn test_env_escape_with_hash() {
        assert_eq!(env_escape(b"#comment-like"), "\"#comment-like\"");
    }

    #[test]
    fn test_env_escape_with_quotes() {
        assert_eq!(env_escape(b"it's a test"), "\"it's a test\"");
    }

    #[test]
    fn test_env_escape_trailing_newline() {
        assert_eq!(env_escape(b"secret\n"), "secret");
    }

    #[test]
    fn test_env_escape_embedded_newline() {
        assert_eq!(env_escape(b"line1\nline2"), "\"line1\\nline2\"");
    }

    #[test]
    fn test_env_escape_semicolon_start() {
        assert_eq!(env_escape(b";value"), "\";value\"");
    }

    #[test]
    fn test_render_env_roundtrip() {
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public();

        let tmp = tempfile::TempDir::new().unwrap();
        let secret_path = tmp.path().join("api-key.age");

        let ciphertext = encrypt(b"sk-test123", &recipient).unwrap();
        std::fs::write(&secret_path, ciphertext).unwrap();

        let output = render_env(tmp.path(), &identity);
        assert_eq!(output, "API_KEY=sk-test123\n");
    }

    #[test]
    fn test_render_exports_roundtrip() {
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public();

        let tmp = tempfile::TempDir::new().unwrap();
        let secret_path = tmp.path().join("api-key.age");

        let ciphertext = encrypt(b"sk-test123", &recipient).unwrap();
        std::fs::write(&secret_path, ciphertext).unwrap();

        let output = render_exports(tmp.path(), &identity);
        assert_eq!(output, "export API_KEY='sk-test123'\n");
    }

    #[test]
    fn test_env_format_value_with_equals() {
        // Values containing = should work fine in env format
        assert_eq!(env_escape(b"sk-ant=xxx=yyy"), "sk-ant=xxx=yyy");
    }

    #[test]
    fn test_env_escape_double_quote() {
        assert_eq!(env_escape(b"say \"hello\""), "\"say \\\"hello\\\"\"");
    }

    #[test]
    fn test_var_to_filename_single_word() {
        assert_eq!(var_to_filename("TOKEN"), "token.age");
    }

    #[test]
    fn test_encrypt_kv_roundtrip() {
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public();

        // Simulate KEY=VAL encrypt: encrypt value, write to var_to_filename
        let key = "GITHUB_PAT";
        let val = b"ghp_xxxx";
        let filename = var_to_filename(key);
        assert_eq!(filename, "github-pat.age");

        let tmp = tempfile::TempDir::new().unwrap();
        let out_path = tmp.path().join(&filename);

        let ciphertext = encrypt(val, &recipient).unwrap();
        std::fs::write(&out_path, ciphertext).unwrap();

        // Decrypt and verify roundtrip
        let output = render_exports(tmp.path(), &identity);
        assert_eq!(output, "export GITHUB_PAT='ghp_xxxx'\n");

        let env_output = render_env(tmp.path(), &identity);
        assert_eq!(env_output, "GITHUB_PAT=ghp_xxxx\n");
    }

    #[test]
    fn test_encrypt_kv_value_with_equals() {
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public();

        // Value contains = (split on first = only)
        let input = "API_KEY=sk-ant=xxx=yyy";
        let (key, val) = input.split_once('=').unwrap();
        assert_eq!(key, "API_KEY");
        assert_eq!(val, "sk-ant=xxx=yyy");

        let tmp = tempfile::TempDir::new().unwrap();
        let filename = var_to_filename(key);
        let ciphertext = encrypt(val.as_bytes(), &recipient).unwrap();
        std::fs::write(tmp.path().join(&filename), ciphertext).unwrap();

        let output = render_env(tmp.path(), &identity);
        assert_eq!(output, "API_KEY=sk-ant=xxx=yyy\n");
    }

    #[test]
    fn test_encrypt_kv_empty_value() {
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public();

        let tmp = tempfile::TempDir::new().unwrap();
        let ciphertext = encrypt(b"", &recipient).unwrap();
        std::fs::write(tmp.path().join("empty-var.age"), ciphertext).unwrap();

        let output = render_exports(tmp.path(), &identity);
        assert_eq!(output, "export EMPTY_VAR=''\n");
    }
}
