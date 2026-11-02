//! Asset source: our own icons (`icons/sx/*.svg`, Lucide, ISC licence in `assets/icons`) first,
//! then GPUI Kit's default bundle. Fluent UI icons replace these later (DESIGN.md §13).

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

const OWN: &[(&str, &[u8])] = &[(
    "icons/sx/trash.svg",
    include_bytes!("../assets/icons/trash.svg"),
)];

/// Use with `application().with_assets(sx_ui::Assets)`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = OWN.iter().find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut out = gpui_kit::assets::Assets.list(path)?;
        out.extend(
            OWN.iter()
                .filter(|(p, _)| p.starts_with(path))
                .map(|(p, _)| SharedString::from(*p)),
        );
        Ok(out)
    }
}
