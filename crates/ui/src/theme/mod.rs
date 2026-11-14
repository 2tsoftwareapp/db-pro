mod fonts;
pub(crate) mod text_selection_style;
pub mod tokens;

// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Vec2, Visuals};

use crate::tokens::{
    component::button::BUTTON_DEFAULT_PADDING,
    semantic::{subtle_wash, SemanticTokens},
    FONT_SIZE_BADGE, FONT_SIZE_MONO_UI, FONT_SIZE_UI_LABEL, RADIUS_BUTTON, RADIUS_DIALOG, RADIUS_POPOVER,
    SHADOW_ALPHA_DARK, SHADOW_ALPHA_LIGHT, SHADOW_BLUR, SHADOW_OFFSET_Y, SPACE_2XL, SPACE_MD, SPACE_SM, SPACE_XS,
    STROKE_THIN, WINDOW_SHADOW_ALPHA,
};

// Renderer adapter mapping: these translate design tokens into egui's own style
// slots and are deliberately private to this adapter rather than public tokens.
const RADIUS_EGUI_WINDOW: f32 = RADIUS_DIALOG;
const RADIUS_EGUI_MENU: f32 = RADIUS_POPOVER;
const RADIUS_EGUI_WIDGET: f32 = RADIUS_BUTTON;
const EGUI_ITEM_SPACING: Vec2 = Vec2::new(SPACE_SM, SPACE_XS);
const EGUI_BUTTON_PADDING: Vec2 = BUTTON_DEFAULT_PADDING;
const EGUI_INTERACT_SIZE: Vec2 = Vec2::new(SPACE_2XL, SPACE_2XL);
const EGUI_WINDOW_MARGIN: f32 = SPACE_MD;
const EGUI_MENU_MARGIN: f32 = SPACE_XS;
const EGUI_INDENT: f32 = SPACE_MD;

const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
const INTER_REGULAR_EXT: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular-ext.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Inter-Medium.ttf");
const INTER_MEDIUM_EXT: &[u8] = include_bytes!("../../assets/fonts/Inter-Medium-ext.ttf");

/// Register a discovered system face under `role` and append it to each family.
///
/// Fallback families are ordered by preference, so faces are always *appended*:
/// the bundled Inter faces stay first for Latin text and the system face only
/// contributes the glyphs Inter cannot draw (#567).
fn install_face(fonts: &mut egui::FontDefinitions, face: &fonts::FontFace, role: &str, families: &[FontFamily]) {
    let key = face.key(role);
    if fonts.font_data.contains_key(&key) {
        return;
    }
    fonts
        .font_data
        .insert(key.clone(), std::sync::Arc::new(egui::FontData::from_owned(face.bytes.clone())));
    for family in families {
        let stack = fonts.families.entry(family.clone()).or_default();
        if !stack.contains(&key) {
            stack.push(key.clone());
        }
    }
}

/// Product-owned visual tokens for the native shell.
///
/// Flat compatibility facade over [`SemanticTokens`]: every field is built from
/// the corresponding role, so existing callers keep working while the semantic
/// layer stays authoritative.
#[derive(Debug, Clone, Copy)]
pub struct DbProTheme {
    pub dark_mode: bool,
    /// Current accessibility preference, carried with the theme to shared widgets.
    pub reduce_motion: bool,
    /// Full role detail for callers that need more than one flat field at a time
    /// (e.g. status fill/border pairs).
    pub semantic: SemanticTokens,
    pub surface_app: Color32,
    pub surface_panel: Color32,
    pub surface_elevated: Color32,
    pub surface_floating: Color32,
    pub surface_editor: Color32,
    /// Resting fill of a text field / textarea / search box / select trigger
    /// (`background.field`). Distinct from `surface_elevated`, so a field is not
    /// painted with the same fill as the card it sits on.
    pub surface_field: Color32,
    pub surface_hover: Color32,
    /// Compatibility alias of `background.selected` — active/pressed widget and
    /// selected-row wash.
    pub surface_active: Color32,
    /// Compatibility alias of `background.subtle` (disabled fills, card icon
    /// boxes); the semantic role is authoritative.
    pub surface_2: Color32,
    pub border_subtle: Color32,
    pub border_default: Color32,
    pub border_strong: Color32,
    /// Focus ring color (`border.focus`); equals `accent` in the shipped themes.
    pub border_focus: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_tertiary: Color32,
    pub text_disabled: Color32,
    /// Compatibility alias of `foreground.muted` (hints, status bars) — its own
    /// role: light coincides with `text_tertiary`, dark deliberately differs.
    pub text_muted: Color32,
    pub text_inverse: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_soft: Color32,
    pub accent_foreground: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub info: Color32,
    pub overlay: Color32,
    pub code_keyword: Color32,
    pub code_string: Color32,
    pub code_number: Color32,
    pub code_comment: Color32,
    pub code_type: Color32,
    pub code_function: Color32,
    pub code_operator: Color32,
    pub code_punctuation: Color32,
    pub code_variable: Color32,
    /// Categorical chart series colors (alias of `semantic.chart.series`); views
    /// pick series colors with `theme.chart_series[i % theme.chart_series.len()]`.
    pub chart_series: [Color32; 6],
}

