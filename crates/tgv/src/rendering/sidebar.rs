use crate::{
    layout::{MainLayout, SidebarLabel},
    rendering::Palette,
};
use ratatui::{
    buffer::Buffer,
    style::{Color, Style},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn render_chrome(
    buf: &mut Buffer,
    layout: &MainLayout,
    palette: &Palette,
    sidebar_resizing: bool,
) {
    let sidebar_style = Style::default().bg(palette.background).fg(Color::Gray);
    if layout.sidebar_area.width > 0 {
        buf.set_style(layout.sidebar_area, sidebar_style);
        for label in &layout.sidebar_labels {
            render_sidebar_label(buf, label, sidebar_style);
        }
    }

    if layout.sidebar_divider_area.width > 0 {
        let divider_style = if sidebar_resizing {
            Style::default().fg(palette.HIGHLIGHT_COLOR)
        } else {
            sidebar_style
        };
        for y in layout.sidebar_divider_area.top()..layout.sidebar_divider_area.bottom() {
            buf.set_string(layout.sidebar_divider_area.x, y, "│", divider_style);
        }
    }

    if let Some(metrics) = layout.scrollbar_metrics() {
        let track_style = Style::default().fg(Color::DarkGray).bg(palette.background);
        let thumb_style = Style::default().fg(Color::Gray).bg(palette.background);
        for y in layout.scrollbar_area.top()..layout.scrollbar_area.bottom() {
            buf.set_string(layout.scrollbar_area.x, y, "│", track_style);
        }
        let thumb_top = layout.scrollbar_area.y.saturating_add(metrics.thumb_start);
        let thumb_bottom = thumb_top.saturating_add(metrics.thumb_length);
        for y in thumb_top..thumb_bottom {
            buf.set_string(layout.scrollbar_area.x, y, "█", thumb_style);
        }
    }
}

fn render_sidebar_label(buf: &mut Buffer, label: &SidebarLabel, style: Style) {
    if label.area.width == 0 || label.area.height == 0 || label.full_height == 0 {
        return;
    }

    let lines = filename_lines(&label.text, label.area.width, label.full_height);
    for visible_row in 0..label.area.height {
        let source_row = label.top_clip.saturating_add(visible_row) as usize;
        if let Some(line) = lines.get(source_row) {
            buf.set_stringn(
                label.area.x,
                label.area.y.saturating_add(visible_row),
                line,
                label.area.width as usize,
                style,
            );
        }
    }
}

fn filename_lines(text: &str, width: u16, height: u16) -> Vec<String> {
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let capacity = width as usize * height as usize;
    let text = middle_ellipsize(text, capacity);
    let wrapped = wrap_by_width(&text, width as usize);
    let mut lines = vec![String::new(); height as usize];
    let start = lines.len().saturating_sub(wrapped.len()) / 2;
    for (destination, line) in lines.iter_mut().skip(start).zip(wrapped) {
        *destination = line;
    }
    lines
}

fn middle_ellipsize(text: &str, maximum_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= maximum_width {
        return text.to_string();
    }
    if maximum_width == 0 {
        return String::new();
    }
    if maximum_width == 1 {
        return "…".to_string();
    }

    let content_width = maximum_width - 1;
    let prefix_width = content_width.div_ceil(2);
    let suffix_width = content_width / 2;
    let prefix = take_prefix(text, prefix_width);
    let suffix = take_suffix(text, suffix_width);
    format!("{prefix}…{suffix}")
}

fn take_prefix(text: &str, maximum_width: usize) -> String {
    let mut width = 0usize;
    text.chars()
        .take_while(|character| {
            let character_width = UnicodeWidthChar::width(*character).unwrap_or_default();
            if width.saturating_add(character_width) > maximum_width {
                false
            } else {
                width += character_width;
                true
            }
        })
        .collect()
}

fn take_suffix(text: &str, maximum_width: usize) -> String {
    let mut width = 0usize;
    let mut characters = text
        .chars()
        .rev()
        .take_while(|character| {
            let character_width = UnicodeWidthChar::width(*character).unwrap_or_default();
            if width.saturating_add(character_width) > maximum_width {
                false
            } else {
                width += character_width;
                true
            }
        })
        .collect::<Vec<_>>();
    characters.reverse();
    characters.into_iter().collect()
}

fn wrap_by_width(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() || width == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_width = 0usize;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or_default();
        if character_width > width {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line_width = 0;
            }
            lines.push("…".to_string());
            continue;
        }
        if line_width > 0 && line_width.saturating_add(character_width) > width {
            lines.push(std::mem::take(&mut line));
            line_width = 0;
        }
        line.push(character);
        line_width += character_width;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_wrap_and_preserve_both_ends() {
        assert_eq!(
            filename_lines("abcdefghijklmnop", 5, 2),
            vec!["abcde", "…mnop"]
        );
    }

    #[test]
    fn filenames_are_centered_in_tall_tracks() {
        assert_eq!(
            filename_lines("sample.bam", 10, 3),
            vec!["", "sample.bam", ""]
        );
    }

    #[test]
    fn filenames_use_terminal_display_width() {
        assert_eq!(filename_lines("前後.bam", 4, 2), vec!["前後", ".bam"]);
        assert_eq!(filename_lines("前後.bam", 4, 1), vec!["前…m"]);
    }
}
