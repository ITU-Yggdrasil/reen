use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct FilePatch {
    pub(crate) old_path: Option<String>,
    pub(crate) new_path: Option<String>,
    hunks: Vec<Hunk>,
    pub(crate) hunk_lines: Vec<HunkLine>,
    pub(crate) is_new_file: bool,
    pub(crate) is_deletion: bool,
}

#[derive(Debug, Clone)]
struct Hunk {
    old_start: usize,
    lines: Vec<HunkLine>,
}

#[derive(Debug, Clone)]
pub(crate) struct HunkLine {
    pub(crate) kind: HunkLineKind,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HunkLineKind {
    Context,
    Add,
    Remove,
}

pub(crate) fn extract_unified_diff_from_agent_output(text: &str) -> Option<String> {
    if let Some(idx) = text.find("diff --git ") {
        return Some(text[idx..].trim().to_string());
    }

    if let Some(idx) = text.find("\n--- ") {
        return Some(text[idx + 1..].trim().to_string());
    }
    if text.trim_start().starts_with("--- ") {
        return Some(text.trim().to_string());
    }
    None
}

pub(crate) fn parse_unified_diff(diff: &str) -> Result<Vec<FilePatch>> {
    let mut lines = diff.lines().peekable();
    let mut patches = Vec::new();

    while let Some(line) = lines.next() {
        if !line.starts_with("diff --git ") {
            continue;
        }

        let mut old_path: Option<String> = None;
        let mut new_path: Option<String> = None;
        let mut is_new_file = false;
        let mut is_deletion = false;
        let mut hunks = Vec::new();
        let mut hunk_lines_flat = Vec::new();

        while let Some(peek) = lines.peek() {
            let p = *peek;
            if p.starts_with("diff --git ") {
                break;
            }
            let l = lines.next().unwrap();
            if l.starts_with("new file mode") {
                is_new_file = true;
            } else if l.starts_with("deleted file mode") {
                is_deletion = true;
            } else if l.starts_with("--- ") {
                old_path = Some(extract_patch_path(l, "--- ")?);
                if old_path.as_deref() == Some("/dev/null") {
                    old_path = None;
                    is_new_file = true;
                }
            } else if l.starts_with("+++ ") {
                new_path = Some(extract_patch_path(l, "+++ ")?);
                if new_path.as_deref() == Some("/dev/null") {
                    new_path = None;
                    is_deletion = true;
                }
            } else if l.starts_with("@@ ") {
                let (old_start, _new_start) = parse_hunk_header(l)?;
                let mut h_lines = Vec::new();
                while let Some(next) = lines.peek() {
                    let nl = *next;
                    if nl.starts_with("diff --git ") || nl.starts_with("@@ ") {
                        break;
                    }
                    let hl = lines.next().unwrap();
                    if hl.starts_with("\\ No newline") {
                        continue;
                    }
                    if hl.is_empty() {
                        h_lines.push(HunkLine {
                            kind: HunkLineKind::Context,
                            text: String::new(),
                        });
                        continue;
                    }
                    let (kind, text) = match hl.chars().next().unwrap() {
                        ' ' => (HunkLineKind::Context, hl[1..].to_string()),
                        '+' => (HunkLineKind::Add, hl[1..].to_string()),
                        '-' => (HunkLineKind::Remove, hl[1..].to_string()),
                        _ => continue,
                    };
                    let h = HunkLine { kind, text };
                    hunk_lines_flat.push(h.clone());
                    h_lines.push(h);
                }
                hunks.push(Hunk {
                    old_start,
                    lines: h_lines,
                });
            }
        }

        let old_path_norm = old_path.and_then(normalize_patch_path);
        let new_path_norm = new_path.and_then(normalize_patch_path);

        patches.push(FilePatch {
            old_path: old_path_norm,
            new_path: new_path_norm,
            hunks,
            hunk_lines: hunk_lines_flat,
            is_new_file,
            is_deletion,
        });
    }

    if patches.is_empty() {
        anyhow::bail!("No file patches found");
    }
    Ok(patches)
}

pub(crate) fn apply_unified_diff(project_root: &Path, diff: &str) -> Result<String> {
    let patches = parse_unified_diff(diff)?;
    let mut writes: Vec<(PathBuf, String)> = Vec::new();

    for fp in patches {
        if fp.is_deletion {
            anyhow::bail!("Refusing to apply deletion patch");
        }
        let target_rel = fp
            .new_path
            .clone()
            .or(fp.old_path.clone())
            .ok_or_else(|| anyhow::anyhow!("Patch missing file path"))?;

        let target_full = project_root.join(&target_rel);
        if let Some(parent) = target_full.parent() {
            fs::create_dir_all(parent).ok();
        }

        let original = if target_full.exists() {
            fs::read_to_string(&target_full)
                .with_context(|| format!("Failed to read {}", target_full.display()))?
        } else {
            String::new()
        };
        let orig_lines = split_lines_preserve_empty(&original);
        let new_lines = apply_hunks(&orig_lines, &fp.hunks)
            .with_context(|| format!("Failed applying hunks to {}", target_rel))?;
        writes.push((target_full, join_lines(&new_lines)));
    }

    for (target_full, new_content) in writes {
        if let Some(parent) = target_full.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&target_full, &new_content)
            .with_context(|| format!("Failed to write {}", target_full.display()))?;
    }
    Ok(diff.trim().to_string())
}

