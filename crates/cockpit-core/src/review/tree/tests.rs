use super::*;
use crate::review::LineKind::{Add, Context, Del};

fn file(name: &str, add: usize, del: usize) -> FileDiff {
    let mut lines: Vec<(u32, _, &str)> = vec![(1, Context, "x")];
    lines.extend((0..add).map(|i| (2 + i as u32, Add, "+")));
    lines.extend((0..del).map(|i| (2 + i as u32, Del, "-")));
    FileDiff::new(name, &lines)
}

fn names(rows: &[TreeRow]) -> Vec<(usize, &str)> {
    rows.iter().map(|r| (r.depth, r.name.as_str())).collect()
}

#[test]
fn folders_come_first_and_files_follow_sorted() {
    let files = vec![
        file("README.md", 1, 0),
        file("src/b.rs", 2, 1),
        file("src/a.rs", 3, 0),
        file("src/ui/x.css", 1, 1),
        file("Cargo.toml", 0, 1),
    ];
    let rows = file_tree(&files);
    assert_eq!(
        names(&rows),
        vec![
            (0, "src"),
            (1, "ui"),
            (2, "x.css"),
            (1, "a.rs"),
            (1, "b.rs"),
            (0, "Cargo.toml"),
            (0, "README.md"),
        ]
    );
}

#[test]
fn a_folder_shows_the_sum_of_its_files() {
    let files = vec![file("src/a.rs", 3, 0), file("src/ui/x.css", 1, 2)];
    let rows = file_tree(&files);
    let src = &rows[0];
    assert_eq!((src.additions, src.deletions), (4, 2));
    assert_eq!(src.file, None);
    let ui = rows.iter().find(|r| r.path == "src/ui").unwrap();
    assert_eq!((ui.additions, ui.deletions), (1, 2));
}

#[test]
fn a_file_row_points_to_its_position_and_counts() {
    let files = vec![file("b.txt", 1, 0), file("a.txt", 5, 4)];
    let rows = file_tree(&files);
    assert_eq!(rows[0].path, "a.txt");
    assert_eq!(rows[0].file, Some(1));
    assert_eq!((rows[0].additions, rows[0].deletions), (5, 4));
    assert_eq!(rows[1].file, Some(0));
}

#[test]
fn no_files_no_rows() {
    assert!(file_tree(&[]).is_empty());
}
