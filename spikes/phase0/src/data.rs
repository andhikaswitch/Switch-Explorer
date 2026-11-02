//! File listings for the spike: a synthetic 100k-row set (performance) and a real fixture folder
//! (drag & drop, native menu). Rows are stored compactly — names in one arena, extensions
//! interned — and display strings are formatted lazily per visible row (ARCHITECTURE.md §9).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::bail;
use gpui_kit::SharedString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    DataTable,
    Uniform,
}

impl Variant {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "datatable" => Ok(Self::DataTable),
            "uniform" => Ok(Self::Uniform),
            other => bail!("unknown table variant {other} (datatable|uniform)"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::DataTable => "datatable",
            Self::Uniform => "uniform",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Fixture,
    Synthetic,
}

impl Source {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "fixture" => Ok(Self::Fixture),
            "synthetic" => Ok(Self::Synthetic),
            other => bail!("unknown data source {other} (fixture|synthetic)"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Fixture => "fixture",
            Self::Synthetic => "synthetic",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    name_at: u32,
    name_len: u16,
    ext: u16,
    is_dir: bool,
    size: u64,
    modified: i64,
}

#[derive(Debug)]
pub struct Dataset {
    pub root: Option<PathBuf>,
    entries: Vec<Entry>,
    names: String,
    exts: Vec<String>,
}

impl Default for Dataset {
    fn default() -> Self {
        Self {
            root: None,
            entries: Vec::new(),
            names: String::new(),
            // Index 0 means "no extension".
            exts: vec![String::new()],
        }
    }
}

impl Dataset {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn name(&self, ix: usize) -> &str {
        let entry = &self.entries[ix];
        let start = entry.name_at as usize;
        &self.names[start..start + entry.name_len as usize]
    }

    pub fn is_dir(&self, ix: usize) -> bool {
        self.entries[ix].is_dir
    }

    /// Real path of a row; synthetic rows have none.
    pub fn path(&self, ix: usize) -> Option<PathBuf> {
        self.root.as_ref().map(|root| root.join(self.name(ix)))
    }

    /// Display strings for one row: name, modified, kind, size.
    pub fn cells(&self, ix: usize) -> [SharedString; 4] {
        let entry = self.entries[ix];
        let kind = if entry.is_dir {
            String::from("Folder")
        } else if entry.ext == 0 {
            String::from("File")
        } else {
            format!("{} File", self.exts[entry.ext as usize].to_uppercase())
        };
        let size = if entry.is_dir {
            String::new()
        } else {
            format_size(entry.size)
        };
        [
            SharedString::from(self.name(ix).to_string()),
            SharedString::from(format_time(entry.modified)),
            SharedString::from(kind),
            SharedString::from(size),
        ]
    }

    pub fn heap_bytes(&self) -> usize {
        self.entries.capacity() * size_of::<Entry>() + self.names.capacity()
    }

    fn push(&mut self, name: &str, is_dir: bool, size: u64, modified: i64) {
        let Ok(name_len) = u16::try_from(name.len()) else {
            return;
        };
        let Ok(name_at) = u32::try_from(self.names.len()) else {
            return;
        };
        let ext = if is_dir { 0 } else { self.ext_id(name) };
        self.names.push_str(name);
        self.entries.push(Entry {
            name_at,
            name_len,
            ext,
            is_dir,
            size,
            modified,
        });
    }

    fn ext_id(&mut self, name: &str) -> u16 {
        let Some((_, ext)) = name.rsplit_once('.') else {
            return 0;
        };
        if ext.is_empty() || ext.len() > 8 {
            return 0;
        }
        let ext = ext.to_ascii_lowercase();
        if let Some(ix) = self.exts.iter().position(|e| *e == ext) {
            return ix as u16;
        }
        self.exts.push(ext);
        (self.exts.len() - 1) as u16
    }

    /// Deterministic names with mixed scripts, lengths and extensions.
    pub fn synthetic(rows: usize) -> Self {
        let mut ds = Self {
            entries: Vec::with_capacity(rows),
            names: String::with_capacity(rows * 30),
            ..Self::default()
        };
        let mut rng = Lcg(0x5EED_2026);
        let now = unix_now();
        let mut name = String::new();
        for i in 0..rows {
            name.clear();
            let is_dir = rng.next().is_multiple_of(10);
            synth_name(&mut name, i, is_dir, &mut rng);
            let size = if is_dir {
                0
            } else {
                (rng.next() % (4u64 << 30)) >> (rng.next() % 24)
            };
            let modified = now - (rng.next() % (3 * 365 * 86_400)) as i64;
            ds.push(&name, is_dir, size, modified);
        }
        ds
    }

    /// One directory level, folders first, case-insensitive by name.
    pub fn load_dir(root: &Path) -> std::io::Result<Self> {
        let mut items: Vec<(String, bool, u64, i64)> = Vec::new();
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            let name = entry.file_name().to_string_lossy().into_owned();
            items.push((name, meta.is_dir(), meta.len(), modified));
        }
        items.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
        });
        let mut ds = Self {
            root: Some(root.to_path_buf()),
            ..Self::default()
        };
        for (name, is_dir, size, modified) in items {
            ds.push(&name, is_dir, size, modified);
        }
        Ok(ds)
    }
}

/// `%TEMP%\sx-fixtures\phase0` (NAMING.md: fixtures live under `%TEMP%\sx-fixtures`).
pub fn fixture_dir() -> PathBuf {
    std::env::temp_dir().join("sx-fixtures").join("phase0")
}

