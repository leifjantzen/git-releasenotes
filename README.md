# git-releasenotes

A Rust-based tool to generate release notes from git commits since the last tag. This is a rewrite of a shell script to provide cross-platform support (Linux, macOS, Windows) and better performance, without relying on system utilities like `sed` or `awk`.

## Features

- **Automatic Release Notes**: Generates notes based on commits since the last git tag.
- **GitHub Integration**: Fetches Pull Request titles and updates from GitHub to provide richer context.
- **Clipboard Support**: Optionally copies the generated notes directly to your clipboard.
- **Cross-Platform**: Works on Linux, macOS, and Windows.
- **Dependabot Handling**: Special handling for Dependabot commits to group or format them appropriately.

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
| `-p` | Include PR numbers in output |
| `-x` | List raw commits that form the basis of the output |
| `-X` | Enable debug logging |
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

## Environment Variables

- `GITHUB_TOKEN`: (Optional but recommended) A GitHub Personal Access Token to fetch details about Pull Requests. If not provided, PR details might be missing or limited by API rate limits.
