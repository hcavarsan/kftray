pub const ASCII_LOGO: &str = r#"
.............................................
.............................................
.............................................
.............................................
...ooooo..............................ooooo..
..oooooooooo......................oooooooooo.
..oooooooooooooo..............oooooooooooooo.
..oooooooooooooooooo......oooooooooooooooooo.
..ooooooo..ooooooooooooooooooooooo...ooooooo.
..ooooooo......ooooooooooooooo.......ooooooo.
..ooooooo..........ooooooo...........ooooooo.
..oooooooo.........................ooooooooo.
...oooooooooooo................ooooooooooooo.
....ooooooooooooooo........oooooooooooooooo..
.......ooooooooooooooo.oooooooooooooooo......
...........oooooooooooooooooooooooo..........
...............oooooooooooooooo..............
...................oooooooo..................
.............................................
.............................................
.............................................
"#;

pub fn resize_ascii_art(ascii_art: &str, scale: f32) -> Vec<String> {
    let lines: Vec<&str> = ascii_art.lines().collect();
    let original_height = lines.len();
    let original_width = lines.iter().map(|line| line.len()).max().unwrap_or(0);

    let new_height = (original_height as f32 * scale).round() as usize;
    let new_width = (original_width as f32 * scale).round() as usize;

    let mut resized_art = Vec::new();

    for i in 0..new_height {
        let original_i = (i as f32 / scale).round() as usize;
        if original_i >= original_height {
            break;
        }

        let mut new_line = String::new();
        for j in 0..new_width {
            let original_j = (j as f32 / scale).round() as usize;
            if original_j >= original_width {
                break;
            }

            let char = lines[original_i].chars().nth(original_j).unwrap_or(' ');
            new_line.push(char);
        }
        resized_art.push(new_line);
    }

    resized_art
}
