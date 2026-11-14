// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
pub mod accordion;
pub mod agent_composer;
pub mod agent_primitives;
pub mod alert;
pub mod animation;
pub mod aspect_ratio;
pub mod badge;
pub mod button;
pub mod calendar;
pub mod card;
pub mod chrome;
pub mod code;
pub mod collapsible;
pub mod command;
pub mod common;
pub mod common_utils;
pub mod database;
pub mod dev_tools;
pub mod dialog;
pub mod diff;
pub(crate) mod disclosure;
pub mod explain;
pub mod feedback;
pub mod form;
pub mod hover_card;
pub mod input;
pub mod interact;
pub mod logs;
pub mod nav;
pub mod overlay;
pub mod radio_group;
pub mod responsive_layout;
pub mod scroll_area;
pub mod select;
pub mod selection;
pub mod separator;
pub mod sql_editor;
pub mod table;
pub mod tabs;
pub mod toggle;
pub mod transaction;
pub mod tree;
pub mod workspace;

pub use accordion::{Accordion, AccordionItem, AccordionType};
pub use agent_composer::{AgentComposer, AgentComposerAction, AgentMode};
pub use agent_primitives::{
    AgentPlan, AgentSqlActionKind, AgentTaskItem, AgentTaskStatus, AgentThinking, ContextChip, ContextChipKind,
    ExecutionApproval, ExecutionApprovalAction, RiskLevel, StatusBadge, StatusBadgeVariant, ToolCall, ToolCallStatus,
};
pub use alert::{Alert, AlertDialog, AlertDialogAction, AlertVariant};
pub use aspect_ratio::AspectRatio;
pub use badge::{Badge, BadgeVariant};
pub use button::{Button, ButtonGroup, ButtonSize, ButtonVariant};
pub use calendar::{day_of_week, days_in_month, is_leap_year, Calendar, DatePicker, SimpleDate};
pub use card::{
    card_content, card_footer, card_header, selectable_frame, Card, MetricCard, MetricTrend, MetricTrendDirection,
    MetricTrendTone,
};
pub use chrome::{toolbar_button, Avatar, AvatarShape, AvatarSize, AvatarStatus, EmptyState, Skeleton, Toolbar};
pub use code::{sql_layouter, CodeBlock, InlineCode};
pub use collapsible::Collapsible;
pub use command::{CommandEmpty, CommandGroup, CommandInput, CommandItem};
pub use common::{
    activity_bar_frame, agent_message_frame, app_frame, calculate_dialog_layout, calculate_sheet_layout, chip_frame,
    clamp_popup_to_screen, editor_frame, format_bytes, format_count_with_suffix, format_duration_millis,
    format_page_range, format_percentage, grid_frame, icon_text, list_row_frame, panel_frame, shell_surface_frame,
    sidebar_frame, shown_total_hint, tab_frame, toolbar_frame, truncate_ellipsis, DialogLayout, ShellSurface,
};
pub use database::{ConnectionCard, ConnectionCardAction, ConnectionStatus, DatabaseDriver, DatabaseTypeBadge};
pub use dev_tools::{ProgressRing, TerminalBlock};
pub use dialog::{dialog_actions, Dialog, DialogActionLabels, Sheet};
pub use diff::{DiffLine, DiffLineType, DiffViewer};
pub use explain::{ExplainPlanTree, PlanNode};
pub use feedback::{kbd_badge, kbd_combo, separator_with_text, status_dot, Progress, Spinner};
pub use form::{FieldRule, FormField, FormState, Label, ValidationMode};
pub use hover_card::HoverCard;
pub use input::{Input, PasswordInput, SearchInput, Textarea};
pub use logs::{LogEntry, LogLevel, LogViewer};
pub use nav::{section_label, sidebar_item, Breadcrumb, BreadcrumbItem, PageHeader, Pagination, SectionHeader};
pub use overlay::{
    context_action_menu, ctx_menu_item, floating_surface, is_context_menu_triggered, popup_dismiss_scrim, DropdownItem,
    DropdownMenu, FloatingPopup, Popover, Toast, ToastItem, ToastManager, ToastPosition, ToastResponse, ToastVariant,
    Tooltip, TooltipPosition,
};
pub use radio_group::{RadioGroup, RadioGroupOption};
pub use responsive_layout::{container_width, grid_metrics, Container, ContainerWidth, GridMetrics, ResponsiveGrid};
pub use scroll_area::{name_scroll_bars, ScrollArea};
pub use select::{dropdown_should_open_above, Select, SelectSize, SelectVariant};
pub use selection::{Checkbox, Radio, Slider, Switch};
pub use separator::{Separator, SeparatorOrientation};
pub use sql_editor::{SqlEditorAction, SqlEditorToolbar};
pub use table::{Table, TableColumn, TableColumnAlign};
pub use tabs::{
    draw_document_tab, tab_button, DocumentTabAction, DocumentTabItem, SegmentedTabs, UnderlineTabs,
    DOCUMENT_TAB_HEIGHT,
};
pub use toggle::{Toggle, ToggleGroup, ToggleGroupItem, ToggleSize, ToggleVariant};
pub use transaction::{DestructiveOperationDialog, TransactionAction, TransactionBar};
pub use tree::{reveal_children, DatabaseTreeNode, TreeNodeKind};
pub use workspace::{
    ActivityBar, ActivityBarItemKind, ConnectionHealth, ConnectionIndicator, StatusBar, StatusBarItem,
};

#[cfg(test)]
#[path = "tests/mod_tests.rs"]
mod tests;