pub(crate) fn apply_draft_patches(project_root: &Path, agent_output: &str) -> Result<Vec<PathBuf>> {
    let diff = extract_unified_diff_from_agent_output(agent_output).ok_or_else(|| {
        anyhow::anyhow!(
            "Fix agent output did not contain a unified diff starting with 'diff --git'"
        )
    })?;
    let patches = parse_unified_diff(&diff)?;
    let mut patched = Vec::new();
    for fp in &patches {
        let target = fp
            .new_path
            .as_deref()
            .or(fp.old_path.as_deref())
            .unwrap_or("");
        if target.is_empty() {
            anyhow::bail!("Patch contains empty path");
        }
        if target.starts_with('/') || target.contains("..") {
            anyhow::bail!("Blocked path (outside repo): {}", target);
        }
        if !target.starts_with("drafts/") {
            anyhow::bail!("Patch must target drafts/ only, got: {}", target);
        }
        if fp.is_deletion {
            anyhow::bail!("File deletion is not allowed: {}", target);
        }
        patched.push(PathBuf::from(target));
    }
    apply_unified_diff(project_root, &diff)?;
    Ok(patched)
}

fn extract_patch_path(line: &str, prefix: &str) -> Result<String> {
    let raw = line.strip_prefix(prefix).unwrap_or("").trim();
    Ok(raw.split_whitespace().next().unwrap_or("").to_string())
}

fn normalize_patch_path(p: String) -> Option<String> {
    if p == "/dev/null" {
        return None;
    }
    Some(
        p.strip_prefix("a/")
            .or_else(|| p.strip_prefix("b/"))
            .unwrap_or(&p)
            .to_string(),
    )
}

fn parse_hunk_header(line: &str) -> Result<(usize, usize)> {
    let re = regex::Regex::new(r"^@@\s+-(\d+)(?:,\d+)?\s+\+(\d+)(?:,\d+)?\s+@@").unwrap();
    let cap = re
        .captures(line)
        .ok_or_else(|| anyhow::anyhow!("Invalid hunk header: {}", line))?;
    let old_start = cap.get(1).unwrap().as_str().parse::<usize>()?;
    let new_start = cap.get(2).unwrap().as_str().parse::<usize>()?;
    Ok((old_start, new_start))
}

fn split_lines_preserve_empty(s: &str) -> Vec<String> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split_terminator('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect()
}

fn join_lines(lines: &[String]) -> String {
    lines.join("\n")
}

