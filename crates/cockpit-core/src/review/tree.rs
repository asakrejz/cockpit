//! The file tree of the review panel: the changed files as indented rows with their counts.

use std::collections::BTreeMap;

use super::FileDiff;

/// Diff lines shown per file before „Mehr anzeigen“.
pub const SHOWN_LINES: usize = 300;

/// A row of the tree: a folder or a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub depth: usize,
    /// Last part of the path.
    pub name: String,
    /// Full path of the folder or file.
    pub path: String,
    /// Position in `Review::files` for a file, `None` for a folder.
    pub file: Option<usize>,
    pub additions: usize,
    pub deletions: usize,
}

/// Folders first, then files, each sorted by name; a folder shows the sum of what is below it.
pub fn file_tree(files: &[FileDiff]) -> Vec<TreeRow> {
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by(|&a, &b| compare(&files[a].name, &files[b].name));

    let mut folders: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for file in files {
        let (a, d) = file.counts();
        let mut prefix = String::new();
        let parts: Vec<&str> = file.name.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            let sum = folders.entry(prefix.clone()).or_default();
            sum.0 += a;
            sum.1 += d;
        }
    }

    let mut rows = Vec::new();
    let mut open: Vec<&str> = Vec::new();
    for index in order {
        let file = &files[index];
        let parts: Vec<&str> = file.name.split('/').collect();
        let (dirs, name) = parts.split_at(parts.len() - 1);
        let same = open.iter().zip(dirs).take_while(|(a, b)| a == b).count();
        open.truncate(same);
        for depth in same..dirs.len() {
            open.push(dirs[depth]);
            let path = dirs[..=depth].join("/");
            let (additions, deletions) = folders.get(&path).copied().unwrap_or_default();
            rows.push(TreeRow {
                depth,
                name: dirs[depth].into(),
                path,
                file: None,
                additions,
                deletions,
            });
        }
        let (additions, deletions) = file.counts();
        rows.push(TreeRow {
            depth: dirs.len(),
            name: name[0].into(),
            path: file.name.clone(),
            file: Some(index),
            additions,
            deletions,
        });
    }
    rows
}

/// Folders before files at the same level, otherwise by name.
fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    let (pa, pb): (Vec<&str>, Vec<&str>) = (a.split('/').collect(), b.split('/').collect());
    for (i, (x, y)) in pa.iter().zip(&pb).enumerate() {
        if x == y {
            continue;
        }
        let (a_dir, b_dir) = (i + 1 < pa.len(), i + 1 < pb.len());
        return b_dir.cmp(&a_dir).then_with(|| x.cmp(y));
    }
    pa.len().cmp(&pb.len())
}

#[cfg(test)]
mod tests;
