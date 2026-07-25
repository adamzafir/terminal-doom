//! Grid maps, level definitions, collision detection, and ray casting.
//!
//! Coordinates use the conventional ray-caster layout: `x` increases to the
//! right, `y` increases down the map, and every map cell is one world unit.

use std::error::Error;
use std::fmt;
use std::ops::{Add, AddAssign, Div, Mul, Sub, SubAssign};

const EPSILON: f64 = 1.0e-9;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn from_angle(angle: f64) -> Self {
        Self::new(angle.cos(), angle.sin())
    }

    pub fn length_squared(self) -> f64 {
        self.dot(self)
    }

    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    pub fn distance_squared(self, other: Self) -> f64 {
        (self - other).length_squared()
    }

    pub fn distance(self, other: Self) -> f64 {
        self.distance_squared(other).sqrt()
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// Returns a unit vector, or [`Vec2::ZERO`] for a zero/non-finite vector.
    pub fn normalized(self) -> Self {
        let length = self.length();
        if length.is_finite() && length > EPSILON {
            self / length
        } else {
            Self::ZERO
        }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for Vec2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Vec2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl Mul<f64> for Vec2 {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Mul<Vec2> for f64 {
    type Output = Vec2;

    fn mul(self, rhs: Vec2) -> Self::Output {
        rhs * self
    }
}

impl Div<f64> for Vec2 {
    type Output = Self;

    fn div(self, rhs: f64) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    Floor,
    Wall(u8),
    Door { open: bool },
    LockedDoor { open: bool },
    Exit,
}

impl Tile {
    pub fn blocks_movement(self) -> bool {
        matches!(
            self,
            Self::Wall(_) | Self::Door { open: false } | Self::LockedDoor { open: false }
        )
    }

    pub fn blocks_sight(self) -> bool {
        self.blocks_movement()
    }

    pub fn material(self) -> u8 {
        match self {
            Self::Wall(material) => material,
            Self::Door { .. } => 10,
            Self::LockedDoor { .. } => 11,
            Self::Exit => 12,
            Self::Floor => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnKind {
    Enemy(u8),
    Ammo,
    Medkit,
    Armor,
    Key,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spawn {
    pub position: Vec2,
    pub kind: SpawnKind,
}

#[derive(Clone, Debug)]
pub struct Level {
    pub map: Map,
    pub player_start: Vec2,
    pub spawns: Vec<Spawn>,
    pub name: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaySide {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    /// Euclidean world-space distance (the input direction is normalized).
    pub distance: f64,
    pub point: Vec2,
    pub side: RaySide,
    pub material: u8,
    pub tile: Tile,
    pub cell: (usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractResult {
    Nothing,
    OpenedDoor,
    ClosedDoor,
    Locked,
    /// The caller should consume one key when this is returned.
    UnlockedDoor,
    ClosedLockedDoor,
    Exit,
}

impl InteractResult {
    pub fn consumed_key(self) -> bool {
        matches!(self, Self::UnlockedDoor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LevelError {
    Empty,
    TooSmall,
    Ragged {
        row: usize,
        expected: usize,
        found: usize,
    },
    UnknownGlyph {
        glyph: char,
        x: usize,
        y: usize,
    },
    MissingPlayer,
    MultiplePlayers,
    MissingExit,
    OpenBoundary {
        x: usize,
        y: usize,
    },
    WrongTileCount {
        expected: usize,
        found: usize,
    },
}

impl fmt::Display for LevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "level has no rows"),
            Self::TooSmall => write!(f, "level must be at least 3 by 3 cells"),
            Self::Ragged {
                row,
                expected,
                found,
            } => write!(
                f,
                "row {row} has width {found}, but the first row has width {expected}"
            ),
            Self::UnknownGlyph { glyph, x, y } => {
                write!(f, "unknown level glyph {glyph:?} at ({x}, {y})")
            }
            Self::MissingPlayer => write!(f, "level has no player start"),
            Self::MultiplePlayers => write!(f, "level has multiple player starts"),
            Self::MissingExit => write!(f, "level has no exit"),
            Self::OpenBoundary { x, y } => {
                write!(f, "level boundary is not sealed at ({x}, {y})")
            }
            Self::WrongTileCount { expected, found } => {
                write!(f, "expected {expected} tiles, found {found}")
            }
        }
    }
}

impl Error for LevelError {}

#[derive(Clone, Debug)]
pub struct Map {
    pub width: usize,
    pub height: usize,
    tiles: Vec<Tile>,
}

impl Map {
    pub fn new(width: usize, height: usize, tiles: Vec<Tile>) -> Result<Self, LevelError> {
        let expected = width
            .checked_mul(height)
            .ok_or(LevelError::WrongTileCount {
                expected: usize::MAX,
                found: tiles.len(),
            })?;
        if tiles.len() != expected {
            return Err(LevelError::WrongTileCount {
                expected,
                found: tiles.len(),
            });
        }
        Ok(Self {
            width,
            height,
            tiles,
        })
    }

    pub fn get(&self, x: usize, y: usize) -> Option<Tile> {
        if x < self.width && y < self.height {
            Some(self.tiles[y * self.width + x])
        } else {
            None
        }
    }

    /// Signed-coordinate accessor convenient for ray casting. The void outside
    /// a map behaves like an opaque wall.
    pub fn tile(&self, x: i32, y: i32) -> Tile {
        if x < 0 || y < 0 {
            return Tile::Wall(0);
        }
        self.get(x as usize, y as usize).unwrap_or(Tile::Wall(0))
    }

    pub fn set(&mut self, x: usize, y: usize, tile: Tile) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        self.tiles[y * self.width + x] = tile;
        true
    }

    /// Tests a circular actor against solid grid cells.
    pub fn is_walkable(&self, position: Vec2, radius: f64) -> bool {
        if !position.is_finite() || !radius.is_finite() || radius < 0.0 {
            return false;
        }

        if radius <= EPSILON {
            return !self
                .tile(position.x.floor() as i32, position.y.floor() as i32)
                .blocks_movement();
        }

        let min_x = (position.x - radius).floor() as i32;
        let max_x = (position.x + radius).floor() as i32;
        let min_y = (position.y - radius).floor() as i32;
        let max_y = (position.y + radius).floor() as i32;
        let radius_sq = radius * radius;

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if !self.tile(x, y).blocks_movement() {
                    continue;
                }
                let nearest_x = position.x.clamp(x as f64, x as f64 + 1.0);
                let nearest_y = position.y.clamp(y as f64, y as f64 + 1.0);
                let dx = position.x - nearest_x;
                let dy = position.y - nearest_y;
                if dx * dx + dy * dy < radius_sq {
                    return false;
                }
            }
        }
        true
    }

    pub fn has_line_of_sight(&self, from: Vec2, to: Vec2) -> bool {
        let delta = to - from;
        let distance = delta.length();
        if !from.is_finite() || !to.is_finite() {
            return false;
        }
        if distance <= EPSILON {
            return !self
                .tile(from.x.floor() as i32, from.y.floor() as i32)
                .blocks_sight();
        }
        match self.cast_ray(from, delta, distance) {
            Some(hit) => hit.distance + 1.0e-7 >= distance,
            None => true,
        }
    }

    /// Casts through the grid using DDA. `direction` need not be normalized.
    pub fn cast_ray(&self, origin: Vec2, direction: Vec2, max_distance: f64) -> Option<RayHit> {
        if !origin.is_finite()
            || !direction.is_finite()
            || !max_distance.is_finite()
            || max_distance < 0.0
        {
            return None;
        }
        let direction = direction.normalized();
        if direction == Vec2::ZERO {
            return None;
        }

        let mut map_x = origin.x.floor() as i32;
        let mut map_y = origin.y.floor() as i32;
        if !self.in_bounds(map_x, map_y) {
            return None;
        }

        let starting_tile = self.tile(map_x, map_y);
        if starting_tile.blocks_sight() {
            return Some(RayHit {
                distance: 0.0,
                point: origin,
                side: RaySide::Vertical,
                material: starting_tile.material(),
                tile: starting_tile,
                cell: (map_x as usize, map_y as usize),
            });
        }

        let delta_x = if direction.x.abs() <= EPSILON {
            f64::INFINITY
        } else {
            1.0 / direction.x.abs()
        };
        let delta_y = if direction.y.abs() <= EPSILON {
            f64::INFINITY
        } else {
            1.0 / direction.y.abs()
        };
        let (step_x, mut side_x) = if direction.x < 0.0 {
            (-1, (origin.x - map_x as f64) * delta_x)
        } else {
            (1, (map_x as f64 + 1.0 - origin.x) * delta_x)
        };
        let (step_y, mut side_y) = if direction.y < 0.0 {
            (-1, (origin.y - map_y as f64) * delta_y)
        } else {
            (1, (map_y as f64 + 1.0 - origin.y) * delta_y)
        };

        loop {
            let (distance, side) = if side_x < side_y {
                map_x += step_x;
                let distance = side_x;
                side_x += delta_x;
                (distance, RaySide::Vertical)
            } else {
                map_y += step_y;
                let distance = side_y;
                side_y += delta_y;
                (distance, RaySide::Horizontal)
            };

            if distance > max_distance + EPSILON || !self.in_bounds(map_x, map_y) {
                return None;
            }
            let tile = self.tile(map_x, map_y);
            if tile.blocks_sight() {
                return Some(RayHit {
                    distance,
                    point: origin + direction * distance,
                    side,
                    material: tile.material(),
                    tile,
                    cell: (map_x as usize, map_y as usize),
                });
            }
        }
    }

    /// Toggles the first door in front of the player. Locked doors report key
    /// consumption via [`InteractResult::UnlockedDoor`], but inventory mutation
    /// remains the caller's responsibility.
    pub fn interact(
        &mut self,
        origin: Vec2,
        direction: Vec2,
        reach: f64,
        has_key: bool,
    ) -> InteractResult {
        let Some((x, y, tile)) = self.first_interactable(origin, direction, reach) else {
            return InteractResult::Nothing;
        };

        let (replacement, result) = match tile {
            Tile::Door { open: false } => (Tile::Door { open: true }, InteractResult::OpenedDoor),
            Tile::Door { open: true } => (Tile::Door { open: false }, InteractResult::ClosedDoor),
            Tile::LockedDoor { open: false } if has_key => (
                Tile::LockedDoor { open: true },
                InteractResult::UnlockedDoor,
            ),
            Tile::LockedDoor { open: false } => (tile, InteractResult::Locked),
            Tile::LockedDoor { open: true } => (
                Tile::LockedDoor { open: false },
                InteractResult::ClosedLockedDoor,
            ),
            Tile::Exit => (tile, InteractResult::Exit),
            _ => return InteractResult::Nothing,
        };
        let _ = self.set(x, y, replacement);
        result
    }

    fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height
    }

    fn first_interactable(
        &self,
        origin: Vec2,
        direction: Vec2,
        reach: f64,
    ) -> Option<(usize, usize, Tile)> {
        if !origin.is_finite() || !direction.is_finite() || !reach.is_finite() || reach < 0.0 {
            return None;
        }
        let direction = direction.normalized();
        if direction == Vec2::ZERO {
            return None;
        }

        let mut map_x = origin.x.floor() as i32;
        let mut map_y = origin.y.floor() as i32;
        if !self.in_bounds(map_x, map_y) {
            return None;
        }
        let delta_x = if direction.x.abs() <= EPSILON {
            f64::INFINITY
        } else {
            1.0 / direction.x.abs()
        };
        let delta_y = if direction.y.abs() <= EPSILON {
            f64::INFINITY
        } else {
            1.0 / direction.y.abs()
        };
        let (step_x, mut side_x) = if direction.x < 0.0 {
            (-1, (origin.x - map_x as f64) * delta_x)
        } else {
            (1, (map_x as f64 + 1.0 - origin.x) * delta_x)
        };
        let (step_y, mut side_y) = if direction.y < 0.0 {
            (-1, (origin.y - map_y as f64) * delta_y)
        } else {
            (1, (map_y as f64 + 1.0 - origin.y) * delta_y)
        };

        loop {
            let distance = if side_x < side_y {
                map_x += step_x;
                let distance = side_x;
                side_x += delta_x;
                distance
            } else {
                map_y += step_y;
                let distance = side_y;
                side_y += delta_y;
                distance
            };
            if distance > reach + EPSILON || !self.in_bounds(map_x, map_y) {
                return None;
            }
            let tile = self.tile(map_x, map_y);
            if matches!(
                tile,
                Tile::Door { .. } | Tile::LockedDoor { .. } | Tile::Exit
            ) {
                return Some((map_x as usize, map_y as usize, tile));
            }
            if tile.blocks_sight() {
                return None;
            }
        }
    }
}

impl Level {
    /// Parse a compact map blueprint.
    ///
    /// `#` and `1`-`9` are walls; `+`/`/` are closed/open doors; `L`/`l`
    /// are closed/open locked doors; `P` is the player; `e`, `g`, and `b`
    /// spawn enemy classes 0, 1, and 2; `A`, `H`, `R`, and `K` are pickups;
    /// and `X` is an exit. Spaces and `.` are floor.
    pub fn from_ascii(name: &'static str, rows: &[&str]) -> Result<Self, LevelError> {
        if rows.is_empty() {
            return Err(LevelError::Empty);
        }
        let width = rows[0].chars().count();
        let height = rows.len();
        if width < 3 || height < 3 {
            return Err(LevelError::TooSmall);
        }

        let mut tiles = Vec::with_capacity(width * height);
        let mut player_start = None;
        let mut spawns = Vec::new();
        let mut exits = 0usize;

        for (y, row) in rows.iter().enumerate() {
            let found = row.chars().count();
            if found != width {
                return Err(LevelError::Ragged {
                    row: y,
                    expected: width,
                    found,
                });
            }
            for (x, glyph) in row.chars().enumerate() {
                let position = Vec2::new(x as f64 + 0.5, y as f64 + 0.5);
                let tile = match glyph {
                    '#' => Tile::Wall(1),
                    '1'..='9' => Tile::Wall(glyph as u8 - b'0'),
                    '.' | ' ' => Tile::Floor,
                    '+' | 'D' => Tile::Door { open: false },
                    '/' | 'd' => Tile::Door { open: true },
                    'L' => Tile::LockedDoor { open: false },
                    'l' => Tile::LockedDoor { open: true },
                    'X' => {
                        exits += 1;
                        Tile::Exit
                    }
                    'P' => {
                        if player_start.replace(position).is_some() {
                            return Err(LevelError::MultiplePlayers);
                        }
                        Tile::Floor
                    }
                    'e' | 'g' | 'b' | 'A' | 'H' | 'R' | 'K' => {
                        let kind = match glyph {
                            'e' => SpawnKind::Enemy(0),
                            'g' => SpawnKind::Enemy(1),
                            'b' => SpawnKind::Enemy(2),
                            'A' => SpawnKind::Ammo,
                            'H' => SpawnKind::Medkit,
                            'R' => SpawnKind::Armor,
                            'K' => SpawnKind::Key,
                            _ => unreachable!(),
                        };
                        spawns.push(Spawn { position, kind });
                        Tile::Floor
                    }
                    _ => return Err(LevelError::UnknownGlyph { glyph, x, y }),
                };
                if (x == 0 || y == 0 || x + 1 == width || y + 1 == height)
                    && !tile.blocks_movement()
                {
                    return Err(LevelError::OpenBoundary { x, y });
                }
                tiles.push(tile);
            }
        }

        let player_start = player_start.ok_or(LevelError::MissingPlayer)?;
        if exits == 0 {
            return Err(LevelError::MissingExit);
        }
        Ok(Self {
            map: Map::new(width, height, tiles)?,
            player_start,
            spawns,
            name,
        })
    }
}

/// Three escalating, validated campaign maps.
pub fn builtin_levels() -> Vec<Level> {
    [
        (
            "The Hangar",
            &[
                "11111111111111111111",
                "1P....A....1.......1",
                "1.111.111..1..e.H..1",
                "1.....1....+.......1",
                "1..e..1....1..111..1",
                "1.....111111.......1",
                "1..R.........e.....1",
                "1........11111.....1",
                "1..H.....1...1..X..1",
                "11111111111111111111",
            ][..],
        ),
        (
            "The Foundry",
            &[
                "2222222222222222222222222",
                "2P.....2...A....2.......2",
                "2.222..2.22222..2..g.H..2",
                "2...2..2....e2..+.......2",
                "2.e.2..2222..2..2.2222..2",
                "2...2........2..2....2..2",
                "2...2222.22222..222..2..2",
                "2.R......2.....K.....2..2",
                "22222+2222.222222L2222..2",
                "2..........2........g...2",
                "2..A..g....2..b......X..2",
                "2222222222222222222222222",
            ][..],
        ),
        (
            "Infernal Keep",
            &[
                "3333333333333333333333333333",
                "3P....e....3.....A....3....3",
                "3.333333...3.333333..3..g..3",
                "3......3...+....e.3..+.....3",
                "3..b...3...3......3..3.33..3",
                "3333L333...3333+333..3..3..3",
                "3..K...3.............3..3..3",
                "3......33333.333333333..3..3",
                "3.333........3.....g...3...3",
                "3...3..e.....3.33333333....3",
                "3.R.3333333..3.....H....b..3",
                "3........A...L............X3",
                "3333333333333333333333333333",
            ][..],
        ),
    ]
    .into_iter()
    .map(|(name, rows)| {
        Level::from_ascii(name, rows)
            .unwrap_or_else(|error| panic!("invalid built-in level {name:?}: {error}"))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_map(rows: &[&str]) -> Map {
        Level::from_ascii("test", rows).unwrap().map
    }

    #[test]
    fn vector_math_is_well_behaved() {
        let v = Vec2::new(3.0, 4.0);
        assert_eq!(v.length(), 5.0);
        assert_eq!(v.normalized(), Vec2::new(0.6, 0.8));
        assert_eq!(Vec2::ZERO.normalized(), Vec2::ZERO);
        assert_eq!(v.distance(Vec2::new(3.0, 0.0)), 4.0);
    }

    #[test]
    fn parser_extracts_entities_and_rejects_bad_maps() {
        let level = Level::from_ascii("tiny", &["111111", "1PeAX1", "1H.RK1", "111111"]).unwrap();
        assert_eq!(level.map.width, 6);
        assert_eq!(level.player_start, Vec2::new(1.5, 1.5));
        assert_eq!(level.spawns.len(), 5);
        assert_eq!(level.map.get(4, 1), Some(Tile::Exit));

        assert!(matches!(
            Level::from_ascii("ragged", &["1111", "1PX1", "111"]),
            Err(LevelError::Ragged { .. })
        ));
        assert!(matches!(
            Level::from_ascii("open", &["1111", "1PX.", "1111"]),
            Err(LevelError::OpenBoundary { .. })
        ));
    }

    #[test]
    fn circle_collision_respects_radius_and_doors() {
        let mut map = test_map(&["11111", "1P+.1", "1..X1", "11111"]);
        assert!(map.is_walkable(Vec2::new(1.5, 1.5), 0.30));
        assert!(!map.is_walkable(Vec2::new(1.85, 1.5), 0.20));
        assert_eq!(
            map.interact(Vec2::new(1.5, 1.5), Vec2::new(1.0, 0.0), 1.0, false),
            InteractResult::OpenedDoor
        );
        assert!(map.is_walkable(Vec2::new(1.85, 1.5), 0.20));
    }

    #[test]
    fn ray_cast_reports_distance_side_and_material() {
        let map = test_map(&["22222", "2P..2", "2..X2", "22222"]);
        let hit = map
            .cast_ray(Vec2::new(1.5, 1.5), Vec2::new(5.0, 0.0), 20.0)
            .unwrap();
        assert!((hit.distance - 2.5).abs() < 1.0e-9);
        assert_eq!(hit.point, Vec2::new(4.0, 1.5));
        assert_eq!(hit.side, RaySide::Vertical);
        assert_eq!(hit.material, 2);
        assert_eq!(hit.cell, (4, 1));
    }

    #[test]
    fn line_of_sight_stops_at_walls_and_closed_doors() {
        let mut map = test_map(&["1111111", "1P.+.X1", "1.....1", "1111111"]);
        let a = Vec2::new(1.5, 1.5);
        let b = Vec2::new(5.5, 1.5);
        assert!(!map.has_line_of_sight(a, b));
        assert_eq!(
            map.interact(a, Vec2::new(1.0, 0.0), 3.0, false),
            InteractResult::OpenedDoor
        );
        assert!(map.has_line_of_sight(a, b));
    }

    #[test]
    fn locked_door_requires_and_consumes_a_key_signal() {
        let mut map = test_map(&["11111", "1PLX1", "1...1", "11111"]);
        let origin = Vec2::new(1.5, 1.5);
        assert_eq!(
            map.interact(origin, Vec2::new(1.0, 0.0), 1.0, false),
            InteractResult::Locked
        );
        assert_eq!(map.get(2, 1), Some(Tile::LockedDoor { open: false }));
        let result = map.interact(origin, Vec2::new(1.0, 0.0), 1.0, true);
        assert_eq!(result, InteractResult::UnlockedDoor);
        assert!(result.consumed_key());
        assert_eq!(map.get(2, 1), Some(Tile::LockedDoor { open: true }));
    }

    #[test]
    fn all_campaign_levels_are_valid_and_populated() {
        let levels = builtin_levels();
        assert_eq!(levels.len(), 3);
        for level in levels {
            assert!(level.map.width >= 20);
            assert!(!level.spawns.is_empty());
            assert!(level.map.is_walkable(level.player_start, 0.25));
        }
    }
}
