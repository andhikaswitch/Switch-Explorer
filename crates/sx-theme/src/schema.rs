//! Token declarations (THEMING.md §3). Every token is declared once here; theme files only give
//! values. `ResolvedTheme` stores one value per token, indexed by `Token as usize`.

/// What kind of value a token holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenType {
    /// A single colour (may be translucent).
    Color,
    /// A colour or a linear gradient.
    Paint,
    Shadow,
    Number,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenMeta {
    pub token: Token,
    pub id: &'static str,
    pub ty: TokenType,
}

macro_rules! define_tokens {
    ($( $variant:ident = $id:literal : $ty:ident ),* $(,)?) => {
        /// A theme token. Ids are stable and dotted (`button.primary.bg_hover`).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u16)]
        pub enum Token { $($variant),* }

        /// Every token, in `Token` order.
        pub static TOKENS: &[TokenMeta] = &[
            $( TokenMeta { token: Token::$variant, id: $id, ty: TokenType::$ty } ),*
        ];
    };
}

impl Token {
    pub fn meta(self) -> &'static TokenMeta {
        &TOKENS[self as usize]
    }

    pub fn id(self) -> &'static str {
        self.meta().id
    }

    pub fn ty(self) -> TokenType {
        self.meta().ty
    }

    pub fn from_id(id: &str) -> Option<Self> {
        TOKENS.iter().find(|m| m.id == id).map(|m| m.token)
    }
}

