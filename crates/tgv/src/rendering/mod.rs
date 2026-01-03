mod alignment;
mod bed;
mod colors;
mod console;
mod contig_list;
mod coordinate;
mod coverage;
mod cytoband;
mod help;
mod intervals;
mod sequence;
mod status_bar;
mod track;
mod variants;
pub use alignment::render_alignment;
pub use bed::render_bed;
pub use colors::{DARK_THEME, Palette};
pub use console::render_console;
pub use contig_list::render_contig_list;
pub use coordinate::render_coordinates;
pub use coverage::render_coverage;
pub use cytoband::render_cytobands;
pub use help::render_help;
pub use sequence::render_sequence;
pub use status_bar::render_status_bar;
pub use track::render_track;
pub use variants::render_variants;

use crate::{
    layout::{AlignmentView, AreaType, MainLayout},
    register::{KeyRegisterType, Registers},
};

use gv_core::{error::TGVError, repository::Repository, state::State};
use ratatui::{buffer::Buffer, layout::Rect};

/// Render all areas in the layout
pub fn render_main(
    buf: &mut Buffer,
    state: &State,
    registers: &Registers,
    layout: &MainLayout,
    alignment_view: &AlignmentView,
    pallete: &Palette,
) -> Result<(), TGVError> {
    // Render each area based on its type
    for (i, (area_type, rect)) in layout.areas.iter().enumerate() {
        if rect.y >= buf.area.height || rect.x >= buf.area.width {
            continue;
        }

        match area_type {
            AreaType::Cytoband => render_cytobands(rect, buf, state, alignment_view, pallete)?,
            AreaType::Coordinate => render_coordinates(rect, buf, alignment_view, state)?,
            AreaType::Coverage => {
                render_coverage(rect, buf, state, alignment_view, pallete)?;
            }
            AreaType::Alignment => {
                render_alignment(rect, buf, state, alignment_view, pallete)?;
            }
            AreaType::Sequence => {
                render_sequence(rect, buf, state, alignment_view, pallete)?;
            }
            AreaType::GeneTrack => {
                render_track(rect, buf, state, alignment_view, pallete)?;
            }
            AreaType::Console => {
                if registers.current == KeyRegisterType::Command {
                    render_console(rect, buf, &registers)?;
                }
            }
            AreaType::Error => {
                render_status_bar(rect, buf, state, alignment_view)?;
            }
            AreaType::Variant => render_variants(rect, buf, state, alignment_view, pallete)?,
            AreaType::Bed => render_bed(rect, buf, state, alignment_view, pallete)?,
        };
    }
    Ok(())
}

pub fn get_abbreviated_length_string(length: u64) -> String {
    let mut length = length;
    let mut power = 0;

    while length >= 1000 {
        length /= 1000;
        power += 1;
    }

    format!(
        "{}{}",
        length,
        match power {
            0 => "bp",
            1 => "kb",
            2 => "Mb",
            3 => "Gb",
            4 => "Tb",
            _ => "",
        }
    )
}
