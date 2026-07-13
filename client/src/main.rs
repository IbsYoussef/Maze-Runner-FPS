use macroquad::prelude::*;
use shared::map::{Map, get_level};

// world constants, one grid cell = one world unit
const WALL_HEIGHT: f32 = 1.0;
const EYE_HEIGHT: f32 = 0.5; // camera height off the floor
const MOVE_SPEED: f32 = 3.0; // world units per second
const MOUSE_SENSITIVITY: f32 = 1.5;

struct LocalPlayer {
    x: f32,
    z: f32,
    yaw: f32,
}

impl LocalPlayer {
    fn forward(&self) -> Vec3 {
        vec3(self.yaw.sin(), 0.0, self.yaw.cos())
    }

    fn update(&mut self, map: &Map, dt: f32, look_dx: f32) {
        // --- mouse look: all turning comes from the mouse ---
        self.yaw += look_dx * MOUSE_SENSITIVITY;
        // keyboard turning fallback — works in WSLg where raw mouse motion doesn't
        const KEY_TURN_SPEED: f32 = 2.5; // radians per second
        if is_key_down(KeyCode::Left) {
            self.yaw += KEY_TURN_SPEED * dt;
        }
        if is_key_down(KeyCode::Right) {
            self.yaw -= KEY_TURN_SPEED * dt;
        }

        // --- WASD movement: W/S along forward, A/D strafe along right ---
        let fwd = self.forward();
        // right = forward x up = (-cos, 0, sin)
        let right = vec3(-self.yaw.cos(), 0.0, self.yaw.sin());

        let mut wish = vec3(0.0, 0.0, 0.0); // desired movement direction
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
            wish += fwd;
        }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
            wish -= fwd;
        }
        if is_key_down(KeyCode::D) {
            wish += right;
        }
        if is_key_down(KeyCode::A) {
            wish -= right;
        }

        // normalise so diagonal movement isn't faster than straight
        if wish.length() > 0.0 {
            wish = wish.normalize() * MOVE_SPEED * dt;
        }

        let new_x = self.x + wish.x;
        let new_z = self.z + wish.z;

        // axis-separated collision, lets you slide along walls
        if !map.is_wall(new_x as usize, self.z as usize) {
            self.x = new_x;
        }
        if !map.is_wall(self.x as usize, new_z as usize) {
            self.z = new_z;
        }
    }

    fn camera(&self) -> Camera3D {
        let pos = vec3(self.x, EYE_HEIGHT, self.z);
        Camera3D {
            position: pos,
            target: pos + self.forward(),
            up: vec3(0.0, 1.0, 0.0),
            ..Default::default()
        }
    }
}

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
    let mut player = LocalPlayer {
        x: 1.5,
        z: 1.5,
        yaw: 0.0,
    };

    let mut grabbed = true;
    set_cursor_grab(true); // lock the mouse to the window
    show_mouse(false);

    loop {
        let dt = get_frame_time();

        // Escape releases the mouse; click re-grabs it
        if is_key_pressed(KeyCode::Escape) {
            grabbed = false;
            set_cursor_grab(false);
            show_mouse(true);
        }

        if is_mouse_button_pressed(MouseButton::Left) && !grabbed {
            grabbed = true;
            set_cursor_grab(true);
            show_mouse(false);
        }

        // manual mouse delta — robust where mouse_delta_position() misbehaves
        let raw = mouse_delta_position();
        let look_dx = if grabbed && raw.x.abs() < 0.2 {
            -raw.x
        } else {
            0.0
        };
        player.update(&map, dt, look_dx);

        clear_background(BLACK);

        set_camera(&player.camera());
        draw_maze(&map);

        set_default_camera();
        let fps_text = format!("FPS: {}", get_fps());
        draw_text(&fps_text, screen_width() - 100.0, 30.0, 24.0, YELLOW);

        next_frame().await
    }
}