define_tokens! {
    // Backgrounds
    BgWindow = "bg.window": Paint,
    BgTitlebar = "bg.titlebar": Paint,
    BgSidebar = "bg.sidebar": Paint,
    BgContent = "bg.content": Paint,
    BgStatusbar = "bg.statusbar": Paint,
    // Surfaces
    SurfaceCard = "surface.card": Paint,
    SurfaceCardHover = "surface.card_hover": Paint,
    SurfaceOverlay = "surface.overlay": Paint,
    SurfaceInput = "surface.input": Paint,
    SurfaceChip = "surface.chip": Paint,
    Scrim = "scrim": Paint,
    // Text
    FgPrimary = "fg.primary": Color,
    FgSecondary = "fg.secondary": Color,
    FgMuted = "fg.muted": Color,
    FgDisabled = "fg.disabled": Color,
    FgOnHighlight = "fg.on_highlight": Color,
    FgOnSolid = "fg.on_solid": Color,
    FgLink = "fg.link": Color,
    // Lines
    BorderSubtle = "border.subtle": Color,
    BorderStrong = "border.strong": Color,
    BorderInput = "border.input": Color,
    BorderInputFocus = "border.input_focus": Color,
    // A text field under the pointer (stronger than `border.input`, not the focus colour).
    BorderInputHover = "border.input_hover": Color,
    Divider = "divider": Color,
    // Accent fields
    AccentHighlight = "accent.highlight": Paint,
    AccentHighlightHover = "accent.highlight_hover": Paint,
    AccentHighlightPressed = "accent.highlight_pressed": Paint,
    AccentSolid = "accent.solid": Paint,
    AccentSolidHover = "accent.solid_hover": Paint,
    AccentSolidPressed = "accent.solid_pressed": Paint,
    // Accent lines and text
    AccentText = "accent.text": Color,
    AccentGlow = "accent.glow": Color,
    AccentEdge = "accent.edge": Color,
    // State
    HoverBg = "hover.bg": Color,
    PressedBg = "pressed.bg": Color,
    SelectionBg = "selection.bg": Color,
    SelectionBorder = "selection.border": Color,
    SelectionInactiveBg = "selection.inactive_bg": Color,
    FocusRing = "focus.ring": Color,
    DropTargetBg = "drop_target.bg": Color,
    DropTargetBorder = "drop_target.border": Color,
    DropBlockedBorder = "drop_blocked.border": Color,
    // Navigation
    NavItemFg = "nav.item.fg": Color,
    NavItemIcon = "nav.item.icon": Color,
    NavItemHoverBg = "nav.item.hover.bg": Paint,
    NavItemHoverFg = "nav.item.hover.fg": Color,
    NavItemPressedBg = "nav.item.pressed.bg": Paint,
    NavItemActiveBg = "nav.item.active.bg": Paint,
    NavItemActiveFg = "nav.item.active.fg": Color,
    NavItemActiveEdge = "nav.item.active.edge": Color,
    NavItemActiveGlow = "nav.item.active.glow": Color,
    NavSectionFg = "nav.section.fg": Color,
    // Tabs
    TabFg = "tab.fg": Color,
    TabHoverBg = "tab.hover.bg": Color,
    TabActiveBg = "tab.active.bg": Paint,
    TabActiveFg = "tab.active.fg": Color,
    TabActiveIndicator = "tab.active.indicator": Paint,
    // Breadcrumb
    BreadcrumbFg = "breadcrumb.fg": Color,
    BreadcrumbCurrentFg = "breadcrumb.current.fg": Color,
    BreadcrumbHoverBg = "breadcrumb.hover.bg": Color,
    BreadcrumbSeparator = "breadcrumb.separator": Color,
    // Lists and grids
    HeaderFg = "header.fg": Color,
    HeaderHoverBg = "header.hover.bg": Color,
    RowHoverBg = "row.hover.bg": Color,
    RowSelectedBg = "row.selected.bg": Color,
    RowSelectedBorder = "row.selected.border": Color,
    RowSelectedInactiveBg = "row.selected_inactive.bg": Color,
    // The part of a name that matches the filter or search (shown bold).
    MatchFg = "match.fg": Color,
    TileHoverBg = "tile.hover.bg": Color,
    TileSelectedBg = "tile.selected.bg": Color,
    // Buttons
    ButtonPrimaryBg = "button.primary.bg": Paint,
    ButtonPrimaryBgHover = "button.primary.bg_hover": Paint,
    ButtonPrimaryBgPressed = "button.primary.bg_pressed": Paint,
    ButtonPrimaryFg = "button.primary.fg": Color,
    ButtonSecondaryBg = "button.secondary.bg": Paint,
    ButtonSecondaryBgHover = "button.secondary.bg_hover": Paint,
    ButtonSecondaryBorder = "button.secondary.border": Color,
    ButtonSecondaryFg = "button.secondary.fg": Color,
    ButtonSecondaryFgHover = "button.secondary.fg_hover": Color,
    ButtonSecondaryBgPressed = "button.secondary.bg_pressed": Paint,
    ButtonGhostBgHover = "button.ghost.bg_hover": Paint,
    ButtonGhostBgPressed = "button.ghost.bg_pressed": Paint,
    ButtonGhostFg = "button.ghost.fg": Color,
    ButtonGhostFgHover = "button.ghost.fg_hover": Color,
    // Cards (drive tiles, Home)
    CardHoverFg = "card.hover.fg": Color,
    ButtonDangerBg = "button.danger.bg": Paint,
    ButtonDangerBgHover = "button.danger.bg_hover": Paint,
    ButtonDangerFg = "button.danger.fg": Color,
    // Controls
    ToggleOnBg = "toggle.on.bg": Paint,
    ToggleOnThumb = "toggle.on.thumb": Color,
    ToggleOffBorder = "toggle.off.border": Color,
    ToggleOffThumb = "toggle.off.thumb": Color,
    CheckboxCheckedBg = "checkbox.checked.bg": Paint,
    CheckboxCheck = "checkbox.check": Color,
    CheckboxBorder = "checkbox.border": Color,
    SliderFill = "slider.fill": Paint,
    SliderTrack = "slider.track": Color,
    InputPlaceholder = "input.placeholder": Color,
    // Feedback
    SuccessSolid = "success.solid": Color,
    SuccessText = "success.text": Color,
    WarningSolid = "warning.solid": Color,
    WarningText = "warning.text": Color,
    DangerSolid = "danger.solid": Color,
    DangerText = "danger.text": Color,
    InfoSolid = "info.solid": Color,
    InfoText = "info.text": Color,
    ToastBg = "toast.bg": Paint,
    ToastFg = "toast.fg": Color,
    ProgressFill = "progress.fill": Paint,
    ProgressTrack = "progress.track": Color,
    // Hot Switch
    OsdBg = "osd.bg": Paint,
    OsdFg = "osd.fg": Color,
    OsdBorder = "osd.border": Color,
    OsdStateOn = "osd.state_on": Paint,
    OsdStateOff = "osd.state_off": Color,
    OsdIconOn = "osd.icon_on": Color,
    OsdIconOff = "osd.icon_off": Color,
    StatuschipOn = "statuschip.on": Color,
    StatuschipPartial = "statuschip.partial": Color,
    StatuschipOff = "statuschip.off": Color,
    // Gauge
    GaugeFill = "gauge.fill": Paint,
    GaugeFillWarning = "gauge.fill_warning": Paint,
    GaugeFillDanger = "gauge.fill_danger": Paint,
    GaugeTrack = "gauge.track": Color,
    // Scrollbar
    ScrollbarThumb = "scrollbar.thumb": Color,
    ScrollbarThumbHover = "scrollbar.thumb_hover": Color,
    ScrollbarTrack = "scrollbar.track": Color,
    // Categories
    CategoryDocuments = "category.documents": Color,
    CategoryAudio = "category.audio": Color,
    CategoryPictures = "category.pictures": Color,
    CategoryVideos = "category.videos": Color,
    CategoryArchives = "category.archives": Color,
    CategoryCode = "category.code": Color,
    CategoryApps = "category.apps": Color,
    CategoryOther = "category.other": Color,
    // Icons
    IconFolderBack = "icon.folder.back": Color,
    IconFolderFront = "icon.folder.front": Color,
    IconDefault = "icon.default": Color,
    // Shadows
    ShadowCard = "shadow.card": Shadow,
    ShadowPopover = "shadow.popover": Shadow,
    ShadowDialog = "shadow.dialog": Shadow,
    ShadowGlow = "shadow.glow": Shadow,
    // Numbers
    RadiusXs = "radius.xs": Number,
    RadiusSm = "radius.sm": Number,
    RadiusMd = "radius.md": Number,
    RadiusLg = "radius.lg": Number,
    RadiusXl = "radius.xl": Number,
    SpaceXxs = "space.xxs": Number,
    SpaceXs = "space.xs": Number,
    SpaceSm = "space.sm": Number,
    SpaceMd = "space.md": Number,
    SpaceLg = "space.lg": Number,
    SpaceXl = "space.xl": Number,
    Space2xl = "space.2xl": Number,
    Space3xl = "space.3xl": Number,
    RowHeightCompact = "row.height.compact": Number,
    RowHeightDefault = "row.height.default": Number,
    RowHeightComfortable = "row.height.comfortable": Number,
    RowCutOpacity = "row.cut.opacity": Number,
    SlashAngleDeg = "slash.angle_deg": Number,
    SlashEdgeWidth = "slash.edge_width": Number,
    MotionFast = "motion.fast": Number,
    MotionBase = "motion.base": Number,
    MotionSlow = "motion.slow": Number,
    MotionSlide = "motion.slide": Number,
    // Type scale (DESIGN.md §4): size and line height in px.
    FontSizeCaption = "font.size.caption": Number,
    FontSizeBody = "font.size.body": Number,
    FontSizeSubtitle = "font.size.subtitle": Number,
    FontSizeTitle = "font.size.title": Number,
    FontSizeDisplay = "font.size.display": Number,
    FontLineCaption = "font.line.caption": Number,
    FontLineBody = "font.line.body": Number,
    FontLineSubtitle = "font.line.subtitle": Number,
    FontLineTitle = "font.line.title": Number,
    FontLineDisplay = "font.line.display": Number,
    // Window chrome (DESIGN.md §8.1) and controls (§11, §14) in px.
    SizeTitlebar = "size.titlebar": Number,
    SizeCommandbar = "size.commandbar": Number,
    SizeStatusbar = "size.statusbar": Number,
    SizeSidebar = "size.sidebar": Number,
    SizeTabMax = "size.tab_max": Number,
    SizeTabIndicator = "size.tab_indicator": Number,
    SizeControl = "size.control": Number,
    SizeIcon = "size.icon": Number,
    SizeIconSm = "size.icon_sm": Number,
    // The Hot Switch OSD (DESIGN.md §9.1): height, width range, distance above the status bar.
    SizeOsdHeight = "size.osd_height": Number,
    SizeOsdMinWidth = "size.osd_min_width": Number,
    SizeOsdMaxWidth = "size.osd_max_width": Number,
    SizeOsdOffset = "size.osd_offset": Number,
    // The search box in the command bar (DESIGN.md §8.1).
    SizeSearchWidth = "size.search_width": Number,
    FocusRingWidth = "focus.ring_width": Number,
    // Opacity of disabled controls (0..1).
    OpacityDisabled = "opacity.disabled": Number,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn table_matches_enum_order_and_ids_are_unique() {
        for (ix, meta) in TOKENS.iter().enumerate() {
            assert_eq!(meta.token as usize, ix);
        }
        let ids: HashSet<_> = TOKENS.iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), TOKENS.len());
        assert_eq!(
            Token::from_id("button.primary.bg_hover"),
            Some(Token::ButtonPrimaryBgHover)
        );
        assert_eq!(Token::from_id("pal.accent.500"), None);
    }
}