impl Default for DbProTheme {
    fn default() -> Self {
        Self::light()
    }
}

impl DbProTheme {
    pub fn light() -> Self {
        let semantic = SemanticTokens::light();
        Self {
            dark_mode: false,
            reduce_motion: false,
            semantic,
            // Warm Minimalism roles; raw hex lives in `tokens::primitive::light`.
            surface_app: semantic.background.canvas,
            surface_panel: semantic.background.panel,
            surface_elevated: semantic.background.elevated,
            surface_floating: semantic.background.floating,
            surface_editor: semantic.background.editor,
            surface_field: semantic.background.field,
            surface_hover: semantic.background.hover,
            surface_active: semantic.background.selected,
            surface_2: semantic.background.subtle,
            border_subtle: semantic.border.subtle,
            border_default: semantic.border.default,
            border_strong: semantic.border.strong,
            border_focus: semantic.border.focus,
            text_primary: semantic.foreground.primary,
            text_secondary: semantic.foreground.secondary,
            text_tertiary: semantic.foreground.tertiary,
            text_disabled: semantic.foreground.disabled,
            text_muted: semantic.foreground.muted,
            text_inverse: semantic.foreground.inverse,
            accent: semantic.accent.solid,
            accent_hover: semantic.accent.solid_hover,
            accent_soft: semantic.accent.subtle,
            accent_foreground: semantic.accent.foreground,
            success: semantic.status.success.solid,
            warning: semantic.status.warning.solid,
            danger: semantic.status.danger.solid,
            info: semantic.status.info.solid,
            overlay: semantic.background.overlay,
            code_keyword: semantic.syntax.keyword,
            code_string: semantic.syntax.string,
            code_number: semantic.syntax.number,
            code_comment: semantic.syntax.comment,
            code_type: semantic.syntax.type_,
            code_function: semantic.syntax.function,
            code_operator: semantic.syntax.operator,
            code_punctuation: semantic.syntax.punctuation,
            code_variable: semantic.syntax.variable,
            chart_series: semantic.chart.series,
        }
    }
    /// Follow the palette already installed on a context. Shared widgets that
    /// are not handed a theme use this so they cannot fall back to light.
    pub fn from_visuals(visuals: &Visuals) -> Self {
        if visuals.dark_mode {
            Self::dark()
        } else {
            Self::light()
        }
    }

