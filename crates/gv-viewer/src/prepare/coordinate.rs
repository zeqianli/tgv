//! Text and tick spacing for the coordinate ruler.

/// The distance between ruler ticks, in bases, and its power of ten: the smallest of 2, 10, 20,
/// 100, 200, and so on that is at least `min_distance` bases.
pub fn calculate_intermarker_distance(min_distance: u64) -> (u64, u64) {
    let mut distance = min_distance;

    // Find the smallest number among [1,2,10,20,50,100, ....] that is greater than or equal to min_distance
    let mut power: usize = 1;
    while distance > 10 {
        distance /= 10;
        power += 1;
    }

    if distance < 2 {
        (2 * 10u64.pow(power as u32 - 1), power as u64 - 1)
    } else {
        (10u64.pow(power as u32), power as u64)
    }
}

/// A coordinate in the unit of its tick spacing's power of ten, such as `20kb`.
pub fn get_abbreviated_coordinate_text(coordinate: u64, power: u64) -> String {
    if power < 3 {
        format!("{}bp", to_thousand_separated(coordinate))
    } else if power < 6 {
        format!("{}kb", to_thousand_separated(coordinate / 1_000))
    } else if power < 9 {
        format!("{}Mb", to_thousand_separated(coordinate / 1_000_000))
    } else if power < 12 {
        format!("{}Gb", to_thousand_separated(coordinate / 1_000_000_000))
    } else {
        format!(
            "{}Tb",
            to_thousand_separated(coordinate / 1_000_000_000_000)
        )
    }
}

/// A number with thousands separators, such as `1,234,567`.
pub fn to_thousand_separated(number: u64) -> String {
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

/// A contig length in its largest unit, such as `248Mb`.
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
