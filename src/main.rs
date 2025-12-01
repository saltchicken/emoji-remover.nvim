use clap::Parser;
use git2::Repository;
use glob::Pattern;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use thiserror::Error;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Error)]
enum AppError {
    #[error("Failed to discover git repository: {0}")]
    GitDiscovery(#[from] git2::Error),
    #[error("Cannot find toplevel: this is a bare repository")]
    BareRepo,
    #[error("File system walk error: {0}")]
    WalkDir(#[from] walkdir::Error),
    #[error("Invalid glob pattern: {0}")]
    InvalidGlob(#[from] glob::PatternError),
    #[error("Failed to read file {0}: {1}")]
    FileRead(PathBuf, #[source] std::io::Error),
    #[error("Failed to write file {0}: {1}")]
    FileWrite(PathBuf, #[source] std::io::Error),
    #[error("File content for {0} is not valid UTF-8")]
    InvalidUtf8(PathBuf),
}

#[derive(Parser, Debug)]
struct Cli {
    /// Glob patterns to include (e.g., "*.rs" "src/**")

    #[arg(long, short = 'i', num_args(1..), default_values_t = ["*.rs".to_string(), "*.toml".to_string(), "*.py".to_string(), "*.jsx".to_string(), "*.tsx".to_string(), "*.html".to_string(), "*.css".to_string()])]
    include: Vec<String>,
    /// Glob patterns to exclude (e.g., "target/*" "*.log")
    #[arg(long, short = 'e', num_args(1..))]
    exclude: Vec<String>,
}

fn process_file(file_path: &Path) -> Result<(), AppError> {
    let content_bytes =
        fs::read(file_path).map_err(|e| AppError::FileRead(file_path.to_path_buf(), e))?;
    let content = String::from_utf8(content_bytes)
        .map_err(|_| AppError::InvalidUtf8(file_path.to_path_buf()))?;

    let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");

    let mut modified = false;
    let cleaned_lines: Vec<String> = content
        .lines()
        .map(|line| {
            let (comment_start, block_ender): (Option<usize>, Option<&str>) = if ext == "html" {
                (line.find("<!--"), Some("-->"))
            } else if ext == "css" {
                (line.find("/*"), Some("*/"))
            } else if matches!(ext, "jsx" | "tsx") {
                let slash_idx = line.find("//");
                let block_idx = line.find("{/*");
                match (slash_idx, block_idx) {
                    (Some(s), Some(b)) => {
                        // Pick the one that appears first
                        if s < b {
                            (Some(s), None)
                        } else {
                            (Some(b), Some("*/}"))
                        }
                    }
                    (Some(s), None) => (Some(s), None),
                    (None, Some(b)) => (Some(b), Some("*/}")),
                    (None, None) => (None, None),
                }
            } else if matches!(ext, "rs" | "js" | "ts") {
                (line.find("//"), None)
            } else {
                (line.find('#'), None)
            };

            if let Some(start) = comment_start {
                if let Some(ender) = block_ender {
                    // Try to find the closing tag on the same line
                    if let Some(end_offset) = line[start..].find(ender) {
                        let end = start + end_offset + ender.len();
                        let comment_content = &line[start..end];

                        if comment_content.contains("‼️") {
                            modified = true;
                            let prefix = &line[..start];
                            let suffix = &line[end..];
                            // If the line is just the comment, trim. Otherwise splice it out.
                            if suffix.trim().is_empty() {
                                prefix.trim_end().to_string()
                            } else {
                                format!("{}{}", prefix, suffix)
                            }
                        } else {
                            line.to_string()
                        }
                    } else {
                        // Fallback for unclosed block on same line (truncates rest of line)
                        let comment_part = &line[start..];
                        if comment_part.contains("‼️") {
                            modified = true;
                            line[..start].trim_end().to_string()
                        } else {
                            line.to_string()
                        }
                    }
                } else {
                    // Standard single-line comment processing
                    let comment_part = &line[start..];
                    if comment_part.contains("‼️") {
                        modified = true;
                        line[..start].trim_end().to_string()
                    } else {
                        line.to_string()
                    }
                }
            } else {
                line.to_string()
            }
        })
        .collect();

    if modified {
        let output = cleaned_lines.join("\n");
        fs::write(file_path, output)
            .map_err(|e| AppError::FileWrite(file_path.to_path_buf(), e))?;
        eprintln!("Cleaned: {}", file_path.display());
    }

    Ok(())
}

fn find_git_root() -> Result<PathBuf, AppError> {
    let repo = Repository::discover(".").map_err(AppError::GitDiscovery)?;
    let workdir = repo.workdir().ok_or(AppError::BareRepo)?;
    Ok(workdir.to_path_buf())
}

fn is_git_dir(entry: &DirEntry) -> bool {
    entry.file_name().to_str().map_or(false, |s| s == ".git")
}

fn list_non_ignored_files(
    repo_root: &Path,
    includes: &[String],
    excludes: &[String],
) -> Result<Vec<PathBuf>, AppError> {
    let repo = Repository::open(repo_root)?;
    let include_patterns: Result<Vec<Pattern>, _> =
        includes.iter().map(|s| Pattern::new(s)).collect();
    let include_patterns = include_patterns.map_err(AppError::InvalidGlob)?;
    let exclude_patterns: Result<Vec<Pattern>, _> =
        excludes.iter().map(|s| Pattern::new(s)).collect();
    let exclude_patterns = exclude_patterns.map_err(AppError::InvalidGlob)?;
    let mut non_ignored_files = Vec::new();
    let walker = WalkDir::new(repo_root)
        .into_iter()
        .filter_entry(|e| !is_git_dir(e));
    for entry_result in walker {
        let entry = entry_result?;
        if entry.path().is_dir() {
            continue;
        }
        let relative_path = match entry.path().strip_prefix(repo_root) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if relative_path.as_os_str().is_empty() {
            continue;
        }
        if repo.is_path_ignored(relative_path)? {
            continue;
        }
        let relative_path_str = match relative_path.to_str() {
            Some(s) => s.replace('\\', "/"),
            None => continue, // Skip non-UTF8 paths
        };
        let mut is_excluded = false;
        for pattern in &exclude_patterns {
            if pattern.matches(&relative_path_str) {
                is_excluded = true;
                break;
            }
        }
        if is_excluded {
            continue;
        }
        if include_patterns.is_empty() {
            non_ignored_files.push(entry.path().to_path_buf());
        } else {
            let mut is_included = false;
            for pattern in &include_patterns {
                if pattern.matches(&relative_path_str) {
                    is_included = true;
                    break;
                }
            }
            if is_included {
                non_ignored_files.push(entry.path().to_path_buf());
            }
        }
    }
    Ok(non_ignored_files)
}

fn main() {
    let cli = Cli::parse();
    let root = match find_git_root() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("Error finding git root: {}", err);
            process::exit(1);
        }
    };
    let files_to_process = match list_non_ignored_files(&root, &cli.include, &cli.exclude) {
        Ok(files) => files,
        Err(err) => {
            eprintln!("Error listing files: {}", err);
            process::exit(1);
        }
    };
    if files_to_process.is_empty() {
        eprintln!("No files found matching criteria.");
        return;
    }
    eprintln!("Found {} files to process...", files_to_process.len());
    for file_path in files_to_process {
        if let Err(e) = process_file(&file_path) {
            eprintln!("Error processing file {}: {}", file_path.display(), e);
        }
    }
    eprintln!("Done.");
}

