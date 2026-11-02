//! Test folders under `%TEMP%\sx-fixtures` (PERFORMANCE.md §6): a flat folder of `n` files,
//! a deep tree, Unicode names and paths longer than 260 characters. Files get realistic sizes
//! through `set_len`, so creating 100k entries stays fast.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context as _;

const NAMES: [&str; 8] = [
    "Laporan Keuangan {i} final.xlsx",
    "IMG_2026{i:06}.jpg",
    "notes-{i}.md",
    "写真_京都_{i}.png",
    "Тест документ {i}.docx",
    "🎵 Playlist {i}.m3u",
    "Tugas Kuliah Bab {i}.pdf",
    "data_{i}.csv",
];

pub fn root() -> PathBuf {
    std::env::temp_dir().join("sx-fixtures")
}

pub fn run(count: usize) -> anyhow::Result<()> {
    let started = Instant::now();
    let root = root();
    let flat = root.join(format!("flat-{count}"));
    flat_folder(&flat, count)?;
    deep_tree(&root.join("deep-30"), 30)?;
    unicode(&root.join("unicode"))?;
    long_paths(&root.join("long-paths"))?;
    println!(
        "fixtures ready in {:.1} s under {}",
        started.elapsed().as_secs_f64(),
        root.display()
    );
    Ok(())
}

fn flat_folder(dir: &Path, count: usize) -> anyhow::Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let existing = fs::read_dir(dir)?.count();
    if existing >= count {
        println!("{} already has {existing} entries", dir.display());
        return Ok(());
    }
    for i in 0..count {
        let name = if i % 10 == 0 {
            format!("Folder {i}")
        } else {
            NAMES[i % NAMES.len()]
                .replace("{i:06}", &format!("{i:06}"))
                .replace("{i}", &i.to_string())
        };
        let path = dir.join(name);
        if i % 10 == 0 {
            fs::create_dir_all(&path)?;
        } else {
            let file = fs::File::create(&path)
                .with_context(|| format!("cannot create {}", path.display()))?;
            // Deterministic spread of sizes from bytes to megabytes (sparse allocation).
            let size = ((i as u64 * 2_654_435_761) % (8 << 20)) >> (i % 12);
            file.set_len(size)?;
        }
    }
    println!("created {count} entries in {}", dir.display());
    Ok(())
}

fn deep_tree(dir: &Path, depth: usize) -> anyhow::Result<()> {
    let mut path = dir.to_path_buf();
    for level in 1..=depth {
        path.push(format!("level-{level:02}"));
    }
    fs::create_dir_all(&path)?;
    fs::write(path.join("bottom.txt"), "the bottom of the tree\n")?;
    Ok(())
}

fn unicode(dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dir)?;
    for name in [
        "Ringkasan rapat — Januari.txt",
        "日本語のファイル名.txt",
        "emoji 🚀✨ test.txt",
        "Ελληνικά.txt",
        "עברית.txt",
        "file with  two spaces.txt",
        "file10.txt",
        "file2.txt",
        "File1.txt",
    ] {
        fs::write(dir.join(name), name.as_bytes())?;
    }
    Ok(())
}

/// Nested folders until the full path passes 300 characters; std adds the `\\?\` prefix.
fn long_paths(dir: &Path) -> anyhow::Result<()> {
    let mut path = dir.to_path_buf();
    while path.as_os_str().len() < 300 {
        path.push("a-rather-long-folder-name-to-pass-max-path");
    }
    fs::create_dir_all(&path)?;
    fs::write(path.join("deep file beyond MAX_PATH.txt"), "ok\n")?;
    Ok(())
}
