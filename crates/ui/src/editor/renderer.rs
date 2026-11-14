use std::time::Duration;

use egui::{
    text::{LayoutJob, TextFormat},
    CornerRadius, Event, FontId, Id, Key, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2,
};

use super::{
    brackets,
    buffer::{EditorSnapshot, TextBuffer},
    cursor::CursorPosition,
    decorations::DiagnosticSeverity,
    diagnostics::Diagnostic,
    hover::HoveredSqlToken,
    prediction::EditPrediction,
    selection::SelectionRange,
    syntax::{CachedSqlTokens, SqlDialect, SqlHighlighter, SyntaxToken, SyntaxTokenKind},
};
use crate::{
    components::interact::text_input_info,
    tokens::STROKE_THIN,
    DbProTheme,
};

const DEFAULT_LINE_HEIGHT: f32 = 20.0;
const LINE_HEIGHT_MULT: f32 = 1.48;
const FONT_SIZE: f32 = 13.5;
const PADDING_LEFT: f32 = 10.0;
const PADDING_TOP: f32 = 12.0;
const PADDING_BOTTOM: f32 = 64.0;
const CARET_BLINK_ON_SECS: f64 = 0.53;
const CARET_BLINK_OFF_SECS: f64 = 0.53;
const CARET_SOLID_AFTER_INPUT_SECS: f64 = 0.5;
/// Flush with the query workspace — no card chrome around the buffer.
const EDITOR_ROUNDING: f32 = 0.0;

#[derive(Debug, Clone)]
pub struct SqlEditorResponse {
    pub changed: bool,
    pub cursor_screen_pos: Pos2,
    /// Interactive viewport rect — used to anchor find/completion overlays.
    pub rect: Rect,
    pub wants_completion: bool,
    pub wants_identifier_typed: bool,
    /// Tab moves the snippet stop by this many steps. `0` means Tab was not used for a snippet.
    pub snippet_delta: i8,
    pub end_snippet: bool,
    /// Word under a Ctrl/Cmd+click. The editor does not navigate; the query surface does.
    pub clicked_identifier: Option<String>,
    pub wants_manual_completion: bool,
    pub wants_manual_prediction: bool,
    pub wants_execute_statement: bool,
    pub wants_execute_all: bool,
    pub wants_format: bool,
    pub wants_save: bool,
    pub wants_dismiss_prediction: bool,
    pub accepted_prediction_len: Option<usize>,
    pub hovered_token: Option<HoveredSqlToken>,
    pub focused: bool,
}

impl Default for SqlEditorResponse {
    fn default() -> Self {
        Self {
            changed: false,
            cursor_screen_pos: Pos2::ZERO,
            rect: Rect::NOTHING,
            wants_completion: false,
            wants_identifier_typed: false,
            snippet_delta: 0,
            end_snippet: false,
            clicked_identifier: None,
            wants_manual_completion: false,
            wants_manual_prediction: false,
            wants_execute_statement: false,
            wants_execute_all: false,
            wants_format: false,
            wants_save: false,
            wants_dismiss_prediction: false,
            accepted_prediction_len: None,
            hovered_token: None,
            focused: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairTextResult {
    NotHandled,
    Inserted,
    SkippedExistingClosing,
}

/// Composition string currently owned by the platform IME.
///
/// #570: the editor only handled `ImeEvent::Commit`, so a Vietnamese/CJK IME in
/// preedit mode showed nothing between keypresses — no composition text, no
/// underline, and a caret that never moved. The preedit is stored per editor in
/// egui's temp store (the renderer is rebuilt every frame, so a field cannot hold
/// it) and painted like ghost text with a composition underline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ImePreedit {
    text: String,
    /// Candidate window anchor in screen coordinates.
    cursor_range: Option<(usize, usize)>,
}

impl ImePreedit {
    fn from_event(event: &egui::ImeEvent) -> Option<Self> {
        match event {
            egui::ImeEvent::Preedit(text, cursor_range) => Some(Self {
                text: text.clone(),
                cursor_range: cursor_range.map(|(start, end)| (start.min(end), start.max(end))),
            }),
            _ => None,
        }
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Composition text clipped to `cursor_range`'s caret position.
    fn around_cursor(&self) -> (String, String) {
        match self.cursor_range {
            Some((caret, _)) => {
                let caret = caret.min(self.text.chars().count());
                let before: String = self.text.chars().take(caret).collect();
                let after: String = self.text.chars().skip(caret).collect();
                (before, after)
            }
            None => (self.text.clone(), String::new()),
        }
    }
}

fn ime_preedit_key(editor_id: Id) -> Id {
    editor_id.with("ime_preedit")
}

fn read_ime_preedit(ui: &Ui, editor_id: Id) -> ImePreedit {
    ui.ctx()
        .data(|data| data.get_temp::<ImePreedit>(ime_preedit_key(editor_id)))
        .unwrap_or_default()
}

pub struct SqlEditor<'a> {
    pub buffer: &'a mut TextBuffer,
    pub cursor: &'a mut CursorPosition,
    pub selection: &'a mut SelectionRange,
    pub dialect: SqlDialect,
    pub theme: &'a DbProTheme,
    pub diagnostics: &'a [Diagnostic],
    pub prediction: Option<&'a EditPrediction>,
    pub prediction_visible: bool,
    /// Token cache owned by the document (single-threaded UI), borrowed for the
    /// frame. The render pass clones the cache's shared `Arc<[SyntaxToken]>`
    /// instead of copying the token vector (#581).
    pub cached_tokens: Option<&'a mut CachedSqlTokens>,
    pub search_query: &'a str,
    pub active_search_match_index: usize,
    pub completion_open: bool,
    pub rich_hover: Option<HoveredSqlToken>,
    pub snippet_active: bool,
    pub snippet_range: Option<(usize, usize)>,
    pub auto_focus: bool,
    pub execution_range: Option<(usize, usize)>,
    pub font_size: f32,
    pub id_salt: &'a str,
}

/// Font metrics + content extents computed once per frame.
struct EditorMetrics {
    font_id: FontId,
    line_height: f32,
    char_width: f32,
    gutter_w: f32,
    line_count: usize,
    content_width: f32,
    content_height: f32,
}

/// Scroll offset for this frame: persisted temp value + this frame's wheel
/// delta, clamped to the content extent.
/// Register the editor as a widget and resolve focus for this frame.
fn bind_editor_widget(ui: &mut Ui, editor_id: Id, viewport: Rect, auto_focus: bool) -> (Response, bool) {
    let resp = ui.interact(viewport, editor_id, Sense::click_and_drag());
    if auto_focus || resp.clicked() {
        resp.request_focus();
    }
    resp.widget_info(|| text_input_info(true, "SQL query editor"));
    let focused = resp.has_focus();
    (resp, focused)
}

// cc-scan:allow TOO_MANY_PARAMS — service/view signature passes grouped context through
fn frame_scroll(ui: &Ui, editor_id: Id, max_scroll: Vec2, hovered: bool, focused: bool) -> Vec2 {
    let mut scroll: Vec2 = ui
        .ctx()
        .data(|d| d.get_temp::<Vec2>(editor_id.with("scroll")))
        .unwrap_or(Vec2::ZERO);
    if hovered || focused {
        scroll -= ui.input(|i| i.smooth_scroll_delta);
    }
    Vec2::new(scroll.x.clamp(0.0, max_scroll.x), scroll.y.clamp(0.0, max_scroll.y))
}

/// Per-frame layout shared by `show()`'s paint passes — computed once so every
/// section agrees on scroll-adjusted coordinates and font metrics.
struct EditorFrame<'a> {
    viewport: Rect,
    rect: Rect,
    gutter_w: f32,
    line_height: f32,
    char_width: f32,
    line_count: usize,
    scroll: Vec2,
    font_id: FontId,
    tokens: &'a [SyntaxToken],
    highlighter: &'a SqlHighlighter,
    focused: bool,
    editor_id: Id,
}

impl<'a> SqlEditor<'a> {
    // allow: constructor borrows buffer/cursor/selection simultaneously (&mut for each
    // distinct field) from egui's paint pass — an options struct would not reduce borrows, only move them.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        buffer: &'a mut TextBuffer,
        cursor: &'a mut CursorPosition,
        selection: &'a mut SelectionRange,
        dialect: SqlDialect,
        theme: &'a DbProTheme,
        diagnostics: &'a [Diagnostic],
        prediction: Option<&'a EditPrediction>,
        id_salt: &'a str,
    ) -> Self {
        Self {
            buffer,
            cursor,
            selection,
            dialect,
            theme,
            diagnostics,
            prediction,
            prediction_visible: true,
            cached_tokens: None,
            search_query: "",
            active_search_match_index: 0,
            completion_open: false,
            rich_hover: None,
            snippet_active: false,
            snippet_range: None,
            auto_focus: false,
            execution_range: None,
            font_size: FONT_SIZE,
            id_salt,
        }
    }

    pub fn with_cached_tokens(mut self, cached_tokens: &'a mut CachedSqlTokens) -> Self {
        self.cached_tokens = Some(cached_tokens);
        self
    }

    pub fn with_search(mut self, query: &'a str, active_match: usize) -> Self {
        self.search_query = query;
        self.active_search_match_index = active_match;
        self
    }

