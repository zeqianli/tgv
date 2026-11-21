mod alignment;
mod bed;
pub mod colors;
mod console;
mod contig_list;
mod coordinate;
mod coverage;
pub(crate) mod cytoband;
mod menu;
mod sequence;
mod sidebar;
mod status_bar;
mod track;
mod variants;

pub use alignment::render_alignment;
pub use bed::render_bed;
pub use console::{render_console, render_mode_line};
pub use contig_list::render_contig_list;
pub use coordinate::render_coordinates;
pub use coverage::render_coverage;
pub use cytoband::render_cytobands;
pub use sequence::render_sequence;
pub use status_bar::render_status_bar;
pub use track::render_track;
pub use variants::render_variants;

pub(crate) mod prelude {
    pub(crate) use super::{
        CHAR_WIDTH, LINE_HEIGHT, TextContext,
        colors::{MUTED_TEXT, Palette, Rgb, TEXT, to_hsla, to_hsla_with_alpha},
        fill_polygon, fill_rect, paint_letter, render_zoom_in_notice,
    };
    pub(crate) use crate::layout::{PixelView, Rect};
    pub(crate) use gpui::{App, Window};
}

use crate::{
    layout::{PixelView, Rect, ResolvedMainLayout},
    menu::ContextMenu,
    mouse::MouseState,
    worker::Frame,
};
use colors::{MUTED_TEXT, Palette, Rgb, to_hsla, to_hsla_with_alpha};
use gpui::{
    App, Bounds, ContentMask, Font, SharedString, TextAlign, TextRun, Window, point, px, size,
};
use gv_viewer::{popup::TextPopup, prelude::*};

pub const FONT_SIZE: f32 = 12.0;
pub const LINE_HEIGHT: f32 = 16.0;
pub const CHAR_WIDTH: f32 = 7.3;

fn default_font() -> Font {
    gpui::font(if cfg!(target_os = "linux") {
        "Liberation Mono"
    } else {
        "Menlo"
    })
}

pub fn fill_rect(rect: Rect, color: impl Into<gpui::Hsla>, window: &mut Window) {
    if rect.is_empty() {
        return;
    }
    window.paint_quad(gpui::fill(bounds(rect), color.into()));
}

pub fn fill_polygon(points: &[(f32, f32)], color: impl Into<gpui::Hsla>, window: &mut Window) {
    let mut builder = gpui::PathBuilder::fill();
    builder.add_polygon(
        &points
            .iter()
            .map(|&(x, y)| point(px(x), px(y)))
            .collect::<Vec<_>>(),
        true,
    );
    match builder.build() {
        Ok(path) => window.paint_path(path, color.into()),
        Err(error) => log::warn!("Failed to draw a shape: {error}"),
    }
}

pub struct TextContext {
    text: SharedString,
    color: gpui::Hsla,
    bold: bool,
    size: f32,
    font: Font,
}

impl Default for TextContext {
    fn default() -> Self {
        Self {
            text: SharedString::default(),
            color: gpui::black(),
            bold: false,
            size: FONT_SIZE,
            font: default_font(),
        }
    }
}

impl TextContext {
    #[must_use]
    pub fn text(mut self, text: &str) -> Self {
        self.text = text.to_string().into();
        self
    }

    #[must_use]
    pub fn color(mut self, color: Rgb) -> Self {
        self.color = to_hsla(color);
        self
    }

