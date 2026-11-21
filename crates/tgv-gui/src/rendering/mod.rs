//! Draws the window: every area in the layout, then the context menu and the popup.
//!
//! The files mirror the terminal UI's `rendering` module, one per kind of area, with the same
//! function names. Each draws from the frame the worker sent and the view as drawn, in pixels.

mod alignment;
mod bed;
pub mod colors;
mod console;
mod contig_list;
mod coordinate;
mod coverage;
pub(crate) mod cytoband;
mod intervals;
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

use crate::{
    layout::{PixelView, Rect, ResolvedMainLayout},
    menu::ContextMenu,
    mouse::MouseRegister,
    worker::Frame,
};
use colors::{Palette, Rgb, ToHsla};
use gpui::{
    App, Bounds, ContentMask, Font, SharedString, TextAlign, TextRun, Window, point, px, size,
};
use gv_core::{message::AlignmentFilter::False, prelude::*};
use gv_viewer::{popup::TextPopup, prelude::*};

pub const FONT_SIZE: f32 = 12.0;
pub const LINE_HEIGHT: f32 = 16.0;
pub const CHAR_WIDTH: f32 = 7.3;

/// Paints rectangles and text, in window pixels.
pub struct Painter<'a> {
    pub window: &'a mut Window,
    pub cx: &'a mut App,
    font: Font,
}