    pub fn with_completion_open(mut self, completion_open: bool) -> Self {
        self.completion_open = completion_open;
        self
    }

    /// Confirmed (delay-filtered) hover token. The anchor's card is drawn by
    /// the caller; the editor paints occurrence highlights for the symbol.
    pub fn with_rich_hover(mut self, token: Option<HoveredSqlToken>) -> Self {
        self.rich_hover = token;
        self
    }

    pub fn with_snippet(mut self, active: bool, range: Option<(usize, usize)>) -> Self {
        self.snippet_active = active;
        self.snippet_range = range;
        self
    }

    pub fn with_prediction_visible(mut self, visible: bool) -> Self {
        self.prediction_visible = visible;
        self
    }

    pub fn with_execution_range(mut self, range: Option<(usize, usize)>) -> Self {
        self.execution_range = range;
        self
    }

    pub fn with_auto_focus(mut self, auto_focus: bool) -> Self {
        self.auto_focus = auto_focus;
        self
    }

    pub fn gutter_width(&self, char_width: f32) -> f32 {
        let lines = self.buffer.line_count().max(1);
        let digits = lines.to_string().len().max(2);
        (digits as f32) * char_width + 30.0
    }

    /// Font metrics and content extents for one frame.
    fn measure(&self, ui: &Ui, available_size: Vec2) -> EditorMetrics {
        let font_id = FontId::monospace(self.font_size);
        let glyph_row = ui.fonts_mut(|f| f.row_height(&font_id));
        let line_height = (self.font_size * LINE_HEIGHT_MULT)
            .max(glyph_row)
            .max(DEFAULT_LINE_HEIGHT);
        let char_width = ui
            .fonts_mut(|f| f.glyph_width(&font_id, 'M'))
            .max(self.font_size * 0.55);
        let line_count = self.buffer.line_count().max(1);
        let max_line_chars = self.buffer.max_line_len_chars().max(40);
        let gutter_w = self.gutter_width(char_width);
        EditorMetrics {
            font_id,
            line_height,
            char_width,
            gutter_w,
            line_count,
            content_width: (gutter_w + PADDING_LEFT + (max_line_chars as f32) * char_width + 96.0)
                .max(available_size.x),
            content_height: (line_count as f32) * line_height + PADDING_TOP + PADDING_BOTTOM,
        }
    }

    /// Editor plane, focus hairline, and the pinned gutter strip — the chrome
    /// every other pass paints onto. The gutter must not slide away on
    /// horizontal scroll (Stitch gutter spec).
    // cc-scan:allow TOO_MANY_PARAMS — service/view signature passes grouped context through
    fn paint_chrome(&self, ui: &Ui, viewport: Rect, gutter_w: f32, focused: bool) {
        ui.painter().rect_filled(
            viewport,
            CornerRadius::same(EDITOR_ROUNDING as u8),
            self.theme.surface_editor,
        );
        if focused {
            ui.painter().rect_stroke(
                viewport,
                CornerRadius::same(EDITOR_ROUNDING as u8),
                Stroke::new(1.0, self.theme.border_subtle),
                egui::StrokeKind::Inside,
            );
        }
        let gutter_rect = Rect::from_min_max(
            Pos2::new(viewport.min.x, viewport.min.y),
            Pos2::new(viewport.min.x + gutter_w, viewport.max.y),
        );
        ui.painter()
            .rect_filled(gutter_rect, CornerRadius::ZERO, self.theme.editor_gutter_fill());
        ui.painter().vline(
            viewport.min.x + gutter_w,
            viewport.y_range(),
            Stroke::new(
                1.0,
                self.theme
                    .border_subtle
                    .linear_multiply(if self.theme.dark_mode { 0.55 } else { 0.7 }),
            ),
        );
    }

    // cc-scan:allow HUGE_FUNCTION — one egui frame orchestrator: measure → interact → dispatch paint passes
    pub fn show(mut self, ui: &mut Ui, available_size: Vec2) -> SqlEditorResponse {
        let mut response = SqlEditorResponse::default();
        // Stable id tied to the interactive rect so focus survives across frames.
        // Requesting focus on a bare `make_persistent_id` that never registers as a
        // widget makes egui clear focus on the next pass (click → one-frame focus → dead).
        let editor_id = ui.make_persistent_id(self.id_salt);
        let m = self.measure(ui, available_size);

        let (viewport, _) = ui.allocate_exact_size(available_size, Sense::hover());
        response.rect = viewport;
        let (resp, focused) = bind_editor_widget(ui, editor_id, viewport, self.auto_focus);
        response.focused = focused;

        let max_scroll = Vec2::new(
            (m.content_width - viewport.width()).max(0.0),
            (m.content_height - viewport.height()).max(0.0),
        );
        let mut scroll = frame_scroll(ui, editor_id, max_scroll, resp.hovered(), focused);

        let cursor_before = self.cursor.offset;
        let line_before = self.cursor.line;

        self.paint_chrome(ui, viewport, m.gutter_w, focused);
        ui.set_clip_rect(ui.clip_rect().intersect(viewport));

        // Content coordinate space (scroll by translating origin).
        let rect = Rect::from_min_size(
            viewport.min - scroll,
            Vec2::new(m.content_width.max(viewport.width()), m.content_height),
        );

        // Handle Keyboard Events when focused
        if focused {
            self.handle_keyboard_events(ui, editor_id, &mut response);
        }

        // Tokens after edits, so the caret, hit testing, and glyphs share one layout.
        let (highlighter, tokens) = self.fresh_tokens();
        let frame = EditorFrame {
            viewport,
            rect,
            gutter_w: m.gutter_w,
            line_height: m.line_height,
            char_width: m.char_width,
            line_count: m.line_count,
            scroll,
            font_id: m.font_id,
            tokens: tokens.as_ref(),
            highlighter: &highlighter,
            focused,
            editor_id,
        };

        // Mouse click & drag positioning
        self.handle_pointer(ui, &resp, &mut response, &frame);

        self.paint_highlights(ui, &frame);
        self.track_hovered_token(ui, &resp, &frame, &mut response);
        self.paint_visible_lines(ui, &frame);
        self.paint_diagnostics(ui, &frame);

        // Caret follows the same glyph advances as the painted line.
        let cursor_screen = self.offset_to_screen_pos_laid_out(
            ui,
            self.cursor.offset,
            frame.rect.min,
            frame.gutter_w,
            frame.line_height,
            &frame.font_id,
            frame.tokens,
        );
        response.cursor_screen_pos = Pos2::new(cursor_screen.x, cursor_screen.y + frame.line_height);

        self.paint_ghost_text(ui, &frame, cursor_screen);
        let preedit_cursor = self.paint_ime_preedit(ui, &frame, cursor_screen);
        self.paint_caret(
            ui,
            &frame,
            response.changed,
            cursor_before,
            line_before,
            preedit_cursor.unwrap_or(cursor_screen),
        );

        // Keep caret inside the viewport after edits / moves.
        scroll = self.scroll_caret_into_view(ui, &frame, scroll, max_scroll, cursor_screen);

        // Minimal scrollbar thumbs & overview ruler (diagnostic marks on vertical track).
        paint_editor_scrollbars(
            ui,
            viewport,
            scroll,
            max_scroll,
            self.diagnostics,
            self.buffer,
            self.theme,
        );

        response
    }

    /// Current-line wash, search matches, execution range, hover symbol
    /// occurrences, selection and delimiter pairing — all the "where am I"
    /// paint, drawn under the text.
    fn paint_highlights(&self, ui: &Ui, frame: &EditorFrame) {
        self.paint_current_line(ui, frame);
        self.paint_search_and_execution(ui, frame);
        self.paint_hover_occurrences(ui, frame);
        self.paint_selection_fill(ui, frame);
        self.paint_delimiter_marks(ui, frame);
    }

    /// Accent-washed current row + 2px accent bar; gutter keeps its own fill (Stitch spec).
    fn paint_current_line(&self, ui: &Ui, frame: &EditorFrame) {
        // Current Line Highlight — accent-washed code row + accent left bar
        // (Stitch `border-l-2 border-accent`); the gutter keeps its own fill.
        let current_line_top = frame.rect.min.y + PADDING_TOP + (self.cursor.line as f32) * frame.line_height;
        let current_line_rect = Rect::from_min_max(
            Pos2::new(frame.viewport.min.x + frame.gutter_w, current_line_top),
            Pos2::new(frame.viewport.max.x, current_line_top + frame.line_height),
        );
        if current_line_rect.intersects(frame.viewport) {
            ui.painter().rect_filled(
                current_line_rect,
                CornerRadius::ZERO,
                self.theme.editor_current_line_fill(),
            );
            ui.painter().rect_filled(
                Rect::from_min_size(current_line_rect.min, Vec2::new(2.0, frame.line_height)),
                CornerRadius::ZERO,
                self.theme.accent,
            );
        }
    }