    pub fn dark() -> Self {
        let semantic = SemanticTokens::dark();
        Self {
            dark_mode: true,
            reduce_motion: false,
            semantic,
            // Dark workstation roles; raw hex lives in `tokens::primitive::dark`.
            surface_app: semantic.background.canvas,
            surface_panel: semantic.background.panel,
            surface_elevated: semantic.background.elevated,
            surface_floating: semantic.background.floating,
            surface_editor: semantic.background.editor,
            surface_field: semantic.background.field,
            surface_hover: semantic.background.hover,
            surface_active: semantic.background.selected,
            surface_2: semantic.background.subtle,
            border_subtle: semantic.border.subtle,
            border_default: semantic.border.default,
            border_strong: semantic.border.strong,
            border_focus: semantic.border.focus,
            text_primary: semantic.foreground.primary,
            text_secondary: semantic.foreground.secondary,
            text_tertiary: semantic.foreground.tertiary,
            text_disabled: semantic.foreground.disabled,
            text_muted: semantic.foreground.muted,
            text_inverse: semantic.foreground.inverse,
            accent: semantic.accent.solid,
            accent_hover: semantic.accent.solid_hover,
            accent_soft: semantic.accent.subtle,
            accent_foreground: semantic.accent.foreground,
            success: semantic.status.success.solid,
            warning: semantic.status.warning.solid,
            danger: semantic.status.danger.solid,
            info: semantic.status.info.solid,
            overlay: semantic.background.overlay,
            code_keyword: semantic.syntax.keyword,
            code_string: semantic.syntax.string,
            code_number: semantic.syntax.number,
            code_comment: semantic.syntax.comment,
            code_type: semantic.syntax.type_,
            code_function: semantic.syntax.function,
            code_operator: semantic.syntax.operator,
            code_punctuation: semantic.syntax.punctuation,
            code_variable: semantic.syntax.variable,
            chart_series: semantic.chart.series,
        }
    }
    /// Subtle tinted fill for badges, diff rows, and status cards (~10-12% opacity).
    /// Recipe owned by the semantic layer so badges and status roles cannot drift.
    pub fn soft_tint(self, color: Color32) -> Color32 {
        subtle_wash(color, self.dark_mode)
    }

    pub fn success_soft(self) -> Color32 {
        self.semantic.status.success.subtle
    }

    pub fn warning_soft(self) -> Color32 {
        self.semantic.status.warning.subtle
    }

    pub fn danger_soft(self) -> Color32 {
        self.semantic.status.danger.subtle
    }

    pub fn info_soft(self) -> Color32 {
        self.semantic.status.info.subtle
    }

    /// Readable foreground for an opaque solid fill, including animated fills.
    pub fn text_on_solid(self, fill: Color32) -> Color32 {
        // Black and white bracket every possible opaque fill: the crossover
        // luminance gives both at least 4.5:1 for normal text.
        if relative_luminance(fill) >= 0.179 {
            Color32::BLACK
        } else {
            Color32::WHITE
        }
    }

    /// Quiet gutter wash behind line numbers (Zed/DBeaver density).
    ///
    /// Uses the chrome panel role: it is one step off the editor in both themes
    /// (light `#f3f3f3` on white, dark `#171717` on `#212121`), where the previous
    /// hard-coded values made the light gutter identical to the canvas.
    pub fn editor_gutter_fill(self) -> Color32 {
        self.surface_panel
    }

    /// Soft current-line highlight — accent-tinted wash per the Stitch
    /// active-line spec, readable without shouting.
    pub fn editor_current_line_fill(self) -> Color32 {
        self.soft_tint(self.accent)
    }

    /// Selection wash over SQL text.
    pub fn editor_selection_fill(self) -> Color32 {
        if self.dark_mode {
            Color32::from_rgba_unmultiplied(51, 156, 255, 55)
        } else {
            Color32::from_rgba_unmultiplied(2, 133, 255, 42)
        }
    }

    pub fn editor_line_number(self, current: bool) -> Color32 {
        if current {
            self.accent
        } else if self.dark_mode {
            Color32::from_rgb(120, 120, 120)
        } else {
            Color32::from_rgb(128, 128, 128)
        }
    }

    /// UI labels, badges, and controls — Inter Medium (500), matching OpenAI Sans Medium.
    pub fn ui_medium_font(size: f32) -> FontId {
        FontId::new(size, FontFamily::Name("ui_medium".into()))
    }

