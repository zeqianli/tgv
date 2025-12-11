pub fn get_abbreviated_length_string(length: usize) -> String {
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