    /// Search matches (active = accent ring) and the amber execution range.
    fn paint_search_and_execution(&self, ui: &Ui, frame: &EditorFrame) {
        // Search Matches Highlights
        if !self.search_query.trim().is_empty() {
            for (idx, (match_start, match_end)) in
                super::search::case_insensitive_matches(self.buffer.text(), self.search_query)
                    .iter()
                    .enumerate()
            {
                let (match_start, match_end) = (*match_start, *match_end);
                let is_active_match = idx == self.active_search_match_index;
                let rects = self.range_to_screen_rects(
                    self.buffer,
                    match_start,
                    match_end,
                    frame.rect.min,
                    frame.gutter_w,
                    frame.line_height,
                    frame.char_width,
                );
                for m_rect in rects {
                    if is_active_match {
                        ui.painter().rect_filled(
                            m_rect,
                            CornerRadius::same(2.0 as u8),
                            self.theme.accent.linear_multiply(0.4),
                        );
                        ui.painter().rect_stroke(
                            m_rect,
                            CornerRadius::same(2.0 as u8),
                            Stroke::new(1.5, self.theme.accent),
                            egui::StrokeKind::Inside,
                        );
                    } else {
                        ui.painter().rect_filled(
                            m_rect,
                            CornerRadius::same(2.0 as u8),
                            self.theme.warning.linear_multiply(0.28),
                        );
                    }
                }
            }
        }

        if let Some((execution_start, execution_end)) = self.execution_range {
            for execution_rect in self.range_to_screen_rects(
                self.buffer,
                execution_start,
                execution_end,
                frame.rect.min,
                frame.gutter_w,
                frame.line_height,
                frame.char_width,
            ) {
                ui.painter().rect_filled(
                    execution_rect,
                    CornerRadius::same(1.0 as u8),
                    self.theme.accent.linear_multiply(0.08),
                );
            }
        }
    }

    /// Every other occurrence of the confirmed-hover symbol, washed in accent.
    /// Runs only while the rich-hover card owns a token (500 ms FSM upstream).
    fn paint_hover_occurrences(&self, ui: &Ui, frame: &EditorFrame) {
        let Some(token) = self.rich_hover else {
            return;
        };
        // Same viewport window the text pass paints (incl. its ±2-line overscan)
        // so off-screen tokens cost no rects and the pass stays cheap.
        let first_visible_line =
            (((frame.scroll.y - PADDING_TOP) / frame.line_height).floor() as isize).max(0) as usize;
        let first_visible_line = first_visible_line.saturating_sub(2);
        let last_visible_line =
            ((((frame.scroll.y + frame.viewport.height() - PADDING_TOP) / frame.line_height).ceil() as usize) + 2)
                .min(frame.line_count);
        let visible_start = self.buffer.line_start_offset(first_visible_line);
        let visible_end = if last_visible_line >= self.buffer.line_count() {
            self.buffer.len_bytes()
        } else {
            self.buffer.line_start_offset(last_visible_line)
        };
        for (start, end) in hover_occurrence_ranges(
            self.buffer.text(),
            token.range,
            frame.tokens,
            visible_start,
            visible_end,
        ) {
            for occ_rect in self.range_to_screen_rects(
                self.buffer,
                start,
                end,
                frame.rect.min,
                frame.gutter_w,
                frame.line_height,
                frame.char_width,
            ) {
                if occ_rect.intersects(frame.viewport) {
                    ui.painter().rect_filled(
                        occ_rect,
                        CornerRadius::same(2.0 as u8),
                        self.theme.accent.linear_multiply(0.22),
                    );
                }
            }
        }
    }

    /// Selection fill, glyph-accurate.
    fn paint_selection_fill(&self, ui: &Ui, frame: &EditorFrame) {
        // Selection Highlight
        if !self.selection.is_empty() {
            let (sel_start, sel_end) = self.selection.normalized();
            let rects = self.range_to_screen_rects_laid_out(
                ui,
                self.buffer,
                sel_start,
                sel_end,
                frame.rect.min,
                frame.gutter_w,
                frame.line_height,
                frame.char_width,
                &frame.font_id,
                frame.tokens,
            );
            for s_rect in rects {
                ui.painter().rect_filled(
                    s_rect,
                    CornerRadius::same(2.0 as u8),
                    self.theme.editor_selection_fill(),
                );
            }
        }
    }

