use crate::layout::CellView;
use crate::rendering::colors::ToColor;
use gv_viewer::prepare::coverage::{calculate_binned_coverage, round_up_max_coverage};
use std::usize;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

use ratatui::symbols::bar::{NINE_LEVELS, Set};

use gv_core::{alignment::CoverageTable, prelude::*};

use crate::{layout::AlignmentView, rendering::Palette};
const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;

/// Render the coverage barplot.
pub fn render_coverage(
    area: &Rect,
    buf: &mut Buffer,
    coverage: &CoverageTable,
    alignment_view: &AlignmentView,
    palette: &Palette,
) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let plot_area = if area.height > MIN_AREA_HEIGHT {
        Rect::new(area.x, area.y + 1, area.width, area.height - 1)
    } else {
        *area
    };

    let Some((left, right)) = displayed_coverage_bounds(alignment_view, &plot_area) else {
        return Ok(());
    };

    let mut binned_coverage =
        calculate_binned_coverage(coverage, left, right, plot_area.width as usize)?;

    let y_max: usize = round_up_max_coverage(
        (0..binned_coverage[0].len())
            .map(|i| binned_coverage[0][i] + binned_coverage[1][i])
            .max()
            .unwrap_or(0),
    );
    StackedSparkline::default()
        .add_data(binned_coverage.remove(0), palette.COVERAGE_ALT.color())
        .add_data(binned_coverage.remove(0), palette.COVERAGE_TOTAL.color())
        .max(y_max)
        .render(plot_area, buf);

    if area.height > MIN_AREA_HEIGHT {
        buf.set_string(area.x, area.y, format!("[0-{}]", y_max,), Style::default());
    }

    Ok(())
}

fn displayed_coverage_bounds(alignment_view: &AlignmentView, area: &Rect) -> Option<(u64, u64)> {
    let (left, _) = alignment_view.coordinates_of_onscreen_x(area.left(), area)?;
    let (_, right) =
        alignment_view.coordinates_of_onscreen_x(area.right().checked_sub(1)?, area)?;

    Some((left, right))
}

/// Stacked sparkline with multiple colors.
/// TODO: move this to a separate crate.
struct StackedSparkline {
    max: Option<usize>,

    data: Vec<(Vec<usize>, Color)>, // bottom, top

    bar_set: Set,
}

impl Default for StackedSparkline {
    fn default() -> Self {
        Self {
            max: Some(0),
            data: Vec::new(),
            bar_set: NINE_LEVELS,
        }
    }
}

impl StackedSparkline {
    pub fn add_data(mut self, data: Vec<usize>, color: Color) -> Self {
        self.data.push((data, color));
        self
    }

    pub fn max(mut self, max: usize) -> Self {
        self.max = Some(max);
        self
    }

    const fn symbol_for_height(&self, height: usize) -> &str {
        match height {
            0 => self.bar_set.empty,
            1 => self.bar_set.one_eighth,
            2 => self.bar_set.one_quarter,
            3 => self.bar_set.three_eighths,
            4 => self.bar_set.half,
            5 => self.bar_set.five_eighths,
            6 => self.bar_set.three_quarters,
            7 => self.bar_set.seven_eighths,
            _ => self.bar_set.full,
        }
    }

    fn get_color(&self, i_stack: usize) -> &Color {
        &self.data[i_stack].1
    }

    fn get_data(&self, i_data: usize, i_stack: usize) -> usize {
        *self.data[i_stack].0.get(i_data).unwrap_or(&0)
    }
}

impl Widget for StackedSparkline {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Inspired by Ratatui's sparkline implementation

        if area.is_empty() {
            return;
        }

        let max_index = usize::min(
            area.width as usize,
            self.data
                .iter()
                .map(|stack_data| stack_data.0.len())
                .max()
                .unwrap_or(0),
        );
        let max = self.max.unwrap_or(
            self.data
                .iter()
                .map(|stack_data| *stack_data.0.iter().max().unwrap_or(&0))
                .max()
                .unwrap_or(0),
        );

        if max == 0 {
            return;
        }

        // Ratatui's sparkline converts the height to # of 1/8 cells.
        // But this doesn't work for the stacked plot because it causes numerical errors.
        let cell_height = usize::max(1, max / area.height as usize);

        // render each item in the data
        for i in 0..max_index {
            let x = area.left() + i as u16;

            let mut pivot = 0;
            let mut accumulator = self.get_data(i, pivot); // accumate un-plotted heights

            for j in (0..area.height).rev() {
                // render from screen bottom to top (loop is top to bottom)
                if accumulator >= cell_height {
                    // render a whole cell
                    buf[(x, area.top() + j)]
                        .set_symbol(self.bar_set.full)
                        .set_style(Style::default().fg(*self.get_color(pivot)));

                    accumulator -= cell_height
                } else {
                    // Multiple color in the same cell
                    // Each cell fits max two colors.
                    // Accumate next stacks until the cell is filled. Only render the top two colors.

                    let mut rendered = false;

                    let mut top_two_indexes: (usize, usize) = (pivot, 0); // largest, second largest
                    let mut top_two_accumulators: (usize, usize) = (accumulator, 0);

                    for k in pivot + 1..self.data.len() {
                        let item = self.get_data(i, k);
                        accumulator += item;

                        if item > top_two_indexes.0 {
                            top_two_indexes = (k, top_two_indexes.0);
                            top_two_accumulators = (accumulator, top_two_accumulators.0);
                        } else if item > top_two_indexes.1 {
                            top_two_indexes = (top_two_indexes.0, k);
                            top_two_accumulators = (top_two_accumulators.0, accumulator);
                        };

                        if accumulator >= cell_height {
                            // render

                            // Note to maintain the order of these two colors
                            let (fg_height, fg_color, bg_color) =
                                if top_two_indexes.0 > top_two_indexes.1 {
                                    // 1 is the bottom stack
                                    // 1's accumulator is smaller than 0, so the foreground height is 1's accumulator
                                    (
                                        top_two_accumulators.1 * 8 / cell_height,
                                        self.get_color(top_two_indexes.1),
                                        self.get_color(top_two_indexes.0),
                                    )
                                } else {
                                    // 0 is the bottom stack
                                    // 0's accumulator is larger than 1, so the foreground height is the difference
                                    (
                                        (top_two_accumulators.1 - top_two_accumulators.0) * 8
                                            / cell_height,
                                        self.get_color(top_two_indexes.0),
                                        self.get_color(top_two_indexes.1),
                                    )
                                };

                            buf[(x, area.top() + j)]
                                .set_symbol(self.symbol_for_height(fg_height))
                                .set_style(Style::default().fg(*fg_color).bg(*bg_color));

                            accumulator -= cell_height;
                            pivot = k;
                            rendered = true;
                            break;
                        }
                    }

                    if !rendered {
                        // Reached the end of data and the whole cell is not filled.
                        // Only render the top accumulator
                        buf[(x, area.top() + j)]
                            .set_symbol(
                                self.symbol_for_height(top_two_accumulators.0 * 8 / cell_height),
                            )
                            .set_style(Style::default().fg(*self.get_color(top_two_indexes.0)));
                        break;
                    }
                }
            }
        }
    }
}