    #[must_use]
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    #[must_use]
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    fn run(&self, len: usize) -> TextRun {
        TextRun {
            len,
            font: if self.bold {
                self.font.clone().bold()
            } else {
                self.font.clone()
            },
            color: self.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    fn shape(&self, window: &Window) -> gpui::ShapedLine {
        let text: SharedString = self.text.replace('\n', " ").into();
        let run = self.run(text.len());
        window
            .text_system()
            .shape_line(text, px(self.size), &[run], None)
    }

    pub fn width(&self, window: &Window) -> f32 {
        f32::from(self.shape(window).width)
    }

    pub fn paint(
        &self,
        x: f32,
        y: f32,
        line_height: f32,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<f32, crate::Error> {
        if self.text.is_empty() {
            return Ok(0.0);
        }
        let line = self.shape(window);
        line.paint(point(px(x), px(y)), px(line_height), window, cx)?;
        Ok(f32::from(line.width))
    }

    pub fn paint_wrapped(
        &self,
        x: f32,
        y: f32,
        width: f32,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<f32, crate::Error> {
        if self.text.is_empty() {
            return Ok(0.0);
        }
        let run = self.run(self.text.len());
        let lines = window.text_system().shape_text(
            self.text.clone(),
            px(self.size),
            &[run],
            Some(px(width.max(CHAR_WIDTH))),
            None,
        )?;
        let mut height = 0.0;
        for line in lines {
            line.paint(
                point(px(x), px(y + height)),
                px(LINE_HEIGHT),
                TextAlign::Left,
                None,
                window,
                cx,
            )?;
            height += f32::from(line.size(px(LINE_HEIGHT)).height);
        }
        Ok(height)
    }
}

pub fn paint_letter(
    rect: Rect,
    letter: char,
    color: Rgb,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let text = TextContext::default()
        .text(&letter.to_string())
        .color(color)
        .size((rect.height - 1.0).clamp(6.0, FONT_SIZE));
    let width = text.width(window);
    text.paint(
        rect.x + (rect.width - width) / 2.0,
        rect.y,
        rect.height,
        window,
        cx,
    )?;
    Ok(())
}

fn bounds(rect: Rect) -> Bounds<gpui::Pixels> {
    Bounds {
        origin: point(px(rect.x), px(rect.y)),
        size: size(px(rect.width), px(rect.height)),
    }
}

pub struct TrackFocus {
    pub target: gv_viewer::message::FocusTarget,
    pub name: String,
    pub modified: bool,
}

pub struct Status<'a> {
    pub hover: Option<&'a str>,
    pub waiting: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropGap {
    pub y: f32,
    pub alpha: f32,
}

#[allow(clippy::too_many_arguments)]
pub fn render_main(
    frame: &Frame,
    layout: &ResolvedMainLayout,
    view: &PixelView,
    mouse_state: &MouseState,
    focus: &TrackFocus,
    context_menu: Option<&ContextMenu>,
    popup: Option<&TextPopup>,
    status: &Status,
    drop_gap: Option<DropGap>,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    fill_rect(layout.window, to_hsla(palette.background), window);
    sidebar::render_sidebar(frame, layout, view, mouse_state, focus, palette, window, cx)?;

    let state = &frame.state;
    let zoom = view.viewport.zoom;
    for (area_type, rect) in layout.areas.iter() {
        if rect.is_empty() {
            continue;
        }
        let rect = *rect;
        let mask = ContentMask {
            bounds: bounds(rect),
        };
        window.with_content_mask(Some(mask), |window| -> Result<(), crate::Error> {
            match area_type {
                AreaType::Cytoband => render_cytobands(&rect, state, view, palette, window, cx)?,
                AreaType::Coordinate => render_coordinates(&rect, view, state, window, cx)?,
                AreaType::Coverage(id) => {
                    let index = layout.track_registry.alignment_index(*id)?;
                    if zoom.shows_alignments() {
                        render_coverage(
                            &rect,
                            &state.alignments[index].coverage,
                            view,
                            palette,
                            window,
                            cx,
                        )?;
                    }
                }
                AreaType::Alignment(id) => {
                    let index = layout.track_registry.alignment_index(*id)?;
                    if zoom.shows_alignments() {
                        let paired = state.alignment_options[index]
                            .contains(&gv_core::message::AlignmentDisplayOption::ViewAsPairs)
                            .then(|| {
                                state.paired_alignments[index]
                                    .as_ref()
                                    .ok_or(crate::Error::PairedAlignmentNotReady { index })
                            })
                            .transpose()?;
                        render_alignment(
                            &rect,
                            &state.alignments[index],
                            paired,
                            view,
                            view.viewport.top(index),
                            palette,
                            window,
                            cx,
                        )?;
                    } else {
                        render_zoom_in_notice(&rect, "alignments", window, cx)?;
                    }
                }
                AreaType::AlignmentDivider { .. } => {
                    let color = if mouse_state.is_divider_highlighted(area_type) {
                        palette.HIGHLIGHT_COLOR
                    } else {
                        colors::SEPARATOR
                    };
                    fill_rect(
                        Rect::new(rect.x, rect.y + rect.height / 2.0 - 1.0, rect.width, 2.0),
                        to_hsla(color),
                        window,
                    );
                }
                AreaType::Sequence => {
                    if zoom.shows_sequence() {
                        render_sequence(&rect, state, view, palette, window, cx)?;
                    }
                }
                AreaType::GeneTrack => render_track(&rect, state, view, palette, window, cx)?,
                AreaType::Console => {
                    if matches!(
                        frame.keyboard_state.mode,
                        InputMode::Command(_) | InputMode::Search(_)
                    ) {
                        render_console(&rect, &frame.keyboard_state, window, cx)?;
                    } else {
                        render_mode_line(
                            &rect,
                            &frame.keyboard_state,
                            focus,
                            layout.sidebar_width == 0.0,
                            palette,
                            window,
                            cx,
                        )?;
                    }
                }
                AreaType::Error => render_status_bar(&rect, state, status, window, cx)?,
                AreaType::Variant(id) => {
                    let index = layout.track_registry.variant_index(*id)?;
                    render_variants(&rect, &state.variants[index], view, palette, window, cx)?;
                }
                AreaType::Bed(id) => {
                    let index = layout.track_registry.bed_index(*id)?;
                    render_bed(
                        &rect,
                        &state.bed_intervals[index],
                        view,
                        palette,
                        window,
                        cx,
                    )?;
                    if frame.keyboard_state.shown_mode() == gv_viewer::keys::Mode::Insert(*id)
                        && let Some((x, width)) = view.span(
                            view.viewport.focus.position,
                            view.viewport.focus.position,
                            &rect,
                        )
                    {
                        fill_rect(
                            Rect::new(x, rect.y, width, rect.height),
                            to_hsla_with_alpha(palette.MODE_INSERT, 0.55),
                            window,
                        );
                    }
                }
                AreaType::Fill => {}
            }
            Ok(())
        })?;
    }
    if let Some(gap) = drop_gap {
        render_drop_gap(layout, gap, palette, window, cx)?;
    }

    if let Some(menu) = context_menu {
        menu::render_context_menu(menu, palette, window, cx)?;
    }
    if let Some(popup) = popup {
        menu::render_text_popup(layout.window, popup, palette, window, cx)?;
    }
    Ok(())
}

fn render_drop_gap(
    layout: &ResolvedMainLayout,
    gap: DropGap,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    let color = palette.DROP_TARGET;
    fill_rect(
        Rect::new(layout.window.x, gap.y - 6.0, layout.window.width, 12.0),
        to_hsla_with_alpha(color, 0.18 * gap.alpha),
        window,
    );
    fill_rect(
        Rect::new(layout.window.x, gap.y - 1.5, layout.window.width, 3.0),
        to_hsla_with_alpha(color, gap.alpha),
        window,
    );
    let note = TextContext::default()
        .text("Drop to add tracks here")
        .color(palette.SEQUENCE_FOREGROUND_COLOR);
    let width = note.width(window) + 2.0 * CHAR_WIDTH;
    let note_area = Rect::new(
        layout.main_area.x + 12.0,
        gap.y - LINE_HEIGHT / 2.0 - 2.0,
        width,
        LINE_HEIGHT + 4.0,
    );
    fill_rect(note_area, to_hsla_with_alpha(color, gap.alpha), window);
    note.paint(
        note_area.x + CHAR_WIDTH,
        note_area.y + 2.0,
        LINE_HEIGHT,
        window,
        cx,
    )?;
    Ok(())
}

/// Says that the track isn't loaded for this view, because the view is zoomed out too far.
pub fn render_zoom_in_notice(
    area: &Rect,
    features: &str,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), crate::Error> {
    TextContext::default()
        .text(&format!("Zoom in to view {features}"))
        .color(MUTED_TEXT)
        .paint(
            area.x + 4.0,
            area.y + (area.height.min(LINE_HEIGHT * 2.0) - LINE_HEIGHT) / 2.0,
            LINE_HEIGHT,
            window,
            cx,
        )?;
    Ok(())
}
