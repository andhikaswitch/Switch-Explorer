//! `DirSnapshot` (ARCHITECTURE.md §9): one folder listing in compact form. Names live in one
//! UTF-8 arena (plus a lowercase arena for filtering), extensions are interned, and the sorted
//! and filtered orders are `u32` index vectors, so 100k entries stay around 11 MB.

use std::cmp::Ordering;

use sx_core::EntryKind;
use sx_win::RawEntry;

/// Natural sort keys for one sort pass. They come from Windows (linguistic order with digits
/// by value, exactly Explorer's order) and are dropped after the sort, so they cost no memory
/// between sorts.
struct NameKeys {
    bytes: Vec<u8>,
    spans: Vec<(u32, u32)>,
}

impl NameKeys {
    fn build(snapshot: &DirSnapshot) -> Self {
        let mut bytes = Vec::with_capacity(snapshot.names.len() * 3);
        let spans = (0..snapshot.entries.len() as u32)
            .map(|ix| {
                let range = sx_win::natural_sort_key(snapshot.name(ix), &mut bytes);
                (range.start as u32, range.end as u32)
            })
            .collect();
        Self { bytes, spans }
    }

    fn get(&self, ix: u32) -> &[u8] {
        let (start, end) = self.spans[ix as usize];
        &self.bytes[start as usize..end as usize]
    }
}

const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;

#[derive(Clone, Copy, Debug)]
struct EntryRec {
    name_at: u32,
    name_len: u32,
    /// Index into `exts`; 0 = no extension.
    ext: u16,
    kind: EntryKind,
    attrs: u32,
    size: u64,
    modified: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Column {
    #[default]
    Name,
    Modified,
    Type,
    Size,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub column: Column,
    pub descending: bool,
    /// `view.folders_first`
    pub folders_first: bool,
    /// `view.natural_sort`
    pub natural: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            column: Column::Name,
            descending: false,
            folders_first: true,
            natural: true,
        }
    }
}

/// Which entries are shown (`view.show_hidden`, `view.show_system`, the filter box).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewOptions {
    pub show_hidden: bool,
    pub show_system: bool,
    /// Case-insensitive substring; empty = no filter.
    pub filter: String,
}

/// Borrowed view of one entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryView<'a> {
    pub index: u32,
    pub name: &'a str,
    /// Lowercase extension without the dot ("" for none and for folders).
    pub ext: &'a str,
    pub kind: EntryKind,
    pub attrs: u32,
    pub size: u64,
    /// FILETIME of the last write.
    pub modified: i64,
}

#[derive(Debug)]
pub struct DirSnapshot {
    entries: Vec<EntryRec>,
    names: String,
    names_lower: String,
    exts: Vec<String>,
    /// Entry indices in sort order.
    order: Vec<u32>,
    /// `order` after hidden/system/filter rules.
    visible: Vec<u32>,
    sort: SortSpec,
    view: ViewOptions,
}

impl Default for DirSnapshot {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            names: String::new(),
            names_lower: String::new(),
            exts: vec![String::new()],
            order: Vec::new(),
            visible: Vec::new(),
            sort: SortSpec::default(),
            view: ViewOptions::default(),
        }
    }
}

impl DirSnapshot {
    pub fn new(sort: SortSpec, view: ViewOptions) -> Self {
        Self {
            sort,
            view,
            ..Self::default()
        }
    }

    /// All entries, including hidden ones.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries shown with the current options.
    pub fn visible_len(&self) -> usize {
        self.visible.len()
    }

