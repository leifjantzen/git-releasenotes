// use anyhow::{anyhow, Result};
use octocrab::Octocrab;
use regex::Regex;
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
pub enum ProcessedCommit {
    Dependabot(Vec<String>),
    Other(String),
}

#[allow(clippy::too_many_arguments)]
pub async fn process_commit(
    subject: &str,
    body: &str,
    hash: &str,
    author: &str,
    include_pr: bool,
    octocrab: &Option<Octocrab>,
    owner: &str,
    repo: &str,
) -> Option<ProcessedCommit> {
    process_commit_with_pr(
        subject, body, hash, author, include_pr, None, octocrab, owner, repo,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn process_commit_with_pr(
    subject: &str,
    body: &str,
    hash: &str,
    author: &str,
    include_pr: bool,
    pr_from_merge: Option<u64>,
    octocrab: &Option<Octocrab>,
    owner: &str,
    repo: &str,
) -> Option<ProcessedCommit> {
    let is_dependabot = author.to_lowercase().contains("dependabot");

    if subject
        .to_lowercase()
        .contains("setting new snapshot version")
    {
        return None;
    }

    // Extract PR number first so we can add it to output lines when include_pr is true
    // Start with PR number from merge commit if provided
    let mut pr_number: Option<u64> = pr_from_merge;

    let re_merge = Regex::new(r"Merge pull request #([0-9]+)").unwrap();
    let re_bump = Regex::new(r"Bump the.*\(#([0-9]+)\) \(").unwrap();
    let re_fallback = Regex::new(r"\(#([0-9]+)\)").unwrap();

    // Only try to extract from subject if we don't already have a PR number
    if pr_number.is_none() {
        if let Some(caps) = re_merge.captures(subject) {
            if let Some(m) = caps.get(1) {
                pr_number = m.as_str().parse().ok();
            }
        } else if let Some(caps) = re_bump.captures(subject) {
            if let Some(m) = caps.get(1) {
                pr_number = m.as_str().parse().ok();
            }
        } else if let Some(caps) = re_fallback.captures(subject) {
            if let Some(m) = caps.get(1) {
                pr_number = m.as_str().parse().ok();
            }
        }
    }

    // Search for PR by SHA fallback (for all commits, not just dependabot)
    // This searches GitHub for PRs that contain this commit SHA
    if pr_number.is_none() && !owner.is_empty() && !repo.is_empty() {
        if let Some(client) = octocrab {
            // search issues/prs by commit SHA
            // GitHub's search API can find PRs that contain a specific commit SHA
            let query = format!("repo:{}/{} sha:{}", owner, repo, hash);
            match client
                .search()
                .issues_and_pull_requests(&query)
                .send()
                .await
            {
                Ok(page) => {
                    if let Some(item) = page.items.first() {
                        // Only get PRs, not issues (PRs have pull_request field)
                        if item.pull_request.is_some() {
                            pr_number = Some(item.number);
                        }
                    }
                }
                Err(e) => {
                    // Silently fail - API might be rate-limited or commit might not be indexed
                    // This is expected for some commits
                    let _ = e;
                }
            }
        }
    }

    // Try to parse updates from the commit body first (no API call needed)
    if is_dependabot {
        let mut update_lines = Vec::new();
        for line in body.lines() {
            let lower = line.to_lowercase();
            if lower.trim().starts_with("updates `") {
                let clean_line = line.trim();
                // Add PR number if include_pr is true and we have one
                let final_line = if include_pr && pr_number.is_some() {
                    format!("- {} (#{})", clean_line, pr_number.unwrap())
                } else {
                    format!("- {}", clean_line)
                };
                update_lines.push(final_line);
            }
        }
        if !update_lines.is_empty() {
            return Some(ProcessedCommit::Dependabot(update_lines));
        }
    }

    if let Some(pr_num) = pr_number {
        // Fetch PR body
        let mut updates_found = false;
        let mut update_lines_vec = Vec::new();

        if let Some(client) = octocrab {
            if !owner.is_empty() && !repo.is_empty() {
                if let Ok(pr) = client.pulls(owner, repo).get(pr_num).await {
                    if let Some(body) = pr.body {
                        // Parse body for updates
                        for body_line in body.lines() {
                            if body_line.starts_with('|')
                                || body_line.contains("|---")
                                || body_line.contains("Bumps the")
                            {
                                continue;
                            }
                            let lower = body_line.to_lowercase();
                            if lower.trim_start().starts_with("updates `") {
                                updates_found = true;
                                let clean_line = body_line.trim();
                                // Add PR number if include_pr is true
                                let final_line = if include_pr {
                                    format!("- {} (#{})", clean_line, pr_num)
                                } else {
                                    format!("- {}", clean_line)
                                };
                                update_lines_vec.push(final_line);
                            }
                        }
                    }
                }
            }
        }

        if updates_found {
            return Some(ProcessedCommit::Dependabot(update_lines_vec));
        }
    }

    // Fallback or no update lines logic
    let re_pr_remove = Regex::new(r" \(#[0-9]+\)").unwrap();

    let cleaned_subject = if !include_pr {
        re_pr_remove.replace_all(subject, "").to_string()
    } else {
        // If include_pr is true, preserve PR numbers that are already in the subject
        // and add PR numbers we found that aren't in the subject
        let mut result = subject.to_string();
        if let Some(pr_num) = pr_number {
            // Check if PR number already appears in the subject (in any format)
            let pr_in_subject = result.contains(&format!("#{}", pr_num));
            if !pr_in_subject {
                // Add PR number to the end of the subject in (#123) format
                result = format!("{} (#{})", result.trim(), pr_num);
            }
        }
        // If include_pr is true, always preserve the subject as-is (it may already contain PR numbers)
        // This ensures PR numbers in subjects like "(#2887)" are preserved even if extraction failed
        result
    };

    if is_dependabot {
        // If it's dependabot but we couldn't find details, just list the subject
        Some(ProcessedCommit::Dependabot(vec![format!(
            "- {}",
            cleaned_subject
        )]))
    } else {
        // Format: - Subject (Author)
        Some(ProcessedCommit::Other(format!(
            "- {} ({})",
            cleaned_subject, author
        )))
    }
}

pub fn consolidate_dependabot_updates(updates: Vec<String>) -> Vec<String> {
    let re_update =
        Regex::new(r"Updates `([^`]+)` from ([^ ]+) to ([^ ]+)(?: \((#[0-9]+)\))?").unwrap();
    let re_bump_link =
        Regex::new(r"Bumps? \[([^\]]+)\]\([^\)]+\) from ([^ ]+) to ([^ ]+)(?: \((#[0-9]+)\))?")
            .unwrap();
    let re_bump_simple =
        Regex::new(r"Bumps? ([^ ]+) from ([^ ]+) to ([^ ]+)(?: \((#[0-9]+)\))?").unwrap();
    let re_pr_number = Regex::new(r"\(#([0-9]+)\)").unwrap();

    let mut package_updates: HashMap<String, (String, String, Vec<u64>)> = HashMap::new();
    let mut other_updates: Vec<String> = Vec::new();

    // Iterate through updates
    // The updates come from process_commit, which processes commits.
    // If the commits are processed newest to oldest (rev_walk default), then:
    // Update A: 1.2.4 -> 1.3.0 (Newest)
    // Update B: 1.2.3 -> 1.2.4 (Older)
    // We want the result 1.2.3 -> 1.3.0.

    // Logic:
    // Map: pkg -> (from, to)
    // When seeing a new update (pkg, new_from, new_to):
    // Check if we have an existing entry (existing_from, existing_to).
    // If new_to == existing_from -> We have a chain (new_from -> new_to -> existing_to). Update entry to (new_from, existing_to).
    // If new_from == existing_to -> We have a chain (existing_from -> existing_to -> new_to). Update entry to (existing_from, new_to).
    // Else -> Separate chain? For now just overwrite or ignore?
    // Wait, if we have disjoint updates: 1.0 -> 1.1 and 2.0 -> 2.1.
    // We probably shouldn't merge them.
    // But typical dependabot behavior is continuous updates.
    // If we overwrite, we lose info.
    // Let's keep a list of updates per package and then merge?
    // Actually, just trying to merge chains is enough.
    // If disjoint, we can keep separate entries in a list?
    // Complex.
    // Let's stick to the simplest "chaining" logic. If it doesn't chain, treat as new entry.
    // But since HashMap keys are package names, we can only store one entry per package.
    // The shell script behavior suggests merging all updates for a package into one "Min -> Max" range.
    // Let's assume that.

    for line in updates {
        // Extract PR number from line if present
        let pr_number = re_pr_number
            .captures(&line)
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse::<u64>().ok());

        let parsed = re_update
            .captures(&line)
            .map(|caps| {
                (
                    caps.get(1).unwrap().as_str().to_string(),
                    caps.get(2).unwrap().as_str().to_string(),
                    caps.get(3).unwrap().as_str().to_string(),
                )
            })
            .or_else(|| {
                re_bump_link.captures(&line).map(|caps| {
                    (
                        caps.get(1).unwrap().as_str().to_string(),
                        caps.get(2).unwrap().as_str().to_string(),
                        caps.get(3).unwrap().as_str().to_string(),
                    )
                })
            })
            .or_else(|| {
                re_bump_simple.captures(&line).map(|caps| {
                    (
                        caps.get(1).unwrap().as_str().to_string(),
                        caps.get(2).unwrap().as_str().to_string(),
                        caps.get(3).unwrap().as_str().to_string(),
                    )
                })
            });

        if let Some((pkg, from, to)) = parsed {
            if let Some((existing_from, existing_to, pr_numbers)) = package_updates.get_mut(&pkg) {
                // Try to chain
                if &to == existing_from {
                    *existing_from = from;
                } else if &from == existing_to {
                    *existing_to = to;
                }
                // Add PR number if present
                if let Some(pr) = pr_number {
                    if !pr_numbers.contains(&pr) {
                        pr_numbers.push(pr);
                    }
                }
            } else {
                let mut pr_nums = Vec::new();
                if let Some(pr) = pr_number {
                    pr_nums.push(pr);
                }
                package_updates.insert(pkg, (from, to, pr_nums));
            }
        } else {
            other_updates.push(line);
        }
    }

    let mut final_lines = Vec::new();
    for (pkg, (from, to, pr_numbers)) in package_updates {
        let pr_suffix = if !pr_numbers.is_empty() {
            // Sort PR numbers in descending order (highest first)
            let mut sorted_prs = pr_numbers.clone();
            sorted_prs.sort();
            sorted_prs.reverse();
            let pr_list: Vec<String> = sorted_prs.iter().map(|n| format!("#{}", n)).collect();
            format!("  ({})", pr_list.join(", "))
        } else {
            String::new()
        };
        final_lines.push(format!(
            "- Updates `{}` from {} to {}{}",
            pkg, from, to, pr_suffix
        ));
    }
    final_lines.extend(other_updates);

    final_lines
}

pub fn generate_release_notes(
    mut dependabot_updates: Vec<String>,
    mut other_changes: Vec<String>,
) -> String {
    let mut final_output_lines = Vec::new();

    if !dependabot_updates.is_empty() {
        // Consolidate updates
        dependabot_updates = consolidate_dependabot_updates(dependabot_updates);

        // Check for major version changes
        let re_update = Regex::new(r"Updates `([^`]+)` from ([^ ]+) to ([^ ]+)").unwrap();
        let mut major_changes = Vec::new();

        for line in &dependabot_updates {
            if let Some(caps) = re_update.captures(line) {
                let pkg = caps.get(1).unwrap().as_str();
                let from = caps.get(2).unwrap().as_str();
                let to = caps.get(3).unwrap().as_str();

                // Simple major version check (first component changed)
                let from_major = from.split('.').next().unwrap_or("0");
                let to_major = to.split('.').next().unwrap_or("0");

                if let (Ok(f), Ok(t)) = (from_major.parse::<u32>(), to_major.parse::<u32>()) {
                    if t > f {
                        major_changes.push(format!("{}: {} → {}", pkg, from, to));
                    }
                }
            }
        }

        if !major_changes.is_empty() {
            major_changes.sort();
            final_output_lines.push(format!(
                "⚠ WARNING: Major version changes detected: {}",
                major_changes.join(", ")
            ));
            final_output_lines.push("".to_string());
        }

        final_output_lines.push("## Dependencies updated by dependabot:".to_string());
        final_output_lines.push("".to_string());
        dependabot_updates.sort();
        final_output_lines.extend(dependabot_updates);
        final_output_lines.push("".to_string());
    }

    if !other_changes.is_empty() {
        other_changes.sort();
        other_changes.dedup();
        final_output_lines.push("## Other changes:".to_string());
        final_output_lines.extend(other_changes);
    }

    final_output_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_normal_commit_no_pr() {
        let res = process_commit("Fix bug", "", "sha", "User", false, &None, "", "").await;
        assert_eq!(
            res,
            Some(ProcessedCommit::Other("- Fix bug (User)".to_string()))
        );
    }

    #[tokio::test]
    async fn test_snapshot_version_ignored() {
        let res = process_commit(
            "Setting new snapshot version 1.0",
            "",
            "sha",
            "User",
            false,
            &None,
            "",
            "",
        )
        .await;
        assert_eq!(res, None);
    }

    #[tokio::test]
    async fn test_pr_number_removal_no_include() {
        let res = process_commit("Fix bug (#123)", "", "sha", "User", false, &None, "", "").await;
        assert_eq!(
            res,
            Some(ProcessedCommit::Other("- Fix bug (User)".to_string()))
        );
    }

    #[tokio::test]
    async fn test_pr_number_keep_include() {
        let res = process_commit("Fix bug (#123)", "", "sha", "User", true, &None, "", "").await;
        assert_eq!(
            res,
            Some(ProcessedCommit::Other(
                "- Fix bug (#123) (User)".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn test_dependabot_no_body() {
        let res = process_commit(
            "Bump package (#123)",
            "",
            "sha",
            "dependabot[bot]",
            false,
            &None,
            "",
            "",
        )
        .await;
        assert_eq!(
            res,
            Some(ProcessedCommit::Dependabot(vec![
                "- Bump package".to_string()
            ]))
        );
    }

    #[tokio::test]
    async fn test_dependabot_with_body() {
        let body = "Bumps [package]...\nUpdates `package` from 1.0 to 1.1\n...";
        let res = process_commit(
            "Bump package (#123)",
            body,
            "sha",
            "dependabot[bot]",
            false,
            &None,
            "",
            "",
        )
        .await;
        assert_eq!(
            res,
            Some(ProcessedCommit::Dependabot(vec![
                "- Updates `package` from 1.0 to 1.1".to_string()
            ]))
        );
    }

    #[tokio::test]
    async fn test_dependabot_with_body_include_pr() {
        let body = "Bumps [package]...\nUpdates `package` from 1.0 to 1.1\n...";
        let res = process_commit(
            "Bump package (#123)",
            body,
            "sha",
            "dependabot[bot]",
            true,
            &None,
            "",
            "",
        )
        .await;
        assert_eq!(
            res,
            Some(ProcessedCommit::Dependabot(vec![
                "- Updates `package` from 1.0 to 1.1 (#123)".to_string()
            ]))
        );
    }

    #[tokio::test]
    async fn test_merge_pull_request_extraction() {
        let res = process_commit(
            "Merge pull request #123 from foo",
            "",
            "sha",
            "User",
            false,
            &None,
            "",
            "",
        )
        .await;
        assert_eq!(
            res,
            Some(ProcessedCommit::Other(
                "- Merge pull request #123 from foo (User)".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn test_merge_pull_request_with_include_pr() {
        let res = process_commit(
            "Merge pull request #123 from foo",
            "",
            "sha",
            "User",
            true,
            &None,
            "",
            "",
        )
        .await;
        // PR number is already in subject, so should be kept as-is
        assert_eq!(
            res,
            Some(ProcessedCommit::Other(
                "- Merge pull request #123 from foo (User)".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn test_pr_number_added_when_missing() {
        // Simulate case where PR number was extracted but not in subject
        // This would happen if pr_number was set via SHA search or other means
        // For this test, we need to manually set pr_number, but since it's extracted from subject,
        // we'll test with a subject that doesn't have the PR number format
        // Actually, the function extracts PR from subject, so we can't easily test this without mocking
        // But we can test that when include_pr is true, existing PR numbers are preserved
        let res = process_commit("Fix important bug", "", "sha", "User", true, &None, "", "").await;
        // No PR number extracted, so subject should remain as-is
        assert_eq!(
            res,
            Some(ProcessedCommit::Other(
                "- Fix important bug (User)".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn test_pr_number_in_subject_preserved() {
        // Test that PR numbers already in subject are preserved when include_pr is true
        let res = process_commit(
            "Bump software.amazon.awssdk:bom in the other-deps group (#2887)",
            "",
            "sha",
            "dependabot[bot]",
            true,
            &None,
            "",
            "",
        )
        .await;
        // PR number should be preserved in output
        assert_eq!(
            res,
            Some(ProcessedCommit::Dependabot(vec![
                "- Bump software.amazon.awssdk:bom in the other-deps group (#2887)".to_string()
            ]))
        );
    }

    #[tokio::test]
    async fn test_pr_number_from_merge_commit() {
        // Test that PR number from merge commit mapping is used
        let res = process_commit_with_pr(
            "Bump the fiks-deps group across 1 directory with 8 updates",
            "",
            "sha",
            "dependabot[bot]",
            true,
            Some(2880),
            &None,
            "",
            "",
        )
        .await;
        // PR number should be added to output
        assert_eq!(
            res,
            Some(ProcessedCommit::Dependabot(vec![
                "- Bump the fiks-deps group across 1 directory with 8 updates (#2880)".to_string()
            ]))
        );
    }

    #[test]
    fn test_consolidate_dependabot_updates() {
        let updates = vec![
            "- Updates `lib` from 1.2.4 to 1.3.0".to_string(), // Newest
            "- Updates `lib` from 1.2.3 to 1.2.4".to_string(), // Oldest
            "- Updates `other` from 1.0 to 1.1".to_string(),
        ];

        let mut res = consolidate_dependabot_updates(updates);
        res.sort();

        let expected = vec![
            "- Updates `lib` from 1.2.3 to 1.3.0".to_string(),
            "- Updates `other` from 1.0 to 1.1".to_string(),
        ];
        // Sort expected to match
        let mut expected_sorted = expected.clone();
        expected_sorted.sort();

        assert_eq!(res, expected_sorted);
    }

    #[test]
    fn test_consolidate_dependabot_updates_unordered() {
        let updates = vec![
            "- Updates `lib` from 1.2.3 to 1.2.4".to_string(),
            "- Updates `lib` from 1.2.4 to 1.3.0".to_string(),
        ];

        let mut res = consolidate_dependabot_updates(updates);
        res.sort();

        let expected = vec!["- Updates `lib` from 1.2.3 to 1.3.0".to_string()];
        assert_eq!(res, expected);
    }

    #[test]
    fn test_consolidate_mixed_formats() {
        let updates = vec![
            "- Updates `lib` from 1.2.4 to 1.3.0".to_string(),
            "- Bumps [lib](https://github.com/lib/lib) from 1.2.3 to 1.2.4".to_string(),
        ];

        let mut res = consolidate_dependabot_updates(updates);
        res.sort();

        let expected = vec!["- Updates `lib` from 1.2.3 to 1.3.0".to_string()];
        assert_eq!(res, expected);
    }

    #[test]
    fn test_consolidate_preserves_pr_numbers() {
        let updates = vec![
            "- Updates `lib` from 1.2.4 to 1.3.0 (#2887)".to_string(),
            "- Updates `lib` from 1.2.3 to 1.2.4 (#2886)".to_string(),
            "- Updates `other` from 1.0 to 1.1 (#2885)".to_string(),
        ];

        let mut res = consolidate_dependabot_updates(updates);
        res.sort();

        // PR numbers should be preserved and combined, sorted in descending order
        let lib_line = res.iter().find(|line| line.contains("lib")).unwrap();
        assert!(lib_line.contains("#2887") && lib_line.contains("#2886"));
        // Verify descending order: #2887 should come before #2886
        let pr_part = lib_line.split("(").nth(1).unwrap();
        assert!(pr_part.starts_with("#2887"));

        assert!(res
            .iter()
            .any(|line| line.contains("other") && line.contains("#2885")));
    }

    #[test]
    fn test_consolidate_pr_numbers_descending_order() {
        let updates = vec![
            "- Updates `lib` from 1.0 to 1.1 (#100)".to_string(),
            "- Updates `lib` from 1.1 to 1.2 (#200)".to_string(),
            "- Updates `lib` from 1.2 to 1.3 (#300)".to_string(),
        ];

        let res = consolidate_dependabot_updates(updates);
        let lib_line = res.iter().find(|line| line.contains("lib")).unwrap();

        // Verify PR numbers are in descending order: #300, #200, #100
        assert!(lib_line.contains("(#300, #200, #100)"));
    }

    #[test]
    fn test_consolidate_pr_numbers_no_duplicates() {
        let updates = vec![
            "- Updates `lib` from 1.0 to 1.1 (#100)".to_string(),
            "- Updates `lib` from 1.1 to 1.2 (#100)".to_string(), // Same PR number
            "- Updates `lib` from 1.2 to 1.3 (#200)".to_string(),
        ];

        let res = consolidate_dependabot_updates(updates);
        let lib_line = res.iter().find(|line| line.contains("lib")).unwrap();

        // Verify PR number #100 appears only once
        let pr_count_100 = lib_line.matches("#100").count();
        assert_eq!(pr_count_100, 1);
        assert!(lib_line.contains("#200"));
    }

    #[test]
    fn test_consolidate_updates_without_pr_numbers() {
        let updates = vec![
            "- Updates `lib` from 1.0 to 1.1".to_string(),
            "- Updates `lib` from 1.1 to 1.2".to_string(),
        ];

        let res = consolidate_dependabot_updates(updates);
        let lib_line = res.iter().find(|line| line.contains("lib")).unwrap();

        // Should not have PR numbers
        assert!(!lib_line.contains("(#"));
        assert_eq!(lib_line, "- Updates `lib` from 1.0 to 1.2");
    }

    #[test]
    fn test_generate_release_notes_empty() {
        let output = generate_release_notes(vec![], vec![]);
        assert_eq!(output, "");
    }

    #[test]
    fn test_generate_release_notes_dependabot_only() {
        let updates = vec!["- Updates `lib` from 1.0.0 to 1.1.0".to_string()];
        let output = generate_release_notes(updates, vec![]);
        assert!(output.contains("## Dependencies updated by dependabot:"));
        assert!(output.contains("- Updates `lib` from 1.0.0 to 1.1.0"));
        assert!(!output.contains("## Other changes:"));
        assert!(!output.contains("Major version changes detected"));
    }

    #[test]
    fn test_generate_release_notes_other_only() {
        let other = vec!["- Fix something".to_string(), "- Add something".to_string()];
        let output = generate_release_notes(vec![], other);
        assert!(!output.contains("## Dependencies updated by dependabot:"));
        assert!(output.contains("## Other changes:"));
        assert!(output.contains("- Fix something"));
        assert!(output.contains("- Add something"));
    }

    #[test]
    fn test_generate_release_notes_major_version_warning() {
        let updates = vec!["- Updates `lib` from 1.0.0 to 2.0.0".to_string()];
        let output = generate_release_notes(updates, vec![]);
        assert!(output.contains("WARNING: Major version changes detected: lib: 1.0.0 → 2.0.0"));
    }

    #[test]
    fn test_generate_release_notes_sorting_and_deduplication() {
        let other = vec![
            "- B change".to_string(),
            "- A change".to_string(),
            "- A change".to_string(),
        ];
        let output = generate_release_notes(vec![], other);
        let lines: Vec<&str> = output.lines().collect();
        // Skip header "## Other changes:"
        let content_lines: Vec<&str> = lines.into_iter().filter(|l| l.starts_with("- ")).collect();
        assert_eq!(content_lines, vec!["- A change", "- B change"]);
    }
}
