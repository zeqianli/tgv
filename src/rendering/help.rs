use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Text},
    widgets::{Paragraph, Widget},
};

const MIN_AREA_WIDTH: u16 = 2;
const MIN_AREA_HEIGHT: u16 = 1;

pub fn render_help(area: Rect, buf: &mut Buffer) {
    if area.width < MIN_AREA_WIDTH || area.height < MIN_AREA_HEIGHT {
        return;
    }

    let help_text = format!(
        "
 Terminal Genome Viewer - version {}
 ------------------------------------------------------------------------------
 
 |:q|    Quit           |<ESC>|     Switch to normal mode / Close this window
 |:h|    Help           |:|         Switch to command mode
 
 |h / j / k / l|   Move left / down / up / right
 |y / p|           Move left / right faster
 |w / b|           Beginning of the next / last exon
 |W / B|           Begining of the next / last gene
 |e / ge|          End of the next / last exon
 |E / gE|          End of the next / last gene
 |z / o|           Zoom in / out
 
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

    paragraph.render(area, buf);
}