    /// The entry at a visible row.
    pub fn row(&self, row: usize) -> Option<EntryView<'_>> {
        self.visible.get(row).map(|&ix| self.view_of(ix))
    }

    pub fn entry(&self, index: u32) -> Option<EntryView<'_>> {
        ((index as usize) < self.entries.len()).then(|| self.view_of(index))
    }

    /// Visible row of an entry, e.g. to keep the selection after re-sorting.
    pub fn row_of(&self, index: u32) -> Option<usize> {
        self.visible.iter().position(|&ix| ix == index)
    }

    pub fn sort_spec(&self) -> SortSpec {
        self.sort
    }

    /// Bytes held by the snapshot (for the memory meter).
    pub fn heap_bytes(&self) -> usize {
        self.entries.capacity() * size_of::<EntryRec>()
            + self.names.capacity()
            + self.names_lower.capacity()
            + (self.order.capacity() + self.visible.capacity()) * size_of::<u32>()
    }

    /// Appends an enumeration batch. New entries go to the end of the current order; call
    /// [`Self::sort`] once enumeration finishes (the first batch renders unsorted-but-fast).
    pub fn push_batch(&mut self, batch: &[RawEntry]) {
        self.entries.reserve(batch.len());
        for raw in batch {
            let Ok(index) = u32::try_from(self.entries.len()) else {
                return;
            };
            let ext = if raw.kind.is_container() {
                0
            } else {
                self.intern_ext(&raw.name)
            };
            self.entries.push(EntryRec {
                name_at: self.names.len() as u32,
                name_len: raw.name.len() as u32,
                ext,
                kind: raw.kind,
                attrs: raw.attrs,
                size: raw.size,
                modified: raw.modified,
            });
            self.names.push_str(&raw.name);
            // Same byte length is not guaranteed for lowercase; keep the arenas parallel by
            // storing lowercase per entry at its own offset map.
            self.names_lower.push_str(&lowercase_same_len(&raw.name));
            self.order.push(index);
            if self.passes(index) {
                self.visible.push(index);
            }
        }
    }

    /// Re-sorts all entries (stable) and rebuilds the visible list.
    pub fn sort(&mut self, spec: SortSpec) {
        self.sort = spec;
        let keys = spec.natural.then(|| NameKeys::build(self));
        let mut order = std::mem::take(&mut self.order);
        order.sort_by(|&a, &b| self.compare(a, b, keys.as_ref()));
        self.order = order;
        self.rebuild_visible();
    }

    pub fn set_view(&mut self, mut view: ViewOptions) {
        view.filter = view.filter.to_lowercase();
        self.view = view;
        self.rebuild_visible();
    }

    fn rebuild_visible(&mut self) {
        let visible: Vec<u32> = self
            .order
            .iter()
            .copied()
            .filter(|&ix| self.passes(ix))
            .collect();
        self.visible = visible;
    }

    fn passes(&self, ix: u32) -> bool {
        let rec = &self.entries[ix as usize];
        if !self.view.show_hidden && rec.attrs & FILE_ATTRIBUTE_HIDDEN != 0 {
            return false;
        }
        // Protected OS files are hidden+system; they need both switches.
        let protected = rec.attrs & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM)
            == FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM;
        if protected && !self.view.show_system {
            return false;
        }
        self.view.filter.is_empty() || self.lower(ix).contains(&self.view.filter)
    }

    fn compare(&self, a: u32, b: u32, keys: Option<&NameKeys>) -> Ordering {
        let (ra, rb) = (&self.entries[a as usize], &self.entries[b as usize]);
        if self.sort.folders_first {
            let folder = |r: &EntryRec| !r.kind.is_container();
            let order = folder(ra).cmp(&folder(rb));
            if order != Ordering::Equal {
                return order; // Folders stay first in both directions.
            }
        }
        let by_name = || match keys {
            Some(keys) => keys.get(a).cmp(keys.get(b)),
            None => self.lower(a).cmp(self.lower(b)),
        };
        let primary = match self.sort.column {
            Column::Name => by_name(),
            Column::Modified => ra.modified.cmp(&rb.modified),
            Column::Size => ra.size.cmp(&rb.size),
            Column::Type => self.exts[ra.ext as usize].cmp(&self.exts[rb.ext as usize]),
        };
        let primary = if self.sort.descending {
            primary.reverse()
        } else {
            primary
        };
        // Ties are broken by name, then by enumeration order (stable and deterministic).
        primary.then_with(by_name).then(a.cmp(&b))
    }

    fn name(&self, ix: u32) -> &str {
        let rec = &self.entries[ix as usize];
        &self.names[rec.name_at as usize..(rec.name_at + rec.name_len) as usize]
    }

    fn lower(&self, ix: u32) -> &str {
        let rec = &self.entries[ix as usize];
        &self.names_lower[rec.name_at as usize..(rec.name_at + rec.name_len) as usize]
    }

    fn view_of(&self, ix: u32) -> EntryView<'_> {
        let rec = &self.entries[ix as usize];
        EntryView {
            index: ix,
            name: self.name(ix),
            ext: &self.exts[rec.ext as usize],
            kind: rec.kind,
            attrs: rec.attrs,
            size: rec.size,
            modified: rec.modified,
        }
    }

    fn intern_ext(&mut self, name: &str) -> u16 {
        let Some((stem, ext)) = name.rsplit_once('.') else {
            return 0;
        };
        // ".gitignore" has no extension in Explorer's sense.
        if stem.is_empty() || ext.is_empty() {
            return 0;
        }
        let ext = ext.to_lowercase();
        if let Some(ix) = self.exts.iter().position(|e| *e == ext) {
            return ix as u16;
        }
        if self.exts.len() >= usize::from(u16::MAX) {
            return 0;
        }
        self.exts.push(ext);
        (self.exts.len() - 1) as u16
    }
}

