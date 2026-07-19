// map.rs
// The maze grid itself: its shape, and the three level layouts.
//
// A Map is just a flat list of numbers describing a square grid, where
// each number is either a wall or open floor. Both the server and the
// client use the exact same Map data, the server uses it to check
// collisions and line of sight for shooting, the client uses it to draw
// the walls and to stop the local player from walking through them.

/// A single level's layout: its size, and which cells are walls.
///
/// The grid is stored as one flat list rather than a 2D array, which is
/// a very common way to represent a grid in a single block of memory.
/// Cell (x, y) lives at index `y * width + x` in that list, see
/// `is_wall` below for exactly how that lookup works.
#[derive(Debug, Clone)]
pub struct Map {
    /// how many cells wide the grid is
    pub width: usize,
    /// how many cells tall the grid is
    pub height: usize,
    /// every cell in the grid, one after another, row by row.
    /// a value of 0 means open floor, a value of 1 means a solid wall
    pub cells: Vec<u8>,
}

impl Map {
    /// Returns true if the given grid cell is a wall.
    ///
    /// Coordinates outside the grid are also treated as a wall. This
    /// matters more than it might seem: without this check, a player
    /// standing right at the edge of the map and taking one more step
    /// outward would read past the end of the `cells` list, which is a
    /// crash in Rust. Treating "off the edge of the map" the same as
    /// "wall" is both simpler and completely safe.
    pub fn is_wall(&self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height {
            return true;
        }
        self.cells[y * self.width + x] == 1
    }
}

// Every level below is a 16 by 16 grid. In the list of numbers, 1 means
// a wall and 0 means open floor you can walk through. The numbers are
// written out in reading order, left to right, then top to bottom, one
// row after another, exactly matching how `is_wall` looks them up.

/// Level 1: wide, simple corridors. The easiest of the three levels,
/// meant to be the first one most players encounter.
pub fn level_1() -> Map {
    Map {
        width: 16,
        height: 16,
        cells: vec![
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 1,
            0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1,
            1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1,
            1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1,
            0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1,
            0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 0,
            0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        ],
    }
}

/// Level 2: tighter corridors with more dead ends than level 1, making
/// navigation somewhat harder while still being fairly readable.
pub fn level_2() -> Map {
    Map {
        width: 16,
        height: 16,
        cells: vec![
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0,
            0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0,
            0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0,
            0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1,
            0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1,
            0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1,
            1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0,
            1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        ],
    }
}

/// Level 3: the densest and hardest of the three, with the longest
/// paths between open areas. Meant as the final, most difficult level.
pub fn level_3() -> Map {
    Map {
        width: 16,
        height: 16,
        cells: vec![
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0,
            1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0,
            1, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0,
            0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1,
            0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1,
            0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1,
            0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0,
            1, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        ],
    }
}

/// Picks a level by number. 1 and 2 return their matching level, and
/// anything else, including 3 and any unexpected value, falls back to
/// level 3. This is what the server uses to load whichever level was
/// requested with the `--level` command line argument on startup.
pub fn get_level(n: u8) -> Map {
    match n {
        1 => level_1(),
        2 => level_2(),
        _ => level_3(),
    }
}