fn apply_hunks(orig: &[String], hunks: &[Hunk]) -> Result<Vec<String>> {
    let mut current: Vec<String> = orig.to_vec();
    let mut line_delta: isize = 0;

    for h in hunks {
        let expected_start = expected_hunk_start(h.old_start, line_delta);
        let (start, trim_leading, trim_trailing) =
            find_hunk_application(&current, h, expected_start).ok_or_else(|| {
                let preferred = h.old_start.saturating_sub(1);
                anyhow::anyhow!(
                    "Could not locate hunk context (preferred_start={}, expected_start={}, pattern_len={})",
                    preferred,
                    expected_start,
                    hunk_preimage_pattern_len(h)
                )
            })?;
        let effective_end = h.lines.len().saturating_sub(trim_trailing);
        let effective_lines = &h.lines[trim_leading..effective_end];

        let mut pos = start;
        let mut segment: Vec<String> = Vec::new();
        for hl in effective_lines {
            match hl.kind {
                HunkLineKind::Context => {
                    let line = current
                        .get(pos)
                        .ok_or_else(|| anyhow::anyhow!("Context line beyond EOF at pos {}", pos))?;
                    if line != &hl.text {
                        anyhow::bail!(
                            "Context mismatch at pos {}: expected {:?}, found {:?}",
                            pos,
                            hl.text,
                            line
                        );
                    }
                    segment.push(line.clone());
                    pos += 1;
                }
                HunkLineKind::Remove => {
                    let line = current
                        .get(pos)
                        .ok_or_else(|| anyhow::anyhow!("Remove line beyond EOF at pos {}", pos))?;
                    if line != &hl.text {
                        anyhow::bail!(
                            "Remove mismatch at pos {}: expected {:?}, found {:?}",
                            pos,
                            hl.text,
                            line
                        );
                    }
                    pos += 1;
                }
                HunkLineKind::Add => {
                    segment.push(hl.text.clone());
                }
            }
        }

        let mut next: Vec<String> = Vec::with_capacity(current.len() + segment.len());
        next.extend_from_slice(&current[..start]);
        next.extend(segment);
        next.extend_from_slice(&current[pos..]);
        current = next;
        line_delta += net_hunk_line_delta(h);
    }

    Ok(current)
}

fn expected_hunk_start(old_start: usize, line_delta: isize) -> usize {
    let base = old_start.saturating_sub(1) as isize + line_delta;
    base.max(0) as usize
}

fn net_hunk_line_delta(h: &Hunk) -> isize {
    let adds = h
        .lines
        .iter()
        .filter(|hl| hl.kind == HunkLineKind::Add)
        .count() as isize;
    let removes = h
        .lines
        .iter()
        .filter(|hl| hl.kind == HunkLineKind::Remove)
        .count() as isize;
    adds - removes
}

fn hunk_preimage_pattern(h: &Hunk) -> Vec<&str> {
    hunk_preimage_pattern_for_lines(&h.lines)
}

fn hunk_preimage_pattern_len(h: &Hunk) -> usize {
    hunk_preimage_pattern(h).len()
}

fn hunk_preimage_pattern_for_lines(lines: &[HunkLine]) -> Vec<&str> {
    let mut pattern: Vec<&str> = Vec::new();
    for hl in lines {
        match hl.kind {
            HunkLineKind::Context | HunkLineKind::Remove => pattern.push(hl.text.as_str()),
            HunkLineKind::Add => {}
        }
    }
    pattern
}