    /// Delimiter pair near the caret — accent for matched, warning for unmatched.
    fn paint_delimiter_marks(&self, ui: &Ui, frame: &EditorFrame) {
        // Matching delimiter highlight and a subtle warning for an unmatched
        // structural delimiter near the caret.
        if let Some(delimiter) = brackets::delimiter_near_cursor(self.buffer.text(), self.cursor.offset) {
            let delimiter_len = self.buffer.char_at(delimiter.delimiter).map_or(0, char::len_utf8);
            if delimiter_len > 0 {
                let color = if delimiter.matching.is_some() {
                    self.theme.accent
                } else {
                    self.theme.warning
                };
                for highlighted in self.range_to_screen_rects(
                    self.buffer,
                    delimiter.delimiter,
                    delimiter.delimiter + delimiter_len,
                    frame.rect.min,
                    frame.gutter_w,
                    frame.line_height,
                    frame.char_width,
                ) {
                    ui.painter().rect_stroke(
                        highlighted,
                        CornerRadius::same(2.0 as u8),
                        Stroke::new(1.2, color.linear_multiply(0.85)),
                        egui::StrokeKind::Inside,
                    );
                }
                if let Some(matching) = delimiter.matching {
                    let matching_len = self.buffer.char_at(matching).map_or(0, char::len_utf8);
                    for highlighted in self.range_to_screen_rects(
                        self.buffer,
                        matching,
                        matching + matching_len,
                        frame.rect.min,
                        frame.gutter_w,
                        frame.line_height,
                        frame.char_width,
                    ) {
                        ui.painter().rect_stroke(
                            highlighted,
                            CornerRadius::same(2.0 as u8),
                            Stroke::new(1.2, self.theme.accent.linear_multiply(0.85)),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
        }
    }

    /// While the pointer rests over a token, record it so the caller can
    /// open the rich-hover card — suppressed while completion owns the hover.
    // cc-scan:allow DEEP_NESTING,TOO_MANY_PARAMS — structure mirrors data depth
    fn track_hovered_token(&self, ui: &Ui, resp: &Response, frame: &EditorFrame, response: &mut SqlEditorResponse) {
        if resp.hovered() && !self.completion_open {
            if let Some(pointer) = ui.input(|input| input.pointer.hover_pos()) {
                if pointer.x >= frame.rect.min.x + frame.gutter_w {
                    let offset = self.screen_pos_to_offset_laid_out(
                        ui,
                        pointer,
                        frame.rect.min,
                        frame.gutter_w,
                        frame.line_height,
                        &frame.font_id,
                        frame.tokens,
                    );
                    let token_idx = frame.tokens.partition_point(|token| token.range.1 <= offset);
                    if let Some(token) = frame.tokens.get(token_idx).filter(|token| {
                        offset >= token.range.0
                            && offset < token.range.1
                            && matches!(
                                token.kind,
                                SyntaxTokenKind::Identifier
                                    | SyntaxTokenKind::Function
                                    | SyntaxTokenKind::Type
                                    | SyntaxTokenKind::Keyword
                            )
                    }) {
                        let anchor_rect = self
                            .range_to_screen_rects(
                                self.buffer,
                                token.range.0,
                                token.range.1,
                                frame.rect.min,
                                frame.gutter_w,
                                frame.line_height,
                                frame.char_width,
                            )
                            .into_iter()
                            .next();
                        if let Some(anchor_rect) = anchor_rect {
                            response.hovered_token = Some(HoveredSqlToken {
                                range: token.range,
                                anchor_rect,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Text lines + gutter numbers for the virtualized visible window only.
    fn paint_visible_lines(&self, ui: &Ui, frame: &EditorFrame) {
        // Virtualized visible lines (frame.viewport ∩ scrolled content).
        let first_visible_line =
            (((frame.scroll.y - PADDING_TOP) / frame.line_height).floor() as isize).max(0) as usize;
        let first_visible_line = first_visible_line.saturating_sub(2);
        let last_visible_line =
            ((((frame.scroll.y + frame.viewport.height() - PADDING_TOP) / frame.line_height).ceil() as usize) + 2)
                .min(frame.line_count);

        // Render Visible Text Lines & Gutter Numbers
        for line_idx in first_visible_line..last_visible_line {
            let line_y = frame.rect.min.y + PADDING_TOP + (line_idx as f32) * frame.line_height;

            // Gutter Line Number
            let line_num_str = format!("{}", line_idx + 1);
            let is_curr = line_idx == self.cursor.line;
            ui.painter().text(
                Pos2::new(
                    frame.viewport.min.x + frame.gutter_w - 8.0,
                    line_y + frame.line_height * 0.5,
                ),
                egui::Align2::RIGHT_CENTER,
                line_num_str,
                FontId::monospace((self.font_size - 1.5).clamp(11.0, 14.0)),
                self.theme.editor_line_number(is_curr),
            );

            let line_text = self.buffer.line_at(line_idx).unwrap_or("");
            if !line_text.is_empty() {
                let galley = layout_line(
                    ui,
                    self.buffer,
                    frame.tokens,
                    frame.highlighter,
                    self.theme,
                    line_idx,
                    &frame.font_id,
                );
                ui.painter().galley(
                    Pos2::new(frame.rect.min.x + frame.gutter_w + PADDING_LEFT, line_y),
                    galley,
                    self.theme.text_primary,
                );
            }
        }
    }

    /// Diagnostic squiggles plus their hover tooltip.
    // cc-scan:allow DEEP_NESTING,LONG_FUNCTION — per-diagnostic paint pass
    fn paint_diagnostics(&self, ui: &Ui, frame: &EditorFrame) {
        // Diagnostics Underlines (Squiggles) & Tooltip Hover
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
        for diag in self.diagnostics {
            let rects = self.range_to_screen_rects(
                self.buffer,
                diag.range.0,
                diag.range.1,
                frame.rect.min,
                frame.gutter_w,
                frame.line_height,
                frame.char_width,
            );
            let diag_color = match diag.severity {
                DiagnosticSeverity::Error => self.theme.danger,
                DiagnosticSeverity::Warning => self.theme.warning,
                _ => self.theme.accent,
            };

            for d_rect in &rects {
                let line_y = d_rect.bottom() - 2.0;
                ui.painter().line_segment(
                    [
                        Pos2::new(d_rect.left(), line_y),
                        Pos2::new(d_rect.right().max(d_rect.left() + 4.0), line_y),
                    ],
                    Stroke::new(1.5, diag_color),
                );

                if let Some(pos) = pointer_pos {
                    // A diagnostic tooltip must not fight the completion popup or the
                    // delayed rich-hover card — those popups own the hover real estate.
                    if d_rect.contains(pos) && !self.completion_open && self.rich_hover.is_none() {
                        let _ = egui::Tooltip::always_open(
                            ui.ctx().clone(),
                            ui.layer_id(),
                            egui::Id::new("diag_hover"),
                            egui::PopupAnchor::Pointer,
                        )
                        .show(|ui| {
                            ui.horizontal(|ui| {
                                let badge = match diag.severity {
                                    DiagnosticSeverity::Error => "Error",
                                    DiagnosticSeverity::Warning => "Warning",
                                    _ => "Info",
                                };
                                ui.colored_label(diag_color, badge);
                                if let Some(code) = &diag.code {
                                    ui.monospace(code);
                                }
                                ui.label(&diag.message);
                            });
                        });
                    }
                }
            }
        }
    }

    /// Inline AI prediction ghost text (suppressed while completion is open).
    // cc-scan:allow COMPLEXITY,DEEP_NESTING,LONG_FUNCTION — classifier/dispatch ladder — one case per branch
    fn paint_ghost_text(&self, ui: &Ui, frame: &EditorFrame, cursor_screen: Pos2) {
        // Inline AI Prediction Ghost Text (suppressed while completion popup is open)
        if !self.completion_open && self.prediction_visible {
            if let Some(pred) = self.prediction {
                if !pred.is_empty() && pred.anchor == self.cursor.offset {
                    let replacement_start = pred.replacement_range.0;
                    let replacement_end = pred.replacement_range.1;
                    let has_replacement = replacement_start < replacement_end
                        && replacement_end <= self.buffer.len_bytes()
                        && self.buffer.is_char_boundary(replacement_start)
                        && self.buffer.is_char_boundary(replacement_end);
                    let ghost_origin = if has_replacement {
                        self.offset_to_screen_pos(
                            self.buffer,
                            replacement_start,
                            frame.rect.min,
                            frame.gutter_w,
                            frame.line_height,
                            frame.char_width,
                        )
                    } else {
                        cursor_screen
                    };
                    if has_replacement {
                        for replacement_rect in self.range_to_screen_rects(
                            self.buffer,
                            replacement_start,
                            replacement_end,
                            frame.rect.min,
                            frame.gutter_w,
                            frame.line_height,
                            frame.char_width,
                        ) {
                            ui.painter().rect_stroke(
                                replacement_rect,
                                CornerRadius::same(2.0 as u8),
                                Stroke::new(1.0, self.theme.accent.linear_multiply(0.35)),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                    let lines: Vec<&str> = pred.text.split('\n').collect();
                    for (idx, line_str) in lines.iter().enumerate() {
                        let ghost_x = if idx == 0 {
                            ghost_origin.x
                        } else {
                            frame.rect.min.x + frame.gutter_w + PADDING_LEFT
                        };
                        let ghost_y = cursor_screen.y + (idx as f32) * frame.line_height;
                        ui.painter().text(
                            Pos2::new(ghost_x, ghost_y),
                            egui::Align2::LEFT_TOP,
                            *line_str,
                            frame.font_id.clone(),
                            self.theme.text_muted.linear_multiply(0.65),
                        );
                    }
                }
            }
        }
    }

    /// Caret bar with input-phase blink, plus the IME anchor rect that must
    /// stay put while the bar blinks off.
    /// Paint the IME composition string at the caret.
    ///
    /// Returns the screen position just after the composition text so the caret
    /// and candidate window follow the composing characters instead of staying
    /// pinned at the buffer offset (#570).
    fn paint_ime_preedit(&self, ui: &Ui, frame: &EditorFrame, cursor_screen: Pos2) -> Option<Pos2> {
        if !frame.focused {
            return None;
        }
        let preedit = read_ime_preedit(ui, frame.editor_id);
        if preedit.is_empty() {
            return None;
        }
        // Composition text is drawn where the commit will land, with the
        // underline every IME shows while a composition is active.
        let galley =
            ui.painter()
                .layout_no_wrap(preedit.text.clone(), frame.font_id.clone(), self.theme.text_primary);
        let origin = cursor_screen;
        let rect = Rect::from_min_size(origin, galley.size());
        ui.painter().galley(origin, galley, self.theme.text_primary);
        ui.painter().line_segment(
            [
                Pos2::new(rect.min.x, rect.max.y),
                Pos2::new(rect.max.x, rect.max.y),
            ],
            Stroke::new(STROKE_THIN, self.theme.accent),
        );
        // Caret sits inside the composition string, at the IME's caret index.
        let (before, _) = preedit.around_cursor();
        let before_width = ui
            .painter()
            .layout_no_wrap(before, frame.font_id.clone(), self.theme.text_primary)
            .size()
            .x;
        Some(Pos2::new(cursor_screen.x + before_width, cursor_screen.y))
    }

    fn paint_caret(
        &self,
        ui: &Ui,
        frame: &EditorFrame,
        changed: bool,
        cursor_before: usize,
        line_before: usize,
        cursor_screen: Pos2,
    ) {
        // Caret — 2px bar in the text color. Solid after input, then a 530ms blink
        // whose phase starts from that input so it does not vanish mid-cycle.
        // The IME rect stays up while the bar is hidden, or the candidate window jumps.
        if frame.focused {
            let now = ui.input(|i| i.time);
            let cursor_moved = self.cursor.offset != cursor_before || self.cursor.line != line_before;
            if changed || cursor_moved {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(frame.editor_id.with("last_input"), now));
            }
            let last_input = ui
                .ctx()
                .data(|d| d.get_temp::<f64>(frame.editor_id.with("last_input")))
                .unwrap_or(now);
            let since = (now - last_input).max(0.0);
            let cycle = CARET_BLINK_ON_SECS + CARET_BLINK_OFF_SECS;
            let blink_on = self.theme.reduce_motion || {
                if since < CARET_SOLID_AFTER_INPUT_SECS {
                    true
                } else {
                    (since - CARET_SOLID_AFTER_INPUT_SECS) % cycle < CARET_BLINK_ON_SECS
                }
            };
            let pixels = ui.ctx().pixels_per_point();
            let caret_x = (cursor_screen.x * pixels).floor() / pixels;
            let caret_w = (2.0f32 * pixels).round().max(1.0) / pixels;
            let cursor_rect = Rect::from_min_size(
                Pos2::new(caret_x, cursor_screen.y),
                Vec2::new(caret_w, frame.line_height),
            );
            if blink_on {
                ui.painter()
                    .rect_filled(cursor_rect, CornerRadius::ZERO, self.theme.text_primary);
            }
            ui.output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    purpose: egui::IMEPurpose::Normal,
                    rect: cursor_rect,
                    cursor_rect,
                    should_interrupt_composition: false,
                });
            });
            let next_secs = if self.theme.reduce_motion {
                cycle
            } else if since < CARET_SOLID_AFTER_INPUT_SECS {
                CARET_SOLID_AFTER_INPUT_SECS - since
            } else {
                let phase = (since - CARET_SOLID_AFTER_INPUT_SECS) % cycle;
                if phase < CARET_BLINK_ON_SECS {
                    CARET_BLINK_ON_SECS - phase
                } else {
                    cycle - phase
                }
            };
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(next_secs.clamp(0.016, cycle)));
        }
    }

    /// Re-retokenize after edits; caret, hit testing, and glyphs share one layout.
    /// Tokens for this frame plus the highlighter that produced them.
    ///
    /// #581: this used to return `cache.tokens().to_vec()` — a full clone of the
    /// document's token vector on every frame, including the ~500 ms caret-blink
    /// repaints. The cached slice is borrowed instead; only the uncached path
    /// (no `cached_tokens` handle) allocates, and it does so once per frame.
    fn fresh_tokens(&mut self) -> (SqlHighlighter, std::sync::Arc<[SyntaxToken]>) {
        let highlighter = SqlHighlighter::new(self.dialect);
        let buffer = &*self.buffer;
        if let Some(cache) = self.cached_tokens.as_deref_mut() {
            // The token list is cached per document; every keystroke past the
            // edit already reuses it, so a caret-blink repaint copies no tokens
            // — only an `Arc` reference (#581).
            return (highlighter, cache.shared(buffer, self.dialect));
        }
        (highlighter, highlighter.tokenize(self.buffer.text()).into())
    }

    /// Scroll just enough that the caret stays inside the viewport, then persist.
    fn scroll_caret_into_view(
        &self,
        ui: &Ui,
        frame: &EditorFrame,
        mut scroll: Vec2,
        max_scroll: Vec2,
        cursor_screen: Pos2,
    ) -> Vec2 {
        let caret_local = Pos2::new(cursor_screen.x - frame.rect.min.x, cursor_screen.y - frame.rect.min.y);
        let margin = 8.0;
        if caret_local.y < scroll.y + margin {
            scroll.y = (caret_local.y - margin).max(0.0);
        } else if caret_local.y + frame.line_height > scroll.y + frame.viewport.height() - margin {
            scroll.y = (caret_local.y + frame.line_height - frame.viewport.height() + margin).max(0.0);
        }
        if caret_local.x < scroll.x + frame.gutter_w + margin {
            scroll.x = (caret_local.x - frame.gutter_w - margin).max(0.0);
        } else if caret_local.x > scroll.x + frame.viewport.width() - margin {
            scroll.x = (caret_local.x - frame.viewport.width() + margin).max(0.0);
        }
        let scroll = Vec2::new(scroll.x.clamp(0.0, max_scroll.x), scroll.y.clamp(0.0, max_scroll.y));
        ui.ctx()
            .data_mut(|d| d.insert_temp(frame.editor_id.with("scroll"), scroll));
        scroll
    }

    /// Click, double/triple-click, and drag positioning. Gutter clicks select
    /// the whole line; text clicks go through glyph-accurate hit testing.
    // cc-scan:allow COMPLEXITY,TOO_MANY_PARAMS — classifier/dispatch ladder — one case per branch
    fn handle_pointer(&mut self, ui: &Ui, resp: &Response, response: &mut SqlEditorResponse, frame: &EditorFrame) {
        // Mouse click & drag positioning
        let shift_pressed = ui.input(|i| i.modifiers.shift);
        let command_click = ui.input(|i| i.modifiers.command || i.modifiers.ctrl || i.modifiers.mac_cmd);
        let Some(mouse_pos) = resp.interact_pointer_pos() else {
            return;
        };
        if resp.double_clicked() {
            let offset = self.hit_offset(ui, frame, mouse_pos);
            self.selection.select_word_at(self.buffer, offset);
            self.cursor.set_offset(self.buffer, self.selection.active);
        } else if resp.triple_clicked() {
            let target_line = self.line_at_pointer(frame, mouse_pos);
            self.selection.select_line_at(self.buffer, target_line);
            self.cursor.set_offset(self.buffer, self.selection.active);
        } else if resp.drag_started() {
            let offset = self.hit_offset(ui, frame, mouse_pos);
            self.cursor.set_offset(self.buffer, offset);
            if !shift_pressed {
                *self.selection = SelectionRange::point(offset);
            } else {
                self.selection.grow_to(offset);
            }
        } else if resp.dragged() {
            let offset = self.hit_offset(ui, frame, mouse_pos);
            self.cursor.set_offset(self.buffer, offset);
            self.selection.grow_to(offset);
        } else if resp.clicked() {
            // Check if click was on gutter
            if mouse_pos.x < frame.rect.min.x + frame.gutter_w {
                let target_line = self.line_at_pointer(frame, mouse_pos);
                self.selection.select_line_at(self.buffer, target_line);
                self.cursor.set_offset(self.buffer, self.selection.active);
            } else {
                let offset = self.hit_offset(ui, frame, mouse_pos);
                self.cursor.set_offset(self.buffer, offset);
                if shift_pressed {
                    self.selection.grow_to(offset);
                } else {
                    self.selection.collapse_to_active();
                }
                if command_click {
                    if let Some(name) = identifier_at(self.buffer, offset) {
                        response.clicked_identifier = Some(name);
                    }
                }
            }
        }
    }

    /// Glyph-accurate hit test for a text-area pointer position.
    fn hit_offset(&self, ui: &Ui, frame: &EditorFrame, mouse_pos: Pos2) -> usize {
        self.screen_pos_to_offset_laid_out(
            ui,
            mouse_pos,
            frame.rect.min,
            frame.gutter_w,
            frame.line_height,
            &frame.font_id,
            frame.tokens,
        )
    }

    /// Gutter clicks map a y position to a whole line.
    fn line_at_pointer(&self, frame: &EditorFrame, mouse_pos: Pos2) -> usize {
        let rel_y = mouse_pos.y - (frame.rect.min.y + PADDING_TOP);
        ((rel_y / frame.line_height).floor() as usize).min(frame.line_count.saturating_sub(1))
    }

    /// IME composition and commit (#570).
    // cc-scan:allow TOO_MANY_PARAMS — event, frame context and response are all distinct concerns
    ///
    /// `Preedit` updates the composition state that the paint pass draws;
    /// `Commit` inserts the text and clears it; every other IME event (blur,
    /// cancellation, `Enabled`/`Disabled`) clears it so nothing lingers.
    fn apply_ime_event(
        &mut self,
        event: &egui::ImeEvent,
        ui: &Ui,
        editor_id: Id,
        response: &mut SqlEditorResponse,
    ) {
        let key = ime_preedit_key(editor_id);
        if let egui::ImeEvent::Commit(text) = event {
            if !text.is_empty() {
                self.type_text(text);
                response.changed = true;
                response.wants_identifier_typed = is_single_identifier_char(text);
                if text == "." {
                    response.wants_completion = true;
                }
            }
            ui.ctx()
                .data_mut(|data| data.insert_temp(key, ImePreedit::default()));
            return;
        }
        let preedit = ImePreedit::from_event(event).unwrap_or_default();
        ui.ctx().data_mut(|data| data.insert_temp(key, preedit));
    }

    /// Keyboard input while the editor owns focus. The completion popup owns
    /// Enter/Tab while open; everything else mutates buffer/cursor/selection
    /// and folds into `response` for the caller.
    // cc-scan:allow COMPLEXITY,DEEP_NESTING,LONG_FUNCTION — event loop — one match arm per event kind
    fn handle_keyboard_events(&mut self, ui: &Ui, editor_id: Id, response: &mut SqlEditorResponse) {
        let events = ui.input(|i| i.events.clone());
        let modifiers = ui.input(|i| i.modifiers);
        let cmd_or_ctrl = modifiers.command || modifiers.ctrl || modifiers.mac_cmd;

        if let Some(cache) = self.cached_tokens.as_mut() {
            cache.get_or_recompute(self.buffer, self.dialect);
        }

        for event in events {
            match event {
                Event::Key {
                    key,
                    pressed: true,
                    modifiers: event_mods,
                    ..
                } => {
                    self.apply_key(key, event_mods, response);
                }
                Event::Text(text) if !cmd_or_ctrl && !text.is_empty() => {
                    match self.handle_pair_text(&text) {
                        PairTextResult::Inserted => {
                            response.changed = true;
                            continue;
                        }
                        PairTextResult::SkippedExistingClosing => continue,
                        PairTextResult::NotHandled => {}
                    }
                    self.type_text(&text);
                    response.changed = true;
                    response.wants_identifier_typed = is_single_identifier_char(&text);
                    if text == "." {
                        response.wants_completion = true;
                    }
                }
                Event::Ime(event) => self.apply_ime_event(&event, ui, editor_id, response),
                Event::Cut if !self.selection.is_empty() => {
                    self.buffer.break_typing_group();
                    let (start, end) = self.selection.normalized();
                    let text = self.buffer.slice(start, end).to_owned();
                    ui.output_mut(|o| o.commands.push(egui::OutputCommand::CopyText(text)));
                    self.delete_selection();
                    response.changed = true;
                }
                Event::Copy if !self.selection.is_empty() => {
                    let (start, end) = self.selection.normalized();
                    let text = self.buffer.slice(start, end).to_owned();
                    ui.output_mut(|o| o.commands.push(egui::OutputCommand::CopyText(text)));
                }
                Event::Paste(text) if !text.is_empty() => {
                    self.buffer.break_typing_group();
                    self.insert_text(&text);
                    response.changed = true;
                }
                _ => {}
            }
        }
    }

    /// Key→edit-action dispatch; each arm is one cohesive editing op on buffer/cursor/selection.
    // cc-scan:allow HUGE_FUNCTION,HARD_COMPLEXITY,DEEP_NESTING — flat key dispatch; arms are single-purpose edit ops
    fn apply_key(&mut self, key: Key, event_mods: egui::Modifiers, response: &mut SqlEditorResponse) {
        let is_cmd = event_mods.command || event_mods.ctrl || event_mods.mac_cmd;
        let shift = event_mods.shift;
        match key {
            Key::Enter => {
                // Completion owns Enter/Tab while open — do not insert a newline.
                // Run and run-all are catalog chords. The app consumes those key
                // events before the editor sees them, so Enter that arrives here
                // is a newline, including Ctrl/Cmd+Enter when it is not bound.
                if self.completion_open {
                    return;
                }
                // Enter with an open selection must not replace it — that wiped
                // the whole buffer on Ctrl+A → Enter (#342). Collapse to the
                // selection end and insert the newline there instead.
                if !self.selection.is_empty() {
                    let (_, end) = self.selection.normalized();
                    *self.selection = SelectionRange::point(end);
                    self.cursor.set_offset(self.buffer, end);
                }
                self.buffer.break_typing_group();
                let current_line_text = self.buffer.line_at(self.cursor.line).unwrap_or("");
                let leading_spaces: String = current_line_text
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .collect();
                let insert_text = format!("\n{leading_spaces}");
                // `type_text` keeps the selection intact for the next keypress;
                // `insert_text` would delete the (now empty) selection anyway.
                let offset = self.cursor.offset;
                self.buffer.insert(offset, &insert_text);
                self.cursor.set_offset(self.buffer, offset + insert_text.len());
                self.selection.collapse_to_active();
                response.changed = true;
            }
            Key::Backspace => {
                self.buffer.break_typing_group();
                if !self.selection.is_empty() {
                    self.delete_selection();
                    response.changed = true;
                } else if self.cursor.offset > 0 {
                    let prev_off = self.buffer.prev_grapheme_boundary(self.cursor.offset);
                    let delete_end = brackets::auto_pair_range(self.buffer.text(), self.cursor.offset)
                        .map_or(self.cursor.offset, |(_, end)| end);
                    self.buffer.delete(prev_off, delete_end);
                    self.cursor.set_offset(self.buffer, prev_off);
                    self.selection.collapse_to_active();
                    response.changed = true;
                }
            }
            Key::Delete => {
                self.buffer.break_typing_group();
                if !self.selection.is_empty() {
                    self.delete_selection();
                    response.changed = true;
                } else if self.cursor.offset < self.buffer.len_bytes() {
                    let next_off = self.buffer.next_grapheme_boundary(self.cursor.offset);
                    self.buffer.delete(self.cursor.offset, next_off);
                    self.selection.collapse_to_active();
                    response.changed = true;
                }
            }
            Key::Tab => {
                self.buffer.break_typing_group();
                // If completion popup is open, do not consume Tab; let completion accept it
                if self.completion_open {
                    return;
                }
                if self.snippet_active {
                    response.snippet_delta = if shift { -1 } else { 1 };
                    return;
                }

                // Accept AI prediction on Tab if prediction is active and no popup
                if let Some(pred) = self.prediction {
                    if !pred.is_empty() && pred.anchor == self.cursor.offset && !shift {
                        let text_to_insert = pred.accept_full().to_owned();
                        let len = text_to_insert.len();
                        self.insert_prediction_text(pred.replacement_range, &text_to_insert);
                        response.accepted_prediction_len = Some(len);
                        response.changed = true;
                        return;
                    }
                }

                if !self.selection.is_empty() {
                    if shift {
                        self.unindent_selection();
                    } else {
                        self.indent_selection();
                    }
                    response.changed = true;
                } else if shift {
                    self.unindent_line();
                    response.changed = true;
                } else {
                    self.insert_text("  ");
                    response.changed = true;
                }
            }
            Key::ArrowLeft => {
                self.buffer.break_typing_group();
                if is_cmd {
                    self.cursor.move_word_left(self.buffer);
                } else {
                    self.cursor.move_left(self.buffer);
                }
                self.update_selection(shift);
            }
            Key::ArrowRight => {
                self.buffer.break_typing_group();
                if event_mods.alt && !self.completion_open {
                    if let Some(pred) = self.prediction {
                        if !pred.is_empty() && pred.anchor == self.cursor.offset {
                            let word = pred.accept_next_word().to_owned();
                            if !word.is_empty() {
                                let len = word.len();
                                self.insert_prediction_text(pred.replacement_range, &word);
                                response.accepted_prediction_len = Some(len);
                                response.changed = true;
                                return;
                            }
                        }
                    }
                }
                if is_cmd {
                    self.cursor.move_word_right(self.buffer);
                } else {
                    self.cursor.move_right(self.buffer);
                }
                self.update_selection(shift);
            }
            Key::ArrowUp => {
                if self.completion_open {
                    return;
                }
                self.buffer.break_typing_group();
                if is_cmd {
                    self.cursor.move_doc_start(self.buffer);
                } else {
                    self.cursor.move_up(self.buffer);
                }
                self.update_selection(shift);
            }
            Key::ArrowDown => {
                if self.completion_open {
                    return;
                }
                self.buffer.break_typing_group();
                if event_mods.alt && !self.completion_open {
                    if let Some(pred) = self.prediction {
                        if !pred.is_empty() && pred.anchor == self.cursor.offset {
                            let line = pred.accept_next_line().to_owned();
                            if !line.is_empty() {
                                let len = line.len();
                                self.insert_prediction_text(pred.replacement_range, &line);
                                response.accepted_prediction_len = Some(len);
                                response.changed = true;
                                return;
                            }
                        }
                    }
                }
                if is_cmd {
                    self.cursor.move_doc_end(self.buffer);
                } else {
                    self.cursor.move_down(self.buffer);
                }
                self.update_selection(shift);
            }
            Key::Home => {
                self.buffer.break_typing_group();
                self.cursor.move_home(self.buffer);
                self.update_selection(shift);
            }
            Key::End => {
                self.buffer.break_typing_group();
                self.cursor.move_end(self.buffer);
                self.update_selection(shift);
            }
            Key::PageUp => {
                if self.completion_open {
                    return;
                }
                self.buffer.break_typing_group();
                self.cursor.move_page_up(self.buffer, 15);
                self.update_selection(shift);
            }
            Key::PageDown => {
                if self.completion_open {
                    return;
                }
                self.buffer.break_typing_group();
                self.cursor.move_page_down(self.buffer, 15);
                self.update_selection(shift);
            }
            Key::A if is_cmd => {
                self.buffer.break_typing_group();
                self.selection.select_all(self.buffer.len_bytes());
                self.cursor.set_offset(self.buffer, self.buffer.len_bytes());
            }
            Key::Z if is_cmd => {
                self.buffer.break_typing_group();
                if shift {
                    if let Some((cur, anchor)) = self.buffer.redo() {
                        self.cursor.set_offset(self.buffer, cur);
                        *self.selection = SelectionRange::new(anchor, cur);
                        response.changed = true;
                    }
                } else if let Some((cur, anchor)) = self.buffer.undo() {
                    self.cursor.set_offset(self.buffer, cur);
                    *self.selection = SelectionRange::new(anchor, cur);
                    response.changed = true;
                }
            }
            Key::Space if is_cmd && event_mods.alt => {
                response.wants_manual_prediction = true;
            }
            Key::S if is_cmd => {
                response.wants_save = true;
            }
            Key::Space if is_cmd => {
                response.wants_completion = true;
                response.wants_manual_completion = true;
            }
            Key::Escape => {
                self.selection.collapse_to_active();
                response.wants_dismiss_prediction = true;
                if self.snippet_active {
                    response.end_snippet = true;
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn screen_pos_to_offset_laid_out(
        &self,
        ui: &Ui,
        pos: Pos2,
        origin: Pos2,
        gutter_w: f32,
        line_height: f32,
        font_id: &FontId,
        tokens: &[SyntaxToken],
    ) -> usize {
        let rel_y = pos.y - (origin.y + PADDING_TOP);
        let target_line = ((rel_y / line_height).floor() as usize).min(self.buffer.line_count().saturating_sub(1));
        let highlighter = SqlHighlighter::new(self.dialect);
        let galley = layout_line(ui, self.buffer, tokens, &highlighter, self.theme, target_line, font_id);
        let local_x = pos.x - (origin.x + gutter_w + PADDING_LEFT);
        let col = char_index_at_x(&galley, local_x);
        self.buffer.line_col_to_offset(target_line, col)
    }

    #[allow(clippy::too_many_arguments)]
    fn offset_to_screen_pos_laid_out(
        &self,
        ui: &Ui,
        offset: usize,
        origin: Pos2,
        gutter_w: f32,
        line_height: f32,
        font_id: &FontId,
        tokens: &[SyntaxToken],
    ) -> Pos2 {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let highlighter = SqlHighlighter::new(self.dialect);
        let galley = layout_line(ui, self.buffer, tokens, &highlighter, self.theme, line, font_id);
        Pos2::new(
            origin.x + gutter_w + PADDING_LEFT + glyph_x(&galley, col),
            origin.y + PADDING_TOP + (line as f32) * line_height,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn range_to_screen_rects_laid_out(
        &self,
        ui: &Ui,
        buffer: &TextBuffer,
        start_offset: usize,
        end_offset: usize,
        origin: Pos2,
        gutter_w: f32,
        line_height: f32,
        char_width: f32,
        font_id: &FontId,
        tokens: &[SyntaxToken],
    ) -> Vec<Rect> {
        let (start_line, start_col) = buffer.offset_to_line_col(start_offset);
        let (end_line, end_col) = buffer.offset_to_line_col(end_offset);
        let highlighter = SqlHighlighter::new(self.dialect);
        let mut rects = Vec::with_capacity(end_line - start_line + 1);
        for line_idx in start_line..=end_line {
            let line_y = origin.y + PADDING_TOP + (line_idx as f32) * line_height;
            let galley = layout_line(ui, buffer, tokens, &highlighter, self.theme, line_idx, font_id);
            let col_start = if line_idx == start_line { start_col } else { 0 };
            let (col_end, covers_break) = if line_idx == end_line {
                (end_col, false)
            } else {
                (buffer.line_at(line_idx).unwrap_or("").chars().count(), true)
            };
            let x1 = origin.x + gutter_w + PADDING_LEFT + glyph_x(&galley, col_start);
            let mut x2 = origin.x + gutter_w + PADDING_LEFT + glyph_x(&galley, col_end);
            if covers_break {
                x2 += char_width;
            }
            rects.push(Rect::from_min_max(
                Pos2::new(x1, line_y),
                Pos2::new(x2.max(x1), line_y + line_height),
            ));
        }
        rects
    }

    pub fn offset_to_screen_pos(
        &self,
        buffer: &TextBuffer,
        offset: usize,
        origin: Pos2,
        gutter_w: f32,
        line_height: f32,
        char_width: f32,
    ) -> Pos2 {
        let (line, col) = buffer.offset_to_line_col(offset);
        let x = origin.x + gutter_w + PADDING_LEFT + (col as f32) * char_width;
        let y = origin.y + PADDING_TOP + (line as f32) * line_height;
        Pos2::new(x, y)
    }

    pub fn screen_pos_to_offset(
        &self,
        buffer: &TextBuffer,
        pos: Pos2,
        origin: Pos2,
        gutter_w: f32,
        line_height: f32,
        char_width: f32,
    ) -> usize {
        let rel_x = pos.x - (origin.x + gutter_w + PADDING_LEFT);
        let rel_y = pos.y - (origin.y + PADDING_TOP);
        let target_line = ((rel_y / line_height).floor() as usize).min(buffer.line_count().saturating_sub(1));
        let target_col = (rel_x / char_width).round().max(0.0) as usize;
        buffer.line_col_to_offset(target_line, target_col)
    }

    // allow: parameters are current layout dimensions (origin, gutter, line height,
    // char width) for pure geometric calculation — order mirrors viewport transformation.
    #[allow(clippy::too_many_arguments)]
    pub fn range_to_screen_rects(
        &self,
        buffer: &TextBuffer,
        start_offset: usize,
        end_offset: usize,
        origin: Pos2,
        gutter_w: f32,
        line_height: f32,
        char_width: f32,
    ) -> Vec<Rect> {
        let (start_line, start_col) = buffer.offset_to_line_col(start_offset);
        let (end_line, end_col) = buffer.offset_to_line_col(end_offset);
        let mut rects = Vec::with_capacity(end_line - start_line + 1);

        for l_idx in start_line..=end_line {
            let line_y = origin.y + PADDING_TOP + (l_idx as f32) * line_height;
            let col_start = if l_idx == start_line { start_col } else { 0 };
            let col_end = if l_idx == end_line {
                end_col
            } else {
                buffer.line_at(l_idx).unwrap_or("").chars().count() + 1
            };
            let x1 = origin.x + gutter_w + PADDING_LEFT + (col_start as f32) * char_width;
            let width = ((col_end.saturating_sub(col_start)) as f32) * char_width;
            rects.push(Rect::from_min_size(
                Pos2::new(x1, line_y),
                Vec2::new(width, line_height),
            ));
        }

        rects
    }

    fn insert_text(&mut self, text: &str) {
        if !self.selection.is_empty() {
            self.delete_selection();
        }
        let offset = self.cursor.offset;
        self.buffer.insert(offset, text);
        self.cursor.set_offset(self.buffer, offset + text.len());
        self.selection.collapse_to_active();
    }

    fn handle_pair_text(&mut self, text: &str) -> PairTextResult {
        let mut characters = text.chars();
        let Some(character) = characters.next() else {
            return PairTextResult::NotHandled;
        };
        if characters.next().is_some() {
            return PairTextResult::NotHandled;
        }

        if brackets::closing_pair(character)
            && self.selection.is_empty()
            && self.buffer.char_at(self.cursor.offset) == Some(character)
        {
            let next = self.buffer.next_char_boundary(self.cursor.offset);
            self.cursor.set_offset(self.buffer, next);
            self.selection.collapse_to_active();
            return PairTextResult::SkippedExistingClosing;
        }

        let Some(closing) = brackets::opening_pair(character) else {
            return PairTextResult::NotHandled;
        };
        if self
            .cached_tokens
            .as_ref()
            .is_some_and(|cache| cache.is_in_string_or_comment(self.cursor.offset.saturating_sub(1)))
        {
            return PairTextResult::NotHandled;
        }

        if !self.selection.is_empty() {
            self.wrap_selection(character, closing);
        } else {
            let offset = self.cursor.offset;
            let before = EditorSnapshot {
                cursor_offset: offset,
                anchor_offset: self.selection.anchor,
            };
            let after = EditorSnapshot {
                cursor_offset: offset + character.len_utf8(),
                anchor_offset: offset + character.len_utf8(),
            };
            let pair = format!("{character}{closing}");
            self.buffer.replace_with_snapshot(offset, offset, &pair, before, after);
            self.cursor.set_offset(self.buffer, offset + character.len_utf8());
            self.selection.collapse_to_active();
        }
        PairTextResult::Inserted
    }

    fn wrap_selection(&mut self, opening: char, closing: char) {
        let (start, end) = self.selection.normalized();
        let selected = self.buffer.slice(start, end).to_owned();
        let wrapped = format!("{opening}{selected}{closing}");
        let before = EditorSnapshot {
            cursor_offset: self.cursor.offset,
            anchor_offset: self.selection.anchor,
        };
        let inner_start = start + opening.len_utf8();
        let inner_end = inner_start + selected.len();
        let (anchor, active) = if self.selection.anchor <= self.selection.active {
            (inner_start, inner_end)
        } else {
            (inner_end, inner_start)
        };
        self.buffer.replace_with_snapshot(
            start,
            end,
            &wrapped,
            before,
            EditorSnapshot {
                cursor_offset: active,
                anchor_offset: anchor,
            },
        );
        self.cursor.set_offset(self.buffer, active);
        *self.selection = SelectionRange::new(anchor, active);
    }

    fn insert_prediction_text(&mut self, replacement_range: (usize, usize), text: &str) {
        let (start, end) = replacement_range;
        self.buffer.replace(start, end, text);
        self.cursor.set_offset(self.buffer, start + text.len());
        self.selection.collapse_to_active();
    }

    fn type_text(&mut self, text: &str) {
        if let Some((start, end)) = self.snippet_range {
            if self.selection.normalized() == (start, end) && start < end {
                self.buffer.replace(start, end, text);
                let cursor = start + text.len();
                self.cursor.set_offset(self.buffer, cursor);
                *self.selection = super::selection::SelectionRange::point(cursor);
                return;
            }
        }
        if !self.selection.is_empty() {
            self.delete_selection();
        }
        let offset = self.cursor.offset;
        let before = super::buffer::EditorSnapshot {
            cursor_offset: offset,
            anchor_offset: self.selection.anchor,
        };
        let after = super::buffer::EditorSnapshot {
            cursor_offset: offset + text.len(),
            anchor_offset: offset + text.len(),
        };
        self.buffer.type_text(offset, text, before, after);
        self.cursor.set_offset(self.buffer, offset + text.len());
        self.selection.collapse_to_active();
    }

    fn delete_selection(&mut self) {
        let (start, end) = self.selection.normalized();
        if start < end {
            self.buffer.delete(start, end);
            self.cursor.set_offset(self.buffer, start);
            self.selection.collapse_to_active();
        }
    }

    fn update_selection(&mut self, shift: bool) {
        self.selection.active = self.cursor.offset;
        if !shift {
            self.selection.collapse_to_active();
        }
    }

    fn indent_selection(&mut self) {
        let (start, end) = self.selection.normalized();
        let (start_line, _) = self.buffer.offset_to_line_col(start);
        let (end_line, end_col) = self.buffer.offset_to_line_col(end);
        let actual_end_line = if end_col == 0 && end_line > start_line {
            end_line - 1
        } else {
            end_line
        };

        for l_idx in (start_line..=actual_end_line).rev() {
            let l_start = self.buffer.line_start_offset(l_idx);
            self.buffer.insert(l_start, "  ");
        }
        let total_added = (actual_end_line - start_line + 1) * 2;
        *self.selection = SelectionRange::new(start + 2, end + total_added);
        self.cursor.set_offset(self.buffer, self.selection.active);
    }

    fn unindent_selection(&mut self) {
        let (start, end) = self.selection.normalized();
        let (start_line, _) = self.buffer.offset_to_line_col(start);
        let (end_line, end_col) = self.buffer.offset_to_line_col(end);
        let actual_end_line = if end_col == 0 && end_line > start_line {
            end_line - 1
        } else {
            end_line
        };

        let mut removed_before_start = 0usize;
        let mut removed_before_end = 0usize;
        for l_idx in (start_line..=actual_end_line).rev() {
            let l_text = self.buffer.line_at(l_idx).unwrap_or("");
            let l_start = self.buffer.line_start_offset(l_idx);
            let removed = if l_text.starts_with("  ") {
                self.buffer.delete(l_start, l_start + 2);
                2
            } else if l_text.starts_with(' ') || l_text.starts_with('\t') {
                self.buffer.delete(l_start, l_start + 1);
                1
            } else {
                0
            };
            // Only bytes actually deleted before each edge shift it — a line
            // with no indent must not move the anchor (#470).
            removed_before_start += removed.min(start.saturating_sub(l_start));
            removed_before_end += removed.min(end.saturating_sub(l_start));
        }
        let (new_lo, new_hi) = (start - removed_before_start, end.saturating_sub(removed_before_end));
        // Keep the original anchor/active direction.
        if self.selection.anchor <= self.selection.active {
            *self.selection = SelectionRange::new(new_lo, new_hi);
        } else {
            *self.selection = SelectionRange::new(new_hi, new_lo);
        }
        self.cursor.set_offset(self.buffer, self.selection.active);
    }

    fn unindent_line(&mut self) {
        let line_text = self.buffer.line_at(self.cursor.line).unwrap_or("");
        let line_start = self.buffer.line_start_offset(self.cursor.line);
        if line_text.starts_with("  ") {
            self.buffer.delete(line_start, line_start + 2);
            self.cursor
                .set_offset(self.buffer, self.cursor.offset.saturating_sub(2));
            self.selection.collapse_to_active();
        } else if line_text.starts_with(' ') || line_text.starts_with('\t') {
            self.buffer.delete(line_start, line_start + 1);
            self.cursor
                .set_offset(self.buffer, self.cursor.offset.saturating_sub(1));
            self.selection.collapse_to_active();
        }
    }
}

/// Byte ranges of every token spelling the same word as the confirmed-hover
/// token, inside the painted window `[window_start, window_end)`. The anchor
/// token itself is excluded — the pointer already marks it. Matching is
/// Unicode-lowercase-insensitive so `USERS` and `users` agree.
fn hover_occurrence_ranges(
    text: &str,
    anchor: (usize, usize),
    tokens: &[SyntaxToken],
    window_start: usize,
    window_end: usize,
) -> Vec<(usize, usize)> {
    let Some(target) = text.get(anchor.0..anchor.1) else {
        return Vec::new();
    };
    if target.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    for token in tokens {
        if token.range.1 <= window_start {
            continue;
        }
        if token.range.0 >= window_end {
            break;
        }
        if token.range == anchor {
            continue;
        }
        let Some(slice) = text.get(token.range.0..token.range.1) else {
            continue;
        };
        if str_eq_ignore_case(slice, target) {
            ranges.push(token.range);
        }
    }
    ranges
}

/// Allocation-free Unicode case-insensitive equality.
fn str_eq_ignore_case(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

fn layout_line(
    ui: &Ui,
    buffer: &TextBuffer,
    tokens: &[SyntaxToken],
    highlighter: &SqlHighlighter,
    theme: &DbProTheme,
    line_idx: usize,
    font_id: &FontId,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    let line_text = buffer.line_at(line_idx).unwrap_or("");
    let line_start_off = buffer.line_start_offset(line_idx);
    let line_end_off = buffer.line_end_offset(line_idx);
    let mut appended = false;
    if !line_text.is_empty() {
        let start_token_idx = tokens.partition_point(|token| token.range.1 <= line_start_off);
        for token in &tokens[start_token_idx..] {
            if token.range.0 >= line_end_off {
                break;
            }
            let seg_start = token.range.0.max(line_start_off);
            let seg_end = token.range.1.min(line_end_off);
            if seg_start < seg_end {
                job.append(
                    &buffer.text()[seg_start..seg_end],
                    0.0,
                    TextFormat {
                        font_id: font_id.clone(),
                        color: highlighter.token_color(token.kind, theme),
                        ..Default::default()
                    },
                );
                appended = true;
            }
        }
    }
    if !appended {
        job.append(
            line_text,
            0.0,
            TextFormat {
                font_id: font_id.clone(),
                color: theme.text_primary,
                ..Default::default()
            },
        );
    }
    ui.painter().layout_job(job)
}

fn glyph_x(galley: &egui::Galley, char_index: usize) -> f32 {
    galley
        .pos_from_cursor(egui::text::CCursor {
            index: egui::text::CharIndex(char_index),
            prefer_next_row: false,
        })
        .min
        .x
}

fn char_index_at_x(galley: &egui::Galley, local_x: f32) -> usize {
    let y = galley
        .rows
        .first()
        .map(|row| (row.min_y() + row.max_y()) * 0.5)
        .unwrap_or(0.0);
    galley.cursor_from_pos(egui::vec2(local_x.max(0.0), y)).index.0
}

fn is_single_identifier_char(text: &str) -> bool {
    text.len() == 1 && (text.as_bytes()[0].is_ascii_alphanumeric() || text == "_")
}

// cc-scan:allow COMPLEXITY — classifier/dispatch ladder — one case per branch
fn identifier_at(buffer: &TextBuffer, offset: usize) -> Option<String> {
    let text = buffer.text();
    if text.is_empty() {
        return None;
    }
    let mut index = buffer.floor_char_boundary(offset.min(text.len()));
    if index >= text.len() || !is_ident_char_at(text, index) {
        if index == 0 {
            return None;
        }
        index = buffer.prev_char_boundary(index);
    }
    if !is_ident_char_at(text, index) {
        return None;
    }
    let mut start = index;
    while start > 0 {
        let prev = buffer.prev_char_boundary(start);
        if !is_ident_char_at(text, prev) {
            break;
        }
        start = prev;
    }
    let mut end = index;
    while end < text.len() && is_ident_char_at(text, end) {
        end = buffer.next_char_boundary(end);
    }
    let word = &text[start..end];
    if word.is_empty() {
        None
    } else {
        Some(word.to_owned())
    }
}

fn is_ident_char_at(text: &str, offset: usize) -> bool {
    text[offset..]
        .chars()
        .next()
        .is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$')
}

fn paint_editor_scrollbars(
    ui: &Ui,
    viewport: Rect,
    scroll: Vec2,
    max_scroll: Vec2,
    diagnostics: &[Diagnostic],
    buffer: &TextBuffer,
    theme: &DbProTheme,
) {
    const THICK: f32 = 4.0;
    const PAD: f32 = 2.0;
    let painter = ui.painter();
    let track_h = (viewport.height() - PAD * 2.0).max(12.0);
    let track_x = viewport.max.x - PAD - THICK;

    // Overview ruler markers for diagnostics (JetBrains / DataGrip error stripes)
    let line_count = buffer.line_count().max(1);
    for diag in diagnostics {
        let (diag_line, _) = buffer.offset_to_line_col(diag.range.0);
        let frac = (diag_line as f32 / line_count as f32).clamp(0.0, 1.0);
        let mark_y = viewport.min.y + PAD + frac * (track_h - 3.0);
        let mark_rect = Rect::from_min_size(Pos2::new(track_x - 1.0, mark_y), Vec2::new(THICK + 2.0, 3.0));
        let mark_color = match diag.severity {
            DiagnosticSeverity::Error => theme.danger,
            DiagnosticSeverity::Warning => theme.warning,
            _ => theme.accent,
        };
        painter.rect_filled(mark_rect, CornerRadius::same(1.0 as u8), mark_color);
    }

    if max_scroll.y > 1.0 {
        let thumb_h = ((viewport.height() / (viewport.height() + max_scroll.y)) * track_h).clamp(16.0, track_h);
        let t = (scroll.y / max_scroll.y).clamp(0.0, 1.0);
        let thumb_y = viewport.min.y + PAD + t * (track_h - thumb_h);
        let thumb = Rect::from_min_size(Pos2::new(track_x, thumb_y), Vec2::new(THICK, thumb_h));
        painter.rect_filled(
            thumb,
            CornerRadius::same((THICK * 0.5) as u8),
            theme.text_muted.linear_multiply(0.55),
        );
    }
    if max_scroll.x > 1.0 {
        let track_w = (viewport.width() - PAD * 2.0).max(12.0);
        let thumb_w = ((viewport.width() / (viewport.width() + max_scroll.x)) * track_w).clamp(16.0, track_w);
        let t = (scroll.x / max_scroll.x).clamp(0.0, 1.0);
        let thumb_x = viewport.min.x + PAD + t * (track_w - thumb_w);
        let thumb = Rect::from_min_size(
            Pos2::new(thumb_x, viewport.max.y - PAD - THICK),
            Vec2::new(thumb_w, THICK),
        );
        painter.rect_filled(
            thumb,
            CornerRadius::same((THICK * 0.5) as u8),
            theme.text_muted.linear_multiply(0.45),
        );
    }
}

#[cfg(test)]
#[path = "tests/renderer_tests.rs"]
mod tests;
