use crate::models::window::{OnScreenCoordinate, ViewingWindow};
use itertools::izip;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

pub fn render_coordinates(
    area: &Rect,
    buf: &mut Buffer,
    viewing_window: &ViewingWindow,
    contig_length: Option<usize>,
) -> Result<(), ()> {
    let (coordinate_texts, coordinate_texts_xs, markers_onscreen_x) =
        calculate_coordinates(viewing_window, area, contig_length);

    for (text, text_x, marker_x) in izip!(
        coordinate_texts.iter(),
        coordinate_texts_xs.iter(),
        markers_onscreen_x.iter()
    ) {
        buf.set_stringn(
            area.x + *text_x,
            area.y,
            text,
            area.width as usize - *text_x as usize,
            Style::default(),
        );
        buf.set_stringn(
            area.x + *marker_x,
            area.y + 1,
            "|",
            area.width as usize - *marker_x as usize,
            Style::default(),
        );
    }

    Ok(())
}

const MIN_SPACING_BETWEEN_MARKERS: u16 = 15;

/// Calculate coordinate markers.
/// left and right are 1-based, inclusive.
fn calculate_coordinates(
    viewing_window: &ViewingWindow,
    area: &Rect,
    contig_length: Option<usize>,
) -> (Vec<String>, Vec<u16>, Vec<u16>) {
    let (intermarker_distance, power) = calculate_intermarker_distance(viewing_window.zoom());

    let mut pivot = (viewing_window.left() / intermarker_distance + 1) * intermarker_distance; // First marker
    let mut markers_onscreen_x: Vec<u16> = Vec::new();
    let mut coordinate_texts: Vec<String> = Vec::new();
    let mut coordinate_texts_xs: Vec<u16> = Vec::new();

    let render_bound = match contig_length {
        Some(length) => usize::min(viewing_window.right(area), length),
        None => viewing_window.right(area),
    };

    while pivot < render_bound {
        let marker_text = get_abbreviated_coordinate_text(pivot, power);

        let onscreen_marker_coordinate = viewing_window.onscreen_x_coordinate(pivot, area);

        match onscreen_marker_coordinate {
            OnScreenCoordinate::OnScreen(x) => {
                if (x >= marker_text.len() / 2) {
                    markers_onscreen_x.push(x as u16);
                    coordinate_texts_xs.push((x - marker_text.len() / 2) as u16);
                    coordinate_texts.push(marker_text);
                } else {
                    markers_onscreen_x.push(x as u16);
                    coordinate_texts_xs.push(0 as u16);
                    coordinate_texts.push(marker_text[(marker_text.len() / 2 - x)..].to_string());
                }
            }
            _ => {
                continue;
            }
        }

        pivot += intermarker_distance;
    }

    (coordinate_texts, coordinate_texts_xs, markers_onscreen_x)
}

fn calculate_intermarker_distance(zoom: usize) -> (usize, usize) {
    let mut distance = zoom * MIN_SPACING_BETWEEN_MARKERS as usize;

    // Find the smallest number among [1,2,10,20,50,100, ....] that is greater than or equal to min_distance
    let mut power: usize = 1;
    while distance > 10 {
        distance /= 10;
        power += 1;
    }

    if distance < 2 {
        return (2 * 10usize.pow(power as u32 - 1), power - 1);
    } else {
        return (10usize.pow(power as u32), power);
    }
}

fn get_abbreviated_coordinate_text(coordinate: usize, power: usize) -> String {
    if power < 3 {
        return format!("{}bp", to_thousand_separated(coordinate));
    } else if (power < 6) {
        return format!("{}kb", to_thousand_separated(coordinate / 1_000));
    } else if power < 9 {
        return format!("{}Mb", to_thousand_separated(coordinate / 1_000_000));
    } else if power < 12 {
        return format!("{}Gb", to_thousand_separated(coordinate / 1_000_000_000));
    } else {
        return format!(
            "{}Tb",
            to_thousand_separated(coordinate / 1_000_000_000_000)
        );
    }
}

fn to_thousand_separated(number: usize) -> String {
    if number < 1000 {
        return format!("{}", number);
    }
    let mut number = number;
    let mut reminder = number % 1000;
    let mut output = format!("{:03}", reminder);

    while number >= 1000 {
        number /= 1000;
        reminder = number % 1000;

        if number < 1000 {
            output = format!("{},{}", reminder, output);
        } else {
            output = format!("{:03},{}", reminder, output);
        }
    }

    output
}