/// Lowercases character by character, keeping a character as-is when its lowercase form has a
/// different UTF-8 length, so both arenas share offsets.
fn lowercase_same_len(name: &str) -> String {
    name.chars()
        .map(|c| {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) if l.len_utf8() == c.len_utf8() => l,
                _ => c,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(name: &str, kind: EntryKind, size: u64, modified: i64, attrs: u32) -> RawEntry {
        RawEntry {
            name: name.into(),
            lossy: false,
            kind,
            attrs,
            size,
            modified,
            created: 0,
        }
    }

    fn sample() -> DirSnapshot {
        let mut snap = DirSnapshot::default();
        snap.push_batch(&[
            raw("file10.txt", EntryKind::File, 300, 3, 0),
            raw("file2.txt", EntryKind::File, 100, 1, 0),
            raw("Zeta", EntryKind::Dir, 0, 5, 0),
            raw("alpha", EntryKind::Dir, 0, 4, 0),
            raw("photo.JPG", EntryKind::File, 200, 2, 0),
            raw(
                "desktop.ini",
                EntryKind::File,
                10,
                0,
                FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM,
            ),
            raw(".hidden", EntryKind::File, 1, 0, FILE_ATTRIBUTE_HIDDEN),
        ]);
        snap
    }

    fn names(snap: &DirSnapshot) -> Vec<&str> {
        (0..snap.visible_len())
            .map(|r| snap.row(r).unwrap().name)
            .collect()
    }

    #[test]
    fn folders_first_natural_and_hidden_rules() {
        let mut snap = sample();
        snap.sort(SortSpec::default());
        assert_eq!(
            names(&snap),
            ["alpha", "Zeta", "file2.txt", "file10.txt", "photo.JPG"]
        );
        snap.set_view(ViewOptions {
            show_hidden: true,
            ..ViewOptions::default()
        });
        assert_eq!(
            snap.visible_len(),
            6,
            "hidden shown, protected system file still hidden"
        );
        snap.set_view(ViewOptions {
            show_hidden: true,
            show_system: true,
            ..ViewOptions::default()
        });
        assert_eq!(snap.visible_len(), 7);
    }

    #[test]
    fn columns_and_direction() {
        let mut snap = sample();
        snap.sort(SortSpec {
            column: Column::Size,
            descending: true,
            ..SortSpec::default()
        });
        // Folders stay on top even when descending.
        assert_eq!(
            names(&snap),
            ["alpha", "Zeta", "file10.txt", "photo.JPG", "file2.txt"]
        );
        snap.sort(SortSpec {
            column: Column::Type,
            folders_first: false,
            ..SortSpec::default()
        });
        assert_eq!(snap.row(snap.visible_len() - 1).unwrap().ext, "txt");
        assert_eq!(snap.entry(4).unwrap().ext, "jpg");
    }

    #[test]
    fn filter_is_case_insensitive() {
        let mut snap = sample();
        snap.sort(SortSpec::default());
        snap.set_view(ViewOptions {
            filter: "FILE".into(),
            ..ViewOptions::default()
        });
        assert_eq!(names(&snap), ["file2.txt", "file10.txt"]);
        assert_eq!(snap.row_of(1), Some(0));
    }

    #[test]
    fn unicode_names_keep_offsets() {
        let mut snap = DirSnapshot::default();
        // 'İ' lowercases to two chars; the arenas must stay aligned.
        snap.push_batch(&[
            raw("İstanbul.txt", EntryKind::File, 0, 0, 0),
            raw("ÉMILE.txt", EntryKind::File, 0, 0, 0),
            raw("日本語.txt", EntryKind::File, 0, 0, 0),
        ]);
        snap.set_view(ViewOptions {
            filter: "émile".into(),
            ..ViewOptions::default()
        });
        assert_eq!(names(&snap), ["ÉMILE.txt"]);
        assert_eq!(snap.entry(0).unwrap().name, "İstanbul.txt");
    }

    /// Same order as Explorer's `StrCmpLogicalW` on names users actually have (TESTING.md §2).
    #[test]
    fn natural_order_matches_explorer() {
        let corpus = [
            "file1.txt",
            "file2.txt",
            "file10.txt",
            "File3.txt",
            "file20b.txt",
            "file20a.txt",
            "Laporan 2026 Q1.xlsx",
            "Laporan 2026 Q10.xlsx",
            "Laporan 2026 Q2.xlsx",
            "IMG_20260101_0001.jpg",
            "IMG_20260101_0010.jpg",
            "IMG_20251231_9999.jpg",
            "Tugas Bab 1.pdf",
            "Tugas Bab 12.pdf",
            "Tugas Bab 3.pdf",
            "tugas bab 2.pdf",
            "a",
            "b",
            "A1",
            "a01",
            "a2",
            "abc",
            "ABD",
            "zeta",
            "Zeta 2",
            "zeta 10",
            "写真_1.png",
            "写真_10.png",
            "写真_2.png",
            "日本語.txt",
            "émile.txt",
            "Émile2.txt",
            "photo 1.jpg",
            "photo1.jpg",
            "v1.2.10",
            "v1.2.9",
            "v1.10.0",
            "x-ray.txt",
            "xenon.txt",
        ];
        let mut snap = DirSnapshot::default();
        let batch: Vec<_> = corpus
            .iter()
            .map(|n| raw(n, EntryKind::File, 0, 0, 0))
            .collect();
        snap.push_batch(&batch);
        snap.sort(SortSpec {
            folders_first: false,
            ..SortSpec::default()
        });
        let ours = names(&snap);
        let mut explorer = corpus.to_vec();
        explorer.sort_by(|a, b| sx_win::str_cmp_logical(a, b));
        assert_eq!(ours, explorer);
    }

    #[test]
    fn dotfiles_have_no_extension() {
        let mut snap = sample();
        snap.set_view(ViewOptions {
            show_hidden: true,
            ..ViewOptions::default()
        });
        let hidden = (0..snap.len() as u32)
            .map(|i| snap.entry(i).unwrap())
            .find(|e| e.name == ".hidden")
            .unwrap();
        assert_eq!(hidden.ext, "");
    }
}
