use crate::error::TGVError;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Text},
    widgets::{Paragraph, Widget},
};

const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;

pub fn render_help(area: &Rect, buf: &mut Buffer) -> Result<(), TGVError> {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return Ok(());
    }

    let help_text = format!(
        "
 Terminal Genome Viewer - version {}
 ------------------------------------------------------------------------------

 See more at: https://github.com/zeqianli/tgv
 
 |:q|    Quit           |<ESC>|     Switch to normal mode / Close this window
 |:h|    Help           |:|         Switch to command mode
 |:ls| or |:contigs|                Switch chromosomes
 
 |h / j / k / l|   Move left / down / up / right
 |y / p|           Move left / right faster
 |w / b|           Beginning of the next / last exon
 |W / B|           Begining of the next / last gene
 |e / ge|          End of the next / last exon
 |E / gE|          End of the next / last gene
 |z / o|           Zoom in / out
 |{{ / }}|        Move to the next / previous contig
 
 |<num><key>|      Repeat movements. Examples:
     - 5h: Move right by 5 bases
     - 11B: Move left by 11 genes
     - 16o: Zoom out by 16x
 
 |:_pos_|          Go to position on same contig.       Example: :1000
 |:_contig_:_pos_| Go to position on a contig.          Example: 17:7572659
 |:_gene_|         Go to _gene_                         Example: :KRAS
 ",
        env!("CARGO_PKG_VERSION")
    );

    let paragraph = Paragraph::new(Text::from(
        help_text.lines().map(Line::from).collect::<Vec<Line>>(),
    ));

    paragraph.render(*area, buf);
    Ok(())
}
