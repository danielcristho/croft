// Croft Banner

// Consts
const GLYPH_ROWS: usize = 5;
const WORD: &str = "CROFT";
const CYAN: &str = "\x1b[1;36m";
const RESET: &str = "\x1b[0m";

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

pub fn print() {
    println!();
    for row in 0..GLYPH_ROWS {
        let mut line = String::new();
        for c in WORD.chars() {
            for cell in glyph(c)[row].chars() {
                line.push_str(if cell == '#' { "██" } else { "  " });
            }
            line.push_str("  ");
        }
        // Doubled: one glyph row reads as too flat and thin otherwise.
        println!("{CYAN}{line}{RESET}");
        println!("{CYAN}{line}{RESET}");
    }
    println!();
}
