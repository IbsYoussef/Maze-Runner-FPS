use macroquad::prelude::*;

#[macroquad::main("Maze Runner FPS")]
async fn main() {
    loop {
        clear_background(BLACK);

        let fps = get_fps();
        let fps_text = format!("FPS: {}", fps);

        draw_text(&fps_text, screen_width() - 100.0, 30.0, 24.0, YELLOW);

        next_frame().await
    }
}