/// Creates the small real-file fixture used for drag & drop and native menu tests.
pub fn ensure_fixture() -> std::io::Result<PathBuf> {
    let dir = fixture_dir();
    std::fs::create_dir_all(dir.join("Folder Proyek"))?;
    std::fs::create_dir_all(dir.join("写真 京都"))?;
    let text_files: [(&str, &str); 9] = [
        (
            "Laporan Keuangan 2026.txt",
            "Laporan uji Switch Explorer Fase 0.\n",
        ),
        (
            "catatan.md",
            "# Catatan\n\n- uji drag ke Chrome\n- uji menu native\n",
        ),
        (
            "data.csv",
            "id,nama,peran\n1,Raka,mahasiswa\n2,Dina,developer\n3,Kai,ricer\n",
        ),
        ("config.json", "{ \"spike\": \"phase0\", \"ok\": true }\n"),
        (
            "halaman.html",
            "<!doctype html><title>spike</title><p>Halo dari fixture.</p>\n",
        ),
        ("Tugas Kuliah Semester 5 - Bab 3.txt", "Isi tugas.\n"),
        ("Тест документ.txt", "Cyrillic name test.\n"),
        ("Folder Proyek/readme.txt", "Isi folder proyek.\n"),
        ("写真 京都/catatan perjalanan.txt", "Kyoto.\n"),
    ];
    for (name, body) in text_files {
        let path = dir.join(name);
        if !path.exists() {
            std::fs::write(&path, body)?;
        }
    }
    let images = ["gambar-biru.bmp", "gambar-hijau.bmp", "写真_京都_001.bmp"];
    for (hue, name) in images.iter().enumerate() {
        let path = dir.join(name);
        if !path.exists() {
            std::fs::write(&path, bmp(96, 64, hue as u8))?;
        }
    }
    Ok(dir)
}

/// A 24-bit uncompressed BMP gradient (valid image for upload/preview tests).
fn bmp(width: u32, height: u32, hue: u8) -> Vec<u8> {
    let row = width * 3;
    let pad = (4 - row % 4) % 4;
    let data_len = (row + pad) * height;
    let file_len = 54 + data_len;
    let mut v = Vec::with_capacity(file_len as usize);
    v.extend_from_slice(b"BM");
    v.extend_from_slice(&file_len.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(&54u32.to_le_bytes());
    v.extend_from_slice(&40u32.to_le_bytes());
    v.extend_from_slice(&(width as i32).to_le_bytes());
    v.extend_from_slice(&(height as i32).to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&24u16.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(&data_len.to_le_bytes());
    v.extend_from_slice(&2835u32.to_le_bytes());
    v.extend_from_slice(&2835u32.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    for y in 0..height {
        for x in 0..width {
            let (gx, gy) = ((x * 255 / width) as u8, (y * 255 / height) as u8);
            let pixel = match hue {
                0 => [230, gx, gy / 2],
                1 => [gy / 2, 200, gx],
                _ => [gx, gy, 220],
            };
            v.extend_from_slice(&pixel);
        }
        v.extend(std::iter::repeat_n(0u8, pad as usize));
    }
    v
}

const DEPTS: [&str; 6] = ["Keuangan", "Pemasaran", "Produksi", "SDM", "IT", "Riset"];
const PLACES: [&str; 5] = ["京都", "大阪", "東京", "札幌", "奈良"];
const WORDS: [&str; 8] = [
    "alpha",
    "nusantara",
    "cerulean",
    "switch",
    "explorer",
    "tugas",
    "proyek",
    "arsip",
];

fn synth_name(out: &mut String, i: usize, is_dir: bool, rng: &mut Lcg) {
    let r = rng.next();
    let word = WORDS[(r >> 8) as usize % WORDS.len()];
    let place = PLACES[(r >> 11) as usize % PLACES.len()];
    let _ = if is_dir {
        match r % 4 {
            0 => write!(out, "Proyek {word} {i}"),
            1 => write!(out, "Arsip {}-{:02} #{i}", 2020 + r % 7, 1 + (r >> 4) % 12),
            2 => write!(out, "写真 {place} {i}"),
            _ => write!(out, "folder-{word}-{i}"),
        }
    } else {
        match r % 9 {
            0 => write!(
                out,
                "Laporan {} {} Q{} v{i}.xlsx",
                DEPTS[(r >> 14) as usize % DEPTS.len()],
                2020 + (r >> 17) % 7,
                1 + (r >> 20) % 4
            ),
            1 => write!(
                out,
                "IMG_{}{:02}{:02}_{i:06}.jpg",
                2021 + (r >> 8) % 5,
                1 + (r >> 12) % 12,
                1 + (r >> 16) % 28
            ),
            2 => write!(out, "写真_{place}_{i:05}.png"),
            3 => write!(out, "Тест документ {i}.docx"),
            4 => write!(out, "🎵 Playlist {word} #{i}.m3u"),
            5 => write!(
                out,
                "Tugas Kuliah Semester {} - Bab {} ({i}).pdf",
                1 + (r >> 8) % 8,
                1 + (r >> 12) % 9
            ),
            6 => write!(out, "notes-{word}-{i}.md"),
            7 => write!(
                out,
                "A very long file name used to check text truncation in the name column, item {i}, final revision.txt"
            ),
            _ => write!(out, "data_{:x}.csv", r ^ i as u64),
        }
    };
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 17
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// UTC "YYYY-MM-DD HH:MM" (the spike skips time zones).
fn format_time(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60
    )
}

/// Howard Hinnant's days-to-civil conversion.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(format_time(1_798_761_600), "2027-01-01 00:00");
    }

    #[test]
    fn synthetic_rows_are_compact() {
        let ds = Dataset::synthetic(1000);
        assert_eq!(ds.len(), 1000);
        assert!(ds.heap_bytes() < 1000 * 140);
        assert!(!ds.name(999).is_empty());
    }

    #[test]
    fn sizes() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
    }
}
