//! System icons in the list (FEATURES.md, DESIGN.md §13): loaded on a dedicated STA worker,
//! cached by [`IconKey`] (one per file type; programs and shortcuts per item), bounded by
//! `perf.icon_cache_entries`. Until an icon arrives the row shows the theme's glyph.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use futures::channel::mpsc;
use gpui_kit::RenderImage;
use sx_shell::icons::{IconBitmap, IconKey};

/// Shell "large" icons; the list draws them at `size.icon_sm`, sharp at 125–200 % scaling.
const ICON_PX: u32 = 32;

#[derive(Debug)]
pub(crate) struct IconCache {
    images: HashMap<IconKey, Arc<RenderImage>>,
    pending: HashSet<IconKey>,
    missing: HashSet<IconKey>,
    requests: Option<std::sync::mpsc::Sender<IconKey>>,
    limit: usize,
}

impl IconCache {
    /// Starts the worker; loaded icons come back on the returned channel.
    pub fn start(limit: usize) -> (Self, mpsc::UnboundedReceiver<(IconKey, Option<IconBitmap>)>) {
        let (tx, rx) = std::sync::mpsc::channel::<IconKey>();
        let (done_tx, done_rx) = mpsc::unbounded();
        let spawned = std::thread::Builder::new()
            .name("sx-icons".into())
            .spawn(move || {
                let _com = sx_shell::StaGuard::new();
                while let Ok(key) = rx.recv() {
                    let icon = std::panic::catch_unwind(|| sx_shell::icons::load(&key, ICON_PX))
                        .ok()
                        .flatten();
                    if done_tx.unbounded_send((key, icon)).is_err() {
                        return;
                    }
                }
            });
        let requests = spawned
            .map_err(|err| tracing::warn!(error = %err, "icon worker unavailable"))
            .ok()
            .map(|_| tx);
        (
            Self {
                images: HashMap::new(),
                pending: HashSet::new(),
                missing: HashSet::new(),
                requests,
                limit,
            },
            done_rx,
        )
    }

    /// The icon for an item, requesting it when it is not cached yet.
    pub fn get(&mut self, path: &Path, is_dir: bool) -> Option<Arc<RenderImage>> {
        let key = IconKey::for_item(path, is_dir);
        if let Some(image) = self.images.get(&key) {
            return Some(image.clone());
        }
        if !self.missing.contains(&key)
            && self.pending.insert(key.clone())
            && let Some(requests) = &self.requests
            && requests.send(key).is_err()
        {
            self.requests = None;
        }
        None
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
