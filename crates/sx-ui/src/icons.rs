//! System icons in the list (FEATURES.md, DESIGN.md §13): loaded on a dedicated STA worker
//! that pumps messages, newest request first (the rows on screen now), cached by [`IconKey`]
//! (one per file type; programs and shortcuts per item), bounded by `perf.icon_cache_entries`.
//! Until an icon arrives the row shows the theme's glyph.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use futures::channel::mpsc;
use gpui_kit::RenderImage;
use sx_shell::icons::{IconBitmap, IconKey};
use sx_shell::{Order, StaWorker};

/// Shell "large" icons; the list draws them at `size.icon_sm`, sharp at 125–200 % scaling.
const ICON_PX: u32 = 32;

#[derive(Debug)]
pub(crate) struct IconCache {
    images: HashMap<IconKey, Arc<RenderImage>>,
    pending: HashSet<IconKey>,
    missing: HashSet<IconKey>,
    worker: Option<StaWorker>,
    loaded: mpsc::UnboundedSender<(IconKey, Option<IconBitmap>)>,
    limit: usize,
}

impl IconCache {
    /// Starts the worker; loaded icons come back on the returned channel.
    pub fn start(limit: usize) -> (Self, mpsc::UnboundedReceiver<(IconKey, Option<IconBitmap>)>) {
        let (loaded, done_rx) = mpsc::unbounded();
        let worker = StaWorker::spawn("sx-icons", Order::Newest)
            .map_err(|err| tracing::warn!(error = %err, "icon worker unavailable"))
            .ok();
        (
            Self {
                images: HashMap::new(),
                pending: HashSet::new(),
                missing: HashSet::new(),
                worker,
                loaded,
                limit,
            },
            done_rx,
        )
    }

    /// The icon for an item, requesting it when it is not cached yet.
    pub fn get(&mut self, path: &Path, is_dir: bool) -> Option<Arc<RenderImage>> {
        self.get_key(IconKey::for_item(path, is_dir))
    }

    /// The icon of a file's type, requesting it when it is not cached yet. The file itself is
    /// never opened for it (see [`IconKey::for_type`]).
    pub fn get_type(&mut self, path: &Path) -> Option<Arc<RenderImage>> {
        self.get_key(IconKey::for_type(path))
    }

    fn get_key(&mut self, key: IconKey) -> Option<Arc<RenderImage>> {
        if let Some(image) = self.images.get(&key) {
            return Some(image.clone());
        }
        if !self.missing.contains(&key)
            && self.pending.insert(key.clone())
            && let Some(worker) = &self.worker
        {
            let loaded = self.loaded.clone();
            let queued = worker.run(move || {
                // Always answer, so a failed icon is marked missing instead of pending forever.
                let icon = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    sx_shell::icons::load(&key, ICON_PX)
                }))
                .ok()
                .flatten();
                let _ = loaded.unbounded_send((key, icon));
            });
            if !queued {
                self.worker = None;
            }
        }
        None
    }

    /// The icon of a file's type when it is already here, without asking for it (for drawing
    /// code that cannot change the cache; ask with [`Self::get_type`] when the file is listed).
    pub fn peek_type(&self, path: &Path) -> Option<Arc<RenderImage>> {
        self.images.get(&IconKey::for_type(path)).cloned()
    }

    pub fn insert(&mut self, key: IconKey, icon: Option<IconBitmap>) {
        self.pending.remove(&key);
        let Some(icon) = icon else {
            self.missing.insert(key);
            return;
        };
        let Some(buffer) = image::RgbaImage::from_raw(icon.width, icon.height, icon.bgra) else {
            self.missing.insert(key);
            return;
        };
        if self.images.len() >= self.limit {
            // Per-item icons go first; type icons are few and shared by many rows.
            self.images.retain(|k, _| !matches!(k, IconKey::Item(_)));
            if self.images.len() >= self.limit {
                self.images.clear();
            }
        }
        // RenderImage holds BGRA, which is what the shell gave us.
        let image = RenderImage::new(vec![image::Frame::new(buffer)]);
        self.images.insert(key, Arc::new(image));
    }
}
