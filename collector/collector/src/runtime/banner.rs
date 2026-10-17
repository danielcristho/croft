//! Startup banner.

use crate::runtime::ansi::{BOLD_CYAN, RESET};

const GLYPH_ROWS: usize = 5;
const WORD: &str = "CROFT";

fn glyph(c: char) -> [&'static str; GLYPH_ROWS] {
    match c {
        'C' => [".###.", "#....", "#....", "#....", ".###."],
        'R' => ["####.", "#...#", "####.", "#..#.", "#...#"],
        'O' => [".###.", "#...#", "#...#", "#...#", ".###."],
        'F' => ["#####", "#....", "####.", "#....", "#...."],
        'T' => ["#####", "..#..", "..#..", "..#..", "..#.."],
        _ => ["....."; GLYPH_ROWS],
    }
}

fn row(index: usize) -> String {
    let mut line = String::new();
    for c in WORD.chars() {
        line.extend(
            glyph(c)[index]
                .chars()
                .map(|cell| if cell == '#' { '█' } else { ' ' }),
        );
        line.push(' ');
    }
    line
}

pub fn print() {
    println!();
    for index in 0..GLYPH_ROWS {
        println!("{BOLD_CYAN}{}{RESET}", row(index));
    }
    println!();
}