fn find_hunk_application(lines: &[String], hunk: &Hunk, expected_start: usize) -> Option<(usize, usize, usize)> {
    let leading_context = hunk
        .lines
        .iter()
        .take_while(|hl| hl.kind == HunkLineKind::Context)
        .count();
    let trailing_context = hunk
        .lines
        .iter()
        .rev()
        .take_while(|hl| hl.kind == HunkLineKind::Context)
        .count();
    let max_fuzz = leading_context + trailing_context;

    for total_trim in 0..=max_fuzz {
        let min_leading_trim = total_trim.saturating_sub(trailing_context);
        let max_leading_trim = total_trim.min(leading_context);
        for trim_leading in min_leading_trim..=max_leading_trim {
            let trim_trailing = total_trim - trim_leading;
            let effective_end = hunk.lines.len().saturating_sub(trim_trailing);
            if trim_leading > effective_end {
                continue;
            }
            let effective_lines = &hunk.lines[trim_leading..effective_end];
            let pattern = hunk_preimage_pattern_for_lines(effective_lines);
            let adjusted_expected = expected_start.saturating_add(trim_leading);

            if let Some(start) = find_hunk_start_near(lines, &pattern, adjusted_expected, 0) {
                return Some((start, trim_leading, trim_trailing));
            }
            if let Some(start) = find_hunk_start_near(lines, &pattern, adjusted_expected, 8) {
                return Some((start, trim_leading, trim_trailing));
            }
            if let Some(start) = find_hunk_start_near(lines, &pattern, adjusted_expected, 32) {
                return Some((start, trim_leading, trim_trailing));
            }
        }
    }

    for total_trim in 0..=max_fuzz {
        let min_leading_trim = total_trim.saturating_sub(trailing_context);
        let max_leading_trim = total_trim.min(leading_context);
        for trim_leading in min_leading_trim..=max_leading_trim {
            let trim_trailing = total_trim - trim_leading;
            let effective_end = hunk.lines.len().saturating_sub(trim_trailing);
            if trim_leading > effective_end {
                continue;
            }
            let effective_lines = &hunk.lines[trim_leading..effective_end];
            let pattern = hunk_preimage_pattern_for_lines(effective_lines);
            if let Some(start) = find_hunk_start_anywhere(lines, &pattern) {
                return Some((start, trim_leading, trim_trailing));
            }
        }
    }

    None
}

fn find_hunk_start_near(
    lines: &[String],
    pattern: &[&str],
    expected: usize,
    fuzz: usize,
) -> Option<usize> {
    if pattern.is_empty() {
        return Some(expected.min(lines.len()));
    }
    if lines.len() < pattern.len() {
        return None;
    }

    let try_at = |i: usize| -> bool {
        if i + pattern.len() > lines.len() {
            return false;
        }
        for (j, needle) in pattern.iter().enumerate() {
            if lines[i + j].as_str() != *needle {
                return false;
            }
        }
        true
    };

    let expected = expected.min(lines.len().saturating_sub(pattern.len()));
    if try_at(expected) {
        return Some(expected);
    }

    for distance in 1..=fuzz {
        if let Some(i) = expected.checked_sub(distance) {
            if try_at(i) {
                return Some(i);
            }
        }
        let i = expected + distance;
        if i <= lines.len().saturating_sub(pattern.len()) && try_at(i) {
            return Some(i);
        }
    }

    None
}

