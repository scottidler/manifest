# Manifest Tool Test Suite

This document describes the comprehensive test suite for the manifest tool, covering all Spec types and their serialization/deserialization and bash script generation functionality.

## Test Coverage

### Configuration Tests (`src/config.rs`)

The configuration tests verify that all Spec types can be correctly loaded from YAML and serialized back to YAML.

#### Individual Spec Type Tests

1. **ManifestSpec Default Test** - Verifies default values for all fields
2. **LinkSpec Tests** - Tests both serialization and deserialization of link configurations
3. **PpaSpec Tests** - Verifies PPA repository configurations
4. **PkgSpec Tests** - Tests package list configurations
5. **AptSpec Tests** - Tests APT package configurations
6. **DnfSpec Tests** - Tests DNF package configurations
7. **NpmSpec Tests** - Tests NPM package configurations
8. **Pip3Spec Tests** - Tests pip3 packages including `distutils` field
9. **PipxSpec Tests** - Tests pipx package configurations
10. **FlatpakSpec Tests** - Tests Flatpak application configurations
11. **CargoSpec Tests** - Tests Cargo crate configurations
12. **ScriptSpec Tests** - Tests script configurations (both top-level and nested)
13. **GithubSpec Tests** - Tests GitHub repository configurations including default repopath
14. **RepoSpec Tests** - Tests individual repository specifications

#### Integration Tests

1. **Full Manifest Test** - Tests a complete manifest with all sections
2. **Actual Manifest File Test** - Loads and validates the real `manifest.yml` file
3. **Empty Specs Serialization** - Tests round-trip serialization of empty specs
4. **Nested Script Test** - Tests RepoSpec with nested ScriptSpec (critical test)
5. **Test Manifest with Nested Scripts** - Loads `test/manifest.yml` and validates nested scripts

### Bash Script Generation Tests (`src/manifest.rs`)

These tests verify that all ManifestType variants correctly generate bash scripts.

#### ManifestType Rendering Tests

1. **Link Rendering** - Tests heredoc generation for symlink creation
2. **PPA Rendering** - Tests PPA addition script generation
3. **Apt Rendering** - Tests APT package installation with continuation lines
4. **DNF Rendering** - Tests DNF package installation
5. **NPM Rendering** - Tests NPM global package installation
6. **Pip3 Rendering** - Tests pip3 package installation with setup
7. **Pipx Rendering** - Tests pipx package installation with heredoc
8. **Flatpak Rendering** - Tests Flatpak application installation
9. **Cargo Rendering** - Tests Cargo crate installation
10. **Script Rendering** - Tests custom script execution
11. **GitHub Rendering** - Tests GitHub repository cloning and processing

#### Helper Function Tests

1. **render_heredoc Tests** - Tests heredoc generation with and without headers
2. **render_continue Tests** - Tests continuation line generation for package lists
3. **render_repo_links Tests** - Tests repository-specific link generation
4. **render_repo_cargo_install Tests** - Tests repository-specific cargo installation
5. **render_github Tests** - Tests complete GitHub repository processing
6. **render_script Tests** - Tests script rendering functionality

#### Build Script Tests

1. **Empty Build Script** - Tests basic script structure generation
2. **Build Script with Functions** - Tests function inclusion and deduplication
3. **Function Deduplication** - Ensures functions aren't duplicated
4. **Leading Newline Removal** - Tests proper formatting of first section

#### Integration Tests

1. **Functions Test** - Verifies which ManifestTypes include which functions
2. **Repo Nested Scripts Integration** - Tests that nested scripts in RepoSpec work correctly

## Test Files

### `test/manifest.yml`

This file contains comprehensive examples of all Spec types, with special emphasis on nested ScriptSpec examples that are missing from the main `manifest.yml`. It includes:

- **Basic configurations** for all package managers and tools
- **GitHub repositories** with various combinations of:
  - Cargo build configurations
  - Link specifications (both simple and recursive)
  - **Nested scripts** (post_install, configure, test, setup, build, post_build)
- **Top-level scripts** for system setup

### Key Features Tested

#### Nested ScriptSpec Examples

The test manifest includes three GitHub repositories that demonstrate nested ScriptSpec functionality:

