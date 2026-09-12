// Croft Banner

// Consts
const GLYPH_ROWS: usize = 5;
const WORD: &str = "CROFT";
const CYAN: &str = "\x1b[1;36m";
const RESET: &str = "\x1b[0m";

// glyph returns the 5x5 representation of a character in the Croft banner.
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

// print prints the Croft banner to stdout.
pub fn print() {
    println!();
    for row in 0..GLYPH_ROWS {
        let mut line = String::new();
        for c in WORD.chars() {
            for cell in glyph(c)[row].chars() {
                line.push(if cell == '#' { '█' } else { ' ' });
            }
            line.push(' ');
        }
        println!("{CYAN}{line}{RESET}");
    }
    println!();
}