fn default_font() -> Font{
    gpui::font(if cfg!(target_os = "linux") {
                    "Liberation Mono"
                } else {
                    "Menlo"
                })
}


    pub fn fill(rect: Rect, color: impl Into<gpui::Hsla>, window: &mut Window) {
        if rect.is_empty() {
            return;
        }
        window
            .paint_quad(gpui::fill(bounds(rect), color.into()));
    }

    /// Fills a polygon, such as a read's strand arrow.
    pub fn polygon( points: &[(f32, f32)], color: impl Into<gpui::Hsla>, window: &Window) {
        let mut builder = gpui::PathBuilder::fill();
        builder.add_polygon(
            &points
                .iter()
                .map(|&(x, y)| point(px(x), px(y)))
                .collect::<Vec<_>>(),
            true,
        );
        match builder.build() {
            Ok(path) => self.window.paint_path(path, color.into()),
            Err(error) => log::warn!("Failed to draw a shape: {error}"),
        }
    }

    /// Draws one line of text with its top-left corner at `(x, y)`, and returns its width.
    pub fn text(&mut self, x: f32, y: f32, text: &str, color: Rgb) -> f32 {
        self.text_with(x, y, text, color.hsla(), false, FONT_SIZE, LINE_HEIGHT)
    }

    /// Draws one line of bold text, and returns its width.
    pub fn bold_text(&mut self, x: f32, y: f32, text: &str, color: Rgb) -> f32 {
        self.text_with(x, y, text, color.hsla(), true, FONT_SIZE, LINE_HEIGHT)
    }

    /// Draws text wrapped at `width` with its top-left corner at `(x, y)`, and returns its
    /// height. Newlines start new lines.
    pub fn wrapped_text(&mut self, x: f32, y: f32, width: f32, text: &str, color: Rgb) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let text: SharedString = text.to_string().into();
        let run = TextRun {
            len: text.len(),
            font: self.font.clone(),
            color: color.hsla(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let lines = match self.window.text_system().shape_text(
            text,
            px(FONT_SIZE),
            &[run],
            Some(px(width.max(CHAR_WIDTH))),
            None,
        ) {
            Ok(lines) => lines,
            Err(error) => {
                log::warn!("Failed to shape text: {error}");
                return 0.0;
            }
        };
        let mut height = 0.0;
        for line in lines {
            if let Err(error) = line.paint(
                point(px(x), px(y + height)),
                px(LINE_HEIGHT),
                TextAlign::Left,
                None,
                self.window,
                self.cx,
            ) {
                log::warn!("Failed to draw text: {error}");
            }
            height += f32::from(line.size(px(LINE_HEIGHT)).height);
        }
        height
    }

    /// Draws a small letter centered in a box, such as a read base.
    pub fn letter(&mut self, rect: Rect, letter: char, color: Rgb) {
        let size = (rect.height - 1.0).clamp(6.0, FONT_SIZE);
        let text = letter.to_string();
        let width = f32::from(self.shape(&text, color.hsla(), false, size).width);
        self.text_with(
            rect.x + (rect.width - width) / 2.0,
            rect.y,
            &text,
            color.hsla(),
            false,
            size,
            rect.height,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn text_with(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        color: gpui::Hsla,
        bold: bool,
        size: f32,
        line_height: f32,
    ) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let line = self.shape(text, color, bold, size);
        let width = f32::from(line.width);
        if let Err(error) = line.paint(point(px(x), px(y)), px(line_height), self.window, self.cx) {
            log::warn!("Failed to draw text: {error}");
        }
        width
    }

    /// The width that a line of text takes.


    fn paint_text(
        &mut self,
        x: f32,
        y: f32,
        line_height: f32,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<f32, TGVError> {
        let font = if bold {
            self.font.clone().bold()
        } else {
            self.font.clone()
        };
        let text: SharedString = text.replace('\n', " ").into();
        let run = TextRun {
            len: text.len(),
            font,
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = self
            .window
            .text_system()
            .shape_line(text, px(size), &[run], None);
        let width = f32::from(line.width);
        line.paint(point(px(x), px(y)), px(line_height), window, cx)?
    }

    /// Paints within `rect` only.
    pub fn clipped(&mut self, rect: Rect, paint: impl FnOnce(&mut Painter)) {
        let font = self.font.clone();
        let cx = &mut *self.cx;
        self.window.with_content_mask(
            Some(ContentMask {
                bounds: bounds(rect),
            }),
            |window| paint(&mut Painter { window, cx, font }),
        )
    }

#[derive(Debug)]
struct TextContext{

    text: &str,
    color: gpui::Hsla,
    bold: bool,
    size: f32,
    font: Font,

}

impl TextContext{
    pub fn width(&self, &mut Window) -> f32 {
        let font = if self.bold {
                    self.font.clone().bold()
                } else {
                    self.font.clone()
                };
                let text: SharedString = self.text.replace('\n', " ").into();
        let run = TextRun {
                    len: self.text.len(),
                    font,
                    self.color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };

                          let shape = window
                          .text_system()
                          .shape_line(self.text, px(self.size), &[run], None);
                return f32::from(shape.width);

      }
}

impl Default for TextContext {
    fn default() -> Self {
        Self{
            text: "",
            color: gpui::Hsla::black(),
            bold: False,
            size: FONT_SIZE,
            font:
        }
    }
}

fn bounds(rect: Rect) -> Bounds<gpui::Pixels> {
    Bounds {
        origin: point(px(rect.x), px(rect.y)),
        size: size(px(rect.width), px(rect.height)),
    }
}

/// What the screen shows about the focus.
pub struct TrackFocus {
    pub target: gv_viewer::message::FocusTarget,
    /// The focused track's name, such as `peaks.bed` or `untitled-1`, or the reference's name.
    pub name: String,
    pub modified: bool,
}

/// What a message line shows instead of the messages: the data under the pointer, or that the
/// window waits for the viewer.
pub struct Status<'a> {
    pub hover: Option<&'a str>,
    pub waiting: bool,
}

/// The gap that dragged files go into: where to draw it, and how opaque, from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropGap {
    pub y: f32,
    pub alpha: f32,
}

/// Render all areas in the layout
#[allow(clippy::too_many_arguments)]
pub fn render_main(
    painter: &mut Painter,
    frame: &Frame,
    layout: &ResolvedMainLayout,
    view: &PixelView,
    mouse_register: &MouseRegister,
    focus: &TrackFocus,
    context_menu: Option<&ContextMenu>,
    popup: Option<&TextPopup>,
    status: &Status,
    drop_gap: Option<DropGap>,
    palette: &Palette,
) -> Result<(), TGVError> {
    painter.fill(layout.window, palette.background.hsla());
    sidebar::render_sidebar(painter, frame, layout, view, mouse_register, focus, palette)?;

    for (area_type, rect) in layout.areas.iter() {
        if rect.is_empty() {
            continue;
        }
        let rect = *rect;
        let mut result = Ok(());
        painter.clipped(rect, |painter| {
            result = render_area(
                painter,
                frame,
                layout,
                view,
                mouse_register,
                focus,
                status,
                palette,
                area_type,
                rect,
            );
        });
        result?;
    }
    if let Some(gap) = drop_gap {
        render_drop_gap(painter, layout, gap, palette);
    }

    // The menu and the popup go over everything else.
    if let Some(menu) = context_menu {
        menu::render_context_menu(painter, menu, palette);
    }
    if let Some(popup) = popup {
        menu::render_text_popup(painter, layout.window, popup, palette);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_area(
    painter: &mut Painter,
    frame: &Frame,
    layout: &ResolvedMainLayout,
    view: &PixelView,
    mouse_register: &MouseRegister,
    focus: &TrackFocus,
    status: &Status,
    palette: &Palette,
    area_type: &AreaType,
    rect: Rect,
) -> Result<(), TGVError> {
    let state = &frame.state;
    let zoom = view.viewport.zoom;
    match area_type {
        AreaType::Cytoband => render_cytobands(painter, &rect, state, view, palette)?,
        AreaType::Coordinate => render_coordinates(painter, &rect, view, state)?,
        AreaType::Coverage(id) => {
            let index = layout.track_registry.alignment_index(*id)?;
            if zoom.shows_alignments() {
                render_coverage(
                    painter,
                    &rect,
                    &state.alignments[index].coverage,
                    view,
                    palette,
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
                            .ok_or(TGVError::StateError(format!(
                                "Paired alignment {index} not yet calculated at rendering"
                            )))
                    })
                    .transpose()?;
                render_alignment(
                    painter,
                    &rect,
                    &state.alignments[index],
                    paired,
                    view,
                    view.viewport.top(index),
                    palette,
                )?;
            } else {
                intervals::render_zoom_in_notice(painter, &rect, "alignments");
            }
        }
        AreaType::AlignmentDivider { .. } => {
            let color = if mouse_register.is_divider_highlighted(area_type) {
                palette.HIGHLIGHT_COLOR
            } else {
                colors::SEPARATOR
            };
            painter.fill(
                Rect::new(rect.x, rect.y + rect.height / 2.0 - 1.0, rect.width, 2.0),
                color.hsla(),
            );
        }
        AreaType::Sequence => {
            if zoom.shows_sequence() {
                render_sequence(painter, &rect, state, view, palette)?;
            }
        }
        AreaType::GeneTrack => render_track(painter, &rect, state, view, palette)?,
        AreaType::Console => {
            if matches!(
                frame.registers.current,
                KeyRegisterType::Command(_) | KeyRegisterType::Search(_)
            ) {
                render_console(painter, &rect, &frame.registers);
            } else {
                render_mode_line(
                    painter,
                    &rect,
                    &frame.registers,
                    focus,
                    layout.sidebar_width == 0.0,
                    palette,
                );
            }
        }
        AreaType::Error => render_status_bar(painter, &rect, state, status),
        AreaType::Variant(id) => {
            let index = layout.track_registry.variant_index(*id)?;
            render_variants(painter, &rect, &state.variants[index], view, palette)?;
        }
        AreaType::Bed(id) => {
            let index = layout.track_registry.bed_index(*id)?;
            render_bed(painter, &rect, &state.bed_intervals[index], view, palette)?;
            // Insert mode draws the cursor as an outlined base.
            if frame.registers.shown_mode() == gv_viewer::keys::Mode::Insert(*id)
                && let Some((x, width)) = view.span(
                    view.viewport.focus.position,
                    view.viewport.focus.position,
                    &rect,
                )
            {
                painter.fill(
                    Rect::new(x, rect.y, width, rect.height),
                    palette.MODE_INSERT.alpha(0.55),
                );
            }
        }
        AreaType::Fill => {}
    }
    Ok(())
}

/// Marks the gap that dragged files go into: a line across the window, with a soft glow and a
/// note.
fn render_drop_gap(
    painter: &mut Painter,
    layout: &ResolvedMainLayout,
    gap: DropGap,
    palette: &Palette,
) {
    let window = layout.window;
    let color = palette.DROP_TARGET;
    painter.fill(
        Rect::new(window.x, gap.y - 6.0, window.width, 12.0),
        color.alpha(0.18 * gap.alpha),
    );
    painter.fill(
        Rect::new(window.x, gap.y - 1.5, window.width, 3.0),
        color.alpha(gap.alpha),
    );
    let note = "Drop to add tracks here";
    let width = painter.text_width(note) + 2.0 * CHAR_WIDTH;
    let note_area = Rect::new(
        layout.main_area.x + 12.0,
        gap.y - LINE_HEIGHT / 2.0 - 2.0,
        width,
        LINE_HEIGHT + 4.0,
    );
    painter.fill(note_area, color.alpha(gap.alpha));
    painter.text(
        note_area.x + CHAR_WIDTH,
        note_area.y + 2.0,
        note,
        palette.SEQUENCE_FOREGROUND_COLOR,
    );
}
