# git-releasenotes

A Rust-based tool to generate release notes from git commits since the last tag. This is a rewrite of a shell script to provide cross-platform support (Linux, macOS, Windows) and better performance, without relying on system utilities like `sed` or `awk`.

## Features

- **Automatic Release Notes**: Generates notes based on commits since the last git tag.
- **GitHub Integration**: Fetches Pull Request titles and updates from GitHub to provide richer context.
- **PR Number Support**: Includes PR numbers in commit messages when using the `-p` flag. PR numbers are automatically extracted from commit subjects, merge commits, or via GitHub API search.
- **Clipboard Support**: Optionally copies the generated notes directly to your clipboard.
- **Cross-Platform**: Works on Linux, macOS, and Windows.
- **Dependabot Handling**: Special handling for Dependabot commits to group or format them appropriately. Multiple updates for the same package are consolidated with PR numbers preserved.
- **Major Version Warnings**: Automatically detects and warns about major version changes in dependencies.

## Prerequisites

- **Rust**: You need to have Rust and Cargo installed.
- **Git**: The tool relies on the `git` executable being in your PATH.
- **GitHub Token**: For PR integration, you must set the `GITHUB_TOKEN` environment variable.

## Installation

### From Binary Releases

Pre-compiled binaries for Linux, macOS, and Windows are available on the [GitHub Releases](https://github.com/leif/releasenotes/releases) page.

### From Source

Clone the repository and install with Cargo:

```bash
cargo install --path .
```

## CI/CD

This repository is configured with GitHub Actions to automatically:
- Build and test the project on every push.
- Create release artifacts (Linux, macOS, Windows) when a new tag (e.g., `v1.0.0`) is pushed.


## Usage

Run the tool from within a git repository:

```bash
git-releasenotes [OPTIONS]
```

### Options

| Flag | Description |
|------|-------------|
| `-c` | Copy output to clipboard |
| `-p` | Include PR numbers in output. PR numbers are extracted from commit subjects (e.g., `(#123)`), merge commits, or via GitHub API search. When multiple PRs update the same dependency, all PR numbers are shown in descending order (e.g., `(#300, #200, #100)`) |
| `-x` | List raw commits that form the basis of the output |
| `-X` | Enable debug logging (shows commit count and other debug information) |
| `-T`, `--terse` | Output only the release notes, no headers or other text |
| `-t <TAG>` | Specify a tag to use instead of the latest one |
| `-C <COMMIT>` | Specify a commit hash to use instead of a tag |
| `-h`, `--help` | Show help message |

### Examples

**Standard usage (prints to stdout):**
```bash
git-releasenotes
```

**Copy to clipboard and include PR numbers:**
```bash
git-releasenotes -c -p
```

**Generate notes starting from a specific tag:**
```bash
git-releasenotes -t v1.0.0
```

**Include PR numbers and enable debug output:**
```bash
git-releasenotes -p -X
```

**Generate terse output (no headers) with PR numbers:**
```bash
git-releasenotes -T -p
```

### Output Format

When using the `-p` flag, PR numbers are included in the output:

- **Individual commits**: `- Fix bug (#123) (Author)`
- **Dependabot updates**: `- Updates `package` from 1.0.0 to 1.1.0 (#123)`
- **Consolidated updates**: When multiple PRs update the same package, PR numbers are combined: `- Updates `package` from 1.0.0 to 1.3.0 (#300, #200, #100)`

PR numbers are extracted from:
1. Commit subject lines (e.g., `Bump package (#123)`)
2. Merge commits (e.g., `Merge pull request #123`)
3. GitHub API search by commit SHA (requires `GITHUB_TOKEN`)

## Environment Variables

- `GITHUB_TOKEN`: (Optional but recommended) A GitHub Personal Access Token to fetch details about Pull Requests and search for PRs by commit SHA. If not provided, PR numbers can still be extracted from commit subjects and merge commits, but GitHub API search will be unavailable.

## How PR Numbers Are Found

The tool uses multiple strategies to find PR numbers for commits:

1. **From commit subjects**: Extracts PR numbers from patterns like `(#123)` or `Merge pull request #123`
2. **From merge commits**: Scans merge commits in the commit range and maps merged commits to their PR numbers
3. **From GitHub API**: Searches GitHub for PRs containing a specific commit SHA (requires `GITHUB_TOKEN`)

When multiple PRs update the same dependency, all PR numbers are preserved and displayed in descending order (highest PR number first).

## Testing

Run the test suite with:

```bash
cargo test
```

The test suite includes comprehensive coverage for:
- PR number extraction from various commit message formats
- Dependabot update consolidation with PR number preservation
- Major version change detection
- Output formatting and sorting