    pub fn install_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "lucide".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(lucide_icons::LUCIDE_FONT_BYTES)),
        );
        fonts
            .families
            .entry(FontFamily::Name("lucide".into()))
            .or_default()
            .insert(0, "lucide".to_owned());

        fonts.font_data.insert(
            "inter".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(INTER_REGULAR)),
        );
        fonts.font_data.insert(
            "inter_ext".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(INTER_REGULAR_EXT)),
        );
        fonts.font_data.insert(
            "inter_medium".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(INTER_MEDIUM)),
        );
        fonts.font_data.insert(
            "inter_medium_ext".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(INTER_MEDIUM_EXT)),
        );

        let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
        proportional.insert(0, "inter_ext".to_owned());
        proportional.insert(0, "inter".to_owned());

        let medium = fonts.families.entry(FontFamily::Name("ui_medium".into())).or_default();
        medium.insert(0, "inter_medium_ext".to_owned());
        medium.insert(0, "inter_medium".to_owned());

        // #567: the bundled Inter files are subsets — they miss the whole
        // Vietnamese `Latin Extended Additional` block (U+1EA0–U+1EF9). A system
        // face is therefore only accepted when it provably draws `ạ ệ ỡ ơ ư`,
        // and the candidate list covers Fedora, Arch, Alpine, NixOS, Flatpak and
        // the three desktop platforms instead of a handful of Debian paths.
        let sans = fonts::first_covering(fonts::SANS_CANDIDATES, fonts::VIETNAMESE_PROBE);
        if let Some(face) = &sans {
            install_face(&mut fonts, face, "ui", &[FontFamily::Proportional]);
        }

        // Monospace keeps its own metrics even when the face misses a few
        // Vietnamese probes (DejaVu Sans Mono does); the missing glyph is then
        // drawn by the UI fallback below rather than replacing the mono face.
        let mono = fonts::first_covering(fonts::MONO_CANDIDATES, fonts::VIETNAMESE_PROBE)
            .or_else(|| fonts::first_existing(fonts::MONO_CANDIDATES));
        if let Some(face) = &mono {
            install_face(&mut fonts, face, "mono", &[FontFamily::Monospace]);
        }

        // #584: one face per CJK script (Han, kana, hangul), never every face
        // found. A single Noto CJK / PingFang / MS Gothic file satisfies all
        // three, so the loader reads ~one file instead of five to eight.
        for face in fonts::load_cjk_faces(fonts::CJK_CANDIDATES) {
            install_face(
                &mut fonts,
                &face,
                "cjk",
                &[
                    FontFamily::Proportional,
                    FontFamily::Name("ui_medium".into()),
                    FontFamily::Monospace,
                ],
            );
        }

        // Monospace falls back to the UI faces last so a character missing from
        // the mono face still renders instead of showing tofu.
        let monospace = fonts.families.entry(FontFamily::Monospace).or_default();
        monospace.push("inter_ext".to_owned());
        monospace.push("inter".to_owned());
        if let Some(face) = &sans {
            let key = face.key("ui");
            if !monospace.contains(&key) {
                monospace.push(key);
            }
        }

        ctx.set_fonts(fonts);
    }



    pub fn apply(self, ctx: &egui::Context) {
        text_selection_style::install(ctx);
        // In egui 0.36 visuals alone do not select the active light/dark style.
        ctx.set_theme(if self.dark_mode {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        let mut visuals = if self.dark_mode {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        visuals.dark_mode = self.dark_mode;
        visuals.override_text_color = Some(self.text_primary);
        visuals.panel_fill = self.surface_panel;
        visuals.window_fill = self.surface_floating;
        visuals.faint_bg_color = self.surface_app;
        visuals.extreme_bg_color = self.surface_editor;
        visuals.code_bg_color = self.surface_editor;
        visuals.hyperlink_color = self.accent;
        visuals.warn_fg_color = self.warning;
        visuals.error_fg_color = self.danger;
        // Text selection uses the same solid blue/white pair in both themes.
        let selection = Self::light();
        visuals.selection.bg_fill = selection.accent_hover;
        visuals.selection.stroke = Stroke::new(STROKE_THIN, selection.accent_foreground);
        visuals.window_corner_radius = CornerRadius::same(RADIUS_EGUI_WINDOW as u8);
        visuals.window_shadow = Shadow {
            offset: [0, SHADOW_OFFSET_Y as i8],
            blur: SHADOW_BLUR as u8,
            spread: 0,
            color: Color32::from_black_alpha(WINDOW_SHADOW_ALPHA),
        };
        visuals.menu_corner_radius = CornerRadius::same(RADIUS_EGUI_MENU as u8);
        visuals.popup_shadow = visuals.window_shadow;
        // Buttons opt into their own emphasis. Bare icon/ghost controls should
        // read as actions in the workspace, not as a wall of outlined fields.
        visuals.button_frame = false;
        visuals.collapsing_header_frame = false;
        visuals.striped = true;
        visuals.widgets.noninteractive.bg_fill = self.surface_panel;
        visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(STROKE_THIN, self.text_secondary);
        visuals.widgets.noninteractive.corner_radius = CornerRadius::same(RADIUS_EGUI_WIDGET as u8);
        visuals.widgets.inactive.bg_fill = self.surface_panel;
        visuals.widgets.inactive.weak_bg_fill = self.surface_panel;
        // Resting inputs stay quiet; hover/focus still provide the interaction boundary.
        visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        visuals.widgets.inactive.fg_stroke = Stroke::new(STROKE_THIN, self.text_secondary);
        visuals.widgets.inactive.corner_radius = CornerRadius::same(RADIUS_EGUI_WIDGET as u8);
        visuals.widgets.hovered.bg_fill = self.surface_hover;
        visuals.widgets.hovered.weak_bg_fill = self.surface_hover;
        visuals.widgets.hovered.bg_stroke = Stroke::new(STROKE_THIN, self.border_strong);
        // `accent` (#006fcc) measures 4.33:1 on the `#ededed` hover wash and 4.10:1 on
        // the `#e7e7e7` selected wash, both short of AA; the deeper hover accent keeps
        // accent text on interactive washes at 5.6:1 / 5.3:1 (#344).
        visuals.widgets.hovered.fg_stroke = Stroke::new(STROKE_THIN, self.accent_hover);
        visuals.widgets.hovered.corner_radius = CornerRadius::same(RADIUS_EGUI_WIDGET as u8);
        visuals.widgets.active.bg_fill = self.surface_active;
        visuals.widgets.active.weak_bg_fill = self.surface_active;
        visuals.widgets.active.bg_stroke = Stroke::new(STROKE_THIN, self.accent_hover);
        visuals.widgets.active.fg_stroke = Stroke::new(STROKE_THIN, self.accent_hover);
        visuals.widgets.active.corner_radius = CornerRadius::same(RADIUS_EGUI_WIDGET as u8);
        visuals.widgets.open.bg_fill = self.surface_hover;
        visuals.widgets.open.weak_bg_fill = self.surface_hover;
        visuals.widgets.open.bg_stroke = Stroke::new(STROKE_THIN, self.border_strong);
        visuals.widgets.open.fg_stroke = Stroke::new(STROKE_THIN, self.text_primary);
        visuals.widgets.open.corner_radius = CornerRadius::same(RADIUS_EGUI_WIDGET as u8);
        visuals.window_stroke = Stroke::new(STROKE_THIN, self.border_subtle);
        ctx.global_style_mut(|style| {
            style.visuals = visuals;
            style.spacing.item_spacing = EGUI_ITEM_SPACING;
            style.spacing.button_padding = EGUI_BUTTON_PADDING;
            // macOS-style overlay scrollbars: nothing painted at rest, thin thumb on
            // hover/scroll, expanding slightly when the bar itself is hovered.
            style.spacing.scroll = egui::style::ScrollStyle {
                dormant_background_opacity: 0.0,
                dormant_handle_opacity: 0.0,
                active_background_opacity: 0.0,
                active_handle_opacity: 0.5,
                interact_background_opacity: 0.0,
                interact_handle_opacity: 1.0,
                ..egui::style::ScrollStyle::thin()
            };
            style.spacing.interact_size = EGUI_INTERACT_SIZE;
            style.spacing.window_margin = Margin::same(EGUI_WINDOW_MARGIN as i8);
            style.spacing.menu_margin = Margin::same(EGUI_MENU_MARGIN as i8);
            style.spacing.indent = EGUI_INDENT;
            style
                .text_styles
                .insert(TextStyle::Body, FontId::proportional(FONT_SIZE_UI_LABEL));
            style
                .text_styles
                .insert(TextStyle::Button, FontId::proportional(FONT_SIZE_UI_LABEL));
            style
                .text_styles
                .insert(TextStyle::Small, FontId::proportional(FONT_SIZE_BADGE));
            style
                .text_styles
                .insert(TextStyle::Monospace, FontId::monospace(FONT_SIZE_MONO_UI));
        });
    }

    pub fn floating_shadow(self) -> Shadow {
        Shadow {
            offset: [0, SHADOW_OFFSET_Y as i8],
            blur: SHADOW_BLUR as u8,
            spread: 0,
            color: Color32::from_black_alpha(if self.dark_mode {
                SHADOW_ALPHA_DARK
            } else {
                SHADOW_ALPHA_LIGHT
            }),
        }
    }
}

pub(crate) fn relative_luminance(color: Color32) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

#[cfg(test)]
#[path = "tests/mod_tests.rs"]
mod tests;
