use macroquad::prelude::*;
use shared::map::{Map, get_level};

// world constants, one grid cell = one world unit
const WALL_HEIGHT: f32 = 1.0;

fn draw_maze(map: &Map) {
    // floor: centre of a 16x16 grid is (8, 8); vec2(8., 8.) are half-extents
    draw_plane(vec3(8.0, 0.0, 8.0), vec2(8.0, 8.0), None, DARKGRAY);

    // one cube per wall cell
    for gy in 0..map.height {
        for gx in 0..map.width {
            if map.is_wall(gx, gy) {
                // cube position is its CENTRE:
                //   grid cell (gx, gy) spans gx..gx+1, so centre is gx + 0.5
                //   map's y axis becomes world z; y is up
                draw_cube(
                    vec3(gx as f32 + 0.5, WALL_HEIGHT / 2.0, gy as f32 + 0.5),
                    vec3(1.0, WALL_HEIGHT, 1.0),
                    None,
                    GRAY,
                );
            }
        }
    }
}

#[macroquad::main("Maze Runner FPS")]
async fn main() {
    // load level once, outside the loop
    let map = get_level(1);

    loop {
        clear_background(BLACK);

        // 3D World Space
        set_camera(&Camera3D {
            position: vec3(8.0, 20.0, 8.0), // 20 units above maze centre
            target: vec3(8.0, 0.0, 8.0), // looking straight down at centre
            up: vec3(0.0, 0.0, -1.0), // when looking straight down, "up" on
            // screen must be a ground direction; -z makes north point up
            ..Default::default()
        });

        draw_maze(&map);

        // 2D screen space (HUD)
        set_default_camera();

        let fps = get_fps();
        let fps_text = format!("FPS: {}", fps);

        draw_text(&fps_text, screen_width() - 100.0, 30.0, 24.0, YELLOW);

        next_frame().await
    }
}
