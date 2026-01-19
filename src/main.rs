use anyhow::{anyhow, Context, Result};
use arboard::Clipboard;
use clap::Parser;
use octocrab::Octocrab;
use regex::Regex;
use std::process::Command;
use std::env;
use git_releasenotes::{process_commit, generate_release_notes, ProcessedCommit};
use gix;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Copy output to clipboard
    #[arg(short = 'c')]
    clipboard: bool,

    /// Include PR numbers in output
    #[arg(short = 'p')]
    include_pr_numbers: bool,

    /// List raw commits that form the basis of the output
    #[arg(short = 'x')]
    show_raw_commits: bool,

    /// Enable debug logging
    #[arg(short = 'X')]
    debug_mode: bool,

    /// Output only the release notes, no headers or other text
    #[arg(short = 'T', long)]
    terse: bool,

    /// Specify a tag to use instead of latest
    #[arg(short = 't')]
    tag: Option<String>,

    /// Specify a commit hash to use instead of tag
    #[arg(short = 'C', conflicts_with = "tag")]
    commit: Option<String>,
}

fn debug(msg: &str, debug_mode: bool) {
    if debug_mode {
        eprintln!("[DEBUG] {}", msg);
    }
}

fn run_git(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .context(format!("Failed to execute git command: git {:?}", args))?;

    if !output.status.success() {
        return Err(anyhow!(
            "Git command failed: git {:?}\nStderr: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if it exists
    dotenv::dotenv().ok();

    let args = Args::parse();
    
    // Open repo
    let repo = gix::discover(".")
        .context("Failed to discover git repository")?;
        
    // Check if git CLI is available (for fetch/pull which are complex in gix)
    if Command::new("git").arg("--version").output().is_err() {
        return Err(anyhow!("Error: git is not installed or not in PATH"));
    }

    if !args.terse {
        // Check for local changes
        // Using git CLI for diff check as it's robust for 'dirty' check
        let diff_exit = Command::new("git")
            .args(&["diff", "--quiet"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let diff_cached_exit = Command::new("git")
            .args(&["diff", "--cached", "--quiet"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !diff_exit || !diff_cached_exit {
            eprintln!("Du har lokale endringer. Commit eller stash før du kjører scriptet.");
            std::process::exit(1);
        }
    }

    // Fetch tags
    if args.terse {
        let _ = run_git(&["fetch", "origin", "--tags"]);
    } else {
        run_git(&["fetch", "origin", "--tags"])?;
    }

    let main_branch = "main";
    // Get current branch
    let head_ref = repo.head()?;
    let current_branch = head_ref.referent_name().map(|n| n.as_bstr().to_string()).unwrap_or_default();
    // referent_name gives refs/heads/main, typically we want short name
    let current_branch_short = current_branch.strip_prefix("refs/heads/").unwrap_or(&current_branch);

    if current_branch_short != main_branch {
        if args.terse {
            let _ = run_git(&["checkout", main_branch]);
        } else {
            run_git(&["checkout", main_branch])?;
        }
    }

    if args.terse {
        let _ = run_git(&["pull", "--ff-only", "origin", main_branch]);
    } else {
        run_git(&["pull", "--ff-only", "origin", main_branch])?;
    }
    
    // Re-discover repo after pull/checkout potentially changed things? 
    // Usually safe to keep using 'repo' handle, but head might have moved.
    let repo = gix::discover(".")?; 

    let from_ref_oid = if let Some(commit_sha) = &args.commit {
        let obj = repo.rev_parse_single(commit_sha.as_str())?;
        obj
    } else if let Some(tag_name) = &args.tag {
        let tag_ref_name = format!("refs/tags/{}", tag_name);
        let tag_ref = repo.find_reference(&tag_ref_name)
            .map_err(|_| anyhow!("Error: '{}' exists but is not a tag", tag_name))?;
        tag_ref.into_fully_peeled_id().context("Failed to peel tag")?
    } else {
         // Find latest tag
         // gix doesn't have a direct "describe --tags" equivalent built-in simply yet
         // We can iterate tags and find the one reachable from HEAD that is closest?
         // For now, to be safe and concise, falling back to git describe or implementing a simple walk.
         // Let's stick to git describe for the *logic* of "latest tag" as it's complex to replicate exactly.
         match run_git(&["describe", "--tags", "--abbrev=0"]) {
            Ok(tag) => {
                 let tag_ref_name = format!("refs/tags/{}", tag);
                 let tag_ref = repo.find_reference(&tag_ref_name)
                    .map_err(|_| anyhow!("Error resolving found tag {}", tag))?;
                 tag_ref.into_fully_peeled_id().context("Failed to peel tag")?
            },
            Err(_) => {
                debug("Error finding latest tag", args.debug_mode);
                return Err(anyhow!("Error: No tags found in repository"));
            }
        }
    };

    let from_oid = from_ref_oid.object()?.id;
    let head_oid = repo.head()?.into_peeled_id().context("HEAD not found")?;

    // Commit count
    // Walk from HEAD to from_oid
    // Efficient way:
    let walk = repo.rev_walk([head_oid]).all()?;
    let mut commit_ids = Vec::new();
    for res in walk {
        let info = res?;
        let oid = info.id;
        if oid == from_oid {
            break;
        }
        commit_ids.push(oid);
    }
    
    let commit_count = commit_ids.len();
    debug(&format!("Found {} commits between {} and HEAD", commit_count, from_oid), args.debug_mode);
    
    if commit_count == 0 {
         debug("WARNING: No commits found", args.debug_mode);
    }

    if args.show_raw_commits {
        for oid in &commit_ids {
            let obj = repo.find_object(*oid)?;
            let commit = obj.into_commit();
            let msg = commit.message()?.summary().to_string();
            let author = commit.author()?.name.to_string();
            println!("{} {} ({})", oid, msg, author);
        }
        return Ok(());
    }
    
    // Display ref name
    let display_ref = if let Some(t) = &args.tag {
        t.clone()
    } else if let Some(c) = &args.commit {
        c.clone()
    } else {
        // We resolved from describe
         match run_git(&["describe", "--tags", "--abbrev=0"]) {
            Ok(t) => t,
            Err(_) => from_oid.to_string(),
        }
    };

    if !args.terse {
        println!();
        println!("Siste release: {}", display_ref);
        println!();
        println!("Commits siden {}:", display_ref);
        println!("----------------------------------------");
    }

    // GitHub client setup
    let token = env::var("GITHUB_TOKEN").ok();
    let octocrab = if let Some(t) = token {
        Octocrab::builder().personal_token(t).build().ok()
    } else {
        None
    };
    
    // Remote URL
    let remote = repo.find_remote("origin").ok();
    let remote_url = remote.and_then(|r| r.url(gix::remote::Direction::Fetch).map(|u| u.to_bstring().to_string()))
        .or_else(|| run_git(&["remote", "get-url", "origin"]).ok())
        .unwrap_or_default();
        
    let repo_regex = Regex::new(r"github\.com[:/]([^/]+)/([^/\.]+)(\.git)?").unwrap();
    let (owner, repo_name) = if let Some(caps) = repo_regex.captures(&remote_url) {
        (caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default(), 
         caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default())
    } else {
        (String::new(), String::new())
    };

    let mut dependabot_updates = Vec::new();
    let mut other_changes = Vec::new();

    for oid in commit_ids {
        let obj = repo.find_object(oid)?;
        let commit = obj.into_commit();
        let msg = commit.message()?;
        let subject = msg.summary().to_string();
        let body = msg.body().map(|b| b.to_string()).unwrap_or_default();
        let author = commit.author()?.name.to_string();
        let hash = oid.to_string();

        let result = process_commit(&subject, &body, &hash, &author, args.include_pr_numbers, &octocrab, &owner, &repo_name).await;
        if let Some(res) = result {
            match res {
                ProcessedCommit::Dependabot(lines) => dependabot_updates.extend(lines),
                ProcessedCommit::Other(line) => other_changes.push(line),
            }
        }
    }

    // Print output
    let full_output = generate_release_notes(dependabot_updates, other_changes);
    
    if !full_output.is_empty() {
        println!("{}", full_output);
    }
    
    if args.clipboard {
        match Clipboard::new() {
            Ok(mut clipboard) => {
                if let Err(e) = clipboard.set_text(&full_output) {
                     eprintln!("Failed to copy to clipboard: {}", e);
                }
            },
            Err(e) => eprintln!("Failed to initialize clipboard: {}", e),
        }
    }

    Ok(())
}
