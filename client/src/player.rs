// player.rs
// The local player: position, facing direction, movement, and the camera
// built from those values.

use macroquad::prelude::*;

pub const MOVE_SPEED: f32 = 3.0; // world units per second
pub const MOUSE_SENSITIVITY: f32 = 1.5;
pub const KEY_TURN_SPEED: f32 = 2.5; // radians per second
pub const EYE_HEIGHT: f32 = 0.5; // camera height off the floor

pub struct LocalPlayer {
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
}

impl LocalPlayer {
    // the direction the player is currently facing, as a unit vector
    pub fn forward(&self) -> Vec3 {
        vec3(self.yaw.sin(), 0.0, self.yaw.cos())
    }

    pub fn update(&mut self, map: &shared::map::Map, dt: f32, look_dx: f32) {
        // turning: the mouse is the primary way to look around
        self.yaw += look_dx * MOUSE_SENSITIVITY;

        // arrow keys are a fallback for turning. This matters specifically
        // under WSLg (Windows Subsystem for Linux's display server), where
        // raw mouse motion does not come through reliably. On native
        // Windows or native Linux the mouse works correctly and these keys
        // are simply unused.
        if is_key_down(KeyCode::Left) {
            self.yaw += KEY_TURN_SPEED * dt;
        }
        if is_key_down(KeyCode::Right) {
            self.yaw -= KEY_TURN_SPEED * dt;
        }

        let fwd = self.forward();
        // the direction to the player's right, at 90 degrees from forward
        let right = vec3(-self.yaw.cos(), 0.0, self.yaw.sin());

        let mut wish = vec3(0.0, 0.0, 0.0);
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

        // without this, holding two movement keys at once (like W and D)
        // would move diagonally faster than moving in a single direction
        if wish.length() > 0.0 {
            wish = wish.normalize() * MOVE_SPEED * dt;
        }

        let new_x = self.x + wish.x;
        let new_z = self.z + wish.z;

        // checking each axis separately lets the player slide along a wall
        // instead of stopping dead the moment they touch it
        if !map.is_wall(new_x as usize, self.z as usize) {
            self.x = new_x;
        }
        if !map.is_wall(self.x as usize, new_z as usize) {
            self.z = new_z;
        }
    }

    pub fn camera(&self) -> Camera3D {
        let pos = vec3(self.x, EYE_HEIGHT, self.z);
        Camera3D {
            position: pos,
            target: pos + self.forward(),
            up: vec3(0.0, 1.0, 0.0),
            ..Default::default()
        }
    }
}