fn find_hunk_start_anywhere(lines: &[String], pattern: &[&str]) -> Option<usize> {
    if pattern.is_empty() {
        return Some(0);
    }
    if lines.len() < pattern.len() {
        return None;
    }

    let try_at = |i: usize| -> bool {
        if i + pattern.len() > lines.len() {
            return false;
        }
        for (j, needle) in pattern.iter().enumerate() {
            if lines[i + j].as_str() != *needle {
                return false;
            }
        }
        true
    };

    for i in 0..=lines.len().saturating_sub(pattern.len()) {
        if try_at(i) {
            return Some(i);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{apply_hunks, apply_unified_diff, Hunk, HunkLine, HunkLineKind};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn apply_hunks_tolerates_stale_leading_context() {
        let original = vec![
            "fn run() {".to_string(),
            "    let x = 1;".to_string(),
            "    let y = 2;".to_string(),
            "    println!(\"{}\", y);".to_string(),
            "}".to_string(),
        ];
        let hunks = vec![Hunk {
            old_start: 1,
            lines: vec![
                HunkLine {
                    kind: HunkLineKind::Context,
                    text: "fn execute() {".to_string(),
                },
                HunkLine {
                    kind: HunkLineKind::Remove,
                    text: "    let y = 2;".to_string(),
                },
                HunkLine {
                    kind: HunkLineKind::Add,
                    text: "    let y = 3;".to_string(),
                },
                HunkLine {
                    kind: HunkLineKind::Context,
                    text: "    println!(\"{}\", y);".to_string(),
                },
                HunkLine {
                    kind: HunkLineKind::Context,
                    text: "}".to_string(),
                },
            ],
        }];

        let updated = apply_hunks(&original, &hunks).expect("hunk should apply");

        assert_eq!(
            updated,
            vec![
                "fn run() {".to_string(),
                "    let x = 1;".to_string(),
                "    let y = 3;".to_string(),
                "    println!(\"{}\", y);".to_string(),
                "}".to_string(),
            ]
        );
    }

    #[test]
    fn apply_hunks_tolerates_stale_trailing_context() {
        let original = vec![
            "fn run() {".to_string(),
            "    let y = 2;".to_string(),
            "    println!(\"{}\", y);".to_string(),
            "}".to_string(),
        ];
        let hunks = vec![Hunk {
            old_start: 1,
            lines: vec![
                HunkLine {
                    kind: HunkLineKind::Context,
                    text: "fn run() {".to_string(),
                },
                HunkLine {
                    kind: HunkLineKind::Remove,
                    text: "    let y = 2;".to_string(),
                },
                HunkLine {
                    kind: HunkLineKind::Add,
                    text: "    let y = 4;".to_string(),
                },
                HunkLine {
                    kind: HunkLineKind::Context,
                    text: "    eprintln!(\"{}\", y);".to_string(),
                },
            ],
        }];

        let updated = apply_hunks(&original, &hunks).expect("hunk should apply");

        assert_eq!(
            updated,
            vec![
                "fn run() {".to_string(),
                "    let y = 4;".to_string(),
                "    println!(\"{}\", y);".to_string(),
                "}".to_string(),
            ]
        );
    }

    #[test]
    fn apply_hunks_uses_running_line_delta_between_hunks() {
        let original = vec![
            "start".to_string(),
            "keep".to_string(),
            "target one".to_string(),
            "middle".to_string(),
            "target two".to_string(),
            "end".to_string(),
        ];
        let hunks = vec![
            Hunk {
                old_start: 2,
                lines: vec![
                    HunkLine {
                        kind: HunkLineKind::Context,
                        text: "keep".to_string(),
                    },
                    HunkLine {
                        kind: HunkLineKind::Add,
                        text: "inserted".to_string(),
                    },
                    HunkLine {
                        kind: HunkLineKind::Context,
                        text: "target one".to_string(),
                    },
                ],
            },
            Hunk {
                old_start: 5,
                lines: vec![
                    HunkLine {
                        kind: HunkLineKind::Context,
                        text: "middle".to_string(),
                    },
                    HunkLine {
                        kind: HunkLineKind::Remove,
                        text: "target two".to_string(),
                    },
                    HunkLine {
                        kind: HunkLineKind::Add,
                        text: "target two updated".to_string(),
                    },
                    HunkLine {
                        kind: HunkLineKind::Context,
                        text: "end".to_string(),
                    },
                ],
            },
        ];

        let updated = apply_hunks(&original, &hunks).expect("hunks should apply");

        assert_eq!(
            updated,
            vec![
                "start".to_string(),
                "keep".to_string(),
                "inserted".to_string(),
                "target one".to_string(),
                "middle".to_string(),
                "target two updated".to_string(),
                "end".to_string(),
            ]
        );
    }

    #[test]
    fn apply_unified_diff_is_transactional_across_files() {
        let root = make_temp_test_dir("patch_service_transaction");
        let file_a = root.join("src/a.rs");
        let file_b = root.join("src/b.rs");
        fs::create_dir_all(file_a.parent().expect("parent")).expect("create dir");
        fs::write(&file_a, "alpha\nbeta\ngamma\n").expect("write a");
        fs::write(&file_b, "one\ntwo\nthree\n").expect("write b");

        let diff = r#"diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,3 @@
 alpha
-beta
+beta updated
 gamma
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -1,3 +1,3 @@
 one
-missing
+two updated
 three
"#;

        let err = apply_unified_diff(&root, diff).expect_err("second file should fail");
        assert!(err.to_string().contains("src/b.rs"));
        assert_eq!(
            fs::read_to_string(&file_a).expect("read a"),
            "alpha\nbeta\ngamma\n"
        );
        assert_eq!(
            fs::read_to_string(&file_b).expect("read b"),
            "one\ntwo\nthree\n"
        );

        fs::remove_dir_all(&root).ok();
    }

    fn make_temp_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{}_{}", prefix, nanos));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