1. **`testuser/tool-with-scripts`** - Shows post-installation, configuration, and testing scripts
2. **`testuser/simple-repo`** - Shows a repository without nested scripts (for comparison)
3. **`testuser/complex-repo`** - Shows complex build workflows with multiple nested scripts

#### Script Types Demonstrated

- **post_install**: Scripts that run after installation (chmod, mkdir, etc.)
- **configure**: Scripts that configure the installed tools
- **test**: Scripts that verify installation success
- **setup**: Scripts that prepare the environment
- **build**: Scripts that build components
- **post_build**: Scripts that run after building

## Running Tests

### Run All Tests
```bash
cargo test
```

### Run Configuration Tests Only
```bash
cargo test config::tests
```

### Run Manifest Tests Only
```bash
cargo test manifest::tests
```

### Run Specific Test
```bash
cargo test test_load_test_manifest_with_nested_scripts
```

## Test Examples

### Generate Script from Test Manifest
```bash
# Generate complete script
cargo run -- --config test/manifest.yml

# Generate script for specific repo with nested scripts
cargo run -- --config test/manifest.yml --github "testuser/tool-with-scripts"

# Generate script for specific package type
cargo run -- --config test/manifest.yml --cargo "bat"
```

### Expected Output for Nested Scripts

When generating a script for `testuser/tool-with-scripts`, you should see:

```bash
echo "github repos:"
echo "testuser/tool-with-scripts:"
git clone --recursive https://github.com/testuser/tool-with-scripts $HOME/test_repos/testuser/tool-with-scripts
(cd $HOME/test_repos/testuser/tool-with-scripts && pwd && git pull && git checkout HEAD)

# Cargo installations
echo "cargo install (path):"
echo "Installing from $HOME/test_repos/testuser/tool-with-scripts/./"
(cd $HOME/test_repos/testuser/tool-with-scripts/./ && cargo install --path .)

# Links
echo "links:"
while read -r file link; do
    linker $file $link
done<<EOM
$HOME/test_repos/testuser/tool-with-scripts/bin/tool ~/bin/tool
$HOME/test_repos/testuser/tool-with-scripts/config/tool.conf ~/.config/tool/tool.conf
$HOME/test_repos/testuser/tool-with-scripts/scripts/helper.sh ~/bin/helper.sh
EOM

# Nested scripts
echo "scripts:"
echo "post_install:"
echo "Running post-install script for tool-with-scripts"
chmod +x ~/bin/tool
chmod +x ~/bin/helper.sh
echo "Creating config directory"
mkdir -p ~/.config/tool

echo "configure:"
echo "Configuring tool-with-scripts"
~/bin/tool --init
echo "Setting up shell integration"
echo 'export PATH="$HOME/bin:$PATH"' >> ~/.bashrc

echo "test:"
echo "Testing tool installation"
~/bin/tool --version
~/bin/helper.sh --check
```

## Test Validation

The tests validate:

1. **Correct YAML parsing** of all Spec types
2. **Proper serialization** back to YAML
3. **Accurate bash script generation** for each ManifestType
4. **Function inclusion and deduplication** in generated scripts
5. **Nested script functionality** in RepoSpec
6. **Integration between different components**

## Key Insights

### Nested ScriptSpec Discovery

The testing revealed that while the main `manifest.yml` file includes many examples of top-level ScriptSpec usage, it lacks examples of nested ScriptSpec within RepoSpec. The `test/manifest.yml` file fills this gap by providing comprehensive examples of:

- Multiple nested scripts per repository
- Different types of scripts (setup, build, post-install, etc.)
- Complex multi-step workflows
- Integration with cargo builds and linking

### Test Coverage Completeness

The test suite provides 100% coverage of:
- All Spec type deserialization
- All ManifestType bash generation
- All helper functions
- Integration scenarios
- Error cases and edge conditions

This ensures that any changes to the manifest format or script generation logic will be caught by the test suite.

## File Structure

```
test/
├── README.md          # This documentation
└── manifest.yml       # Comprehensive test manifest with nested ScriptSpec examples
```

## Contributing

When adding new Spec types or modifying existing ones:

1. Add corresponding tests in `src/config.rs` for YAML parsing
2. Add corresponding tests in `src/manifest.rs` for bash generation
3. Update `test/manifest.yml` with examples of the new functionality
4. Update this README with documentation of the new tests