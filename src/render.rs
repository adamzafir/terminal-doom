//! Terminal presentation for the game.
//!
//! This module deliberately consumes a small, read-only render snapshot rather
//! than `game::Game`.  It can therefore be tested independently and does not own
//! terminal setup, teardown, timing, or input.

use std::f64::consts::PI;
use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
};

use crate::world::{Map, RaySide, Tile, Vec2};

const MIN_WIDTH: u16 = 50;
const MIN_HEIGHT: u16 = 18;
const HUD_HEIGHT: u16 = 4;

/// One styled terminal character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
}

impl Cell {
    pub const fn new(ch: char, fg: Color, bg: Color) -> Self {
        Self {
            ch,
            fg,
            bg,
            bold: false,
        }
    }

    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::new(' ', Color::White, Color::Black)
    }
}

/// A complete terminal frame. Every cell stores its own presentation style.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    cells: Vec<Cell>,
}

impl Frame {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            cells: vec![Cell::default(); width as usize * height as usize],
        }
    }

    #[inline]
    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        if x < self.width && y < self.height {
            Some(&self.cells[y as usize * self.width as usize + x as usize])
        } else {
            None
        }
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, cell: Cell) {
        if x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32 {
            self.cells[y as usize * self.width as usize + x as usize] = cell;
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, width: i32, height: i32, cell: Cell) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + width).min(self.width as i32);
        let y1 = (y + height).min(self.height as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                self.set(px, py, cell);
            }
        }
    }

    pub fn text(&mut self, x: i32, y: i32, text: &str, style: Cell) {
        for (offset, ch) in text.chars().enumerate() {
            self.set(x + offset as i32, y, Cell { ch, ..style });
        }
    }

    pub fn centered_text(&mut self, y: i32, text: &str, style: Cell) {
        let len = text.chars().count() as i32;
        self.text((self.width as i32 - len) / 2, y, text, style);
    }

    /// Queue the frame to a writer using ANSI commands supplied by crossterm.
    ///
    /// Adjacent cells with the same style are emitted as a single string. The
    /// caller remains responsible for flushing and terminal lifecycle.
    pub fn queue_to<W: Write>(&self, out: &mut W) -> io::Result<()> {
        self.queue_changes_to(None, out)
    }

    /// Queue only cells that differ from the previous frame.
    ///
    /// Avoiding unchanged cells materially reduces terminal traffic and keeps
    /// static UI such as the HUD from being repainted—and visibly flashing—on
    /// every simulation tick.
    fn queue_changes_to<W: Write>(&self, previous: Option<&Self>, out: &mut W) -> io::Result<()> {
        let previous = previous.filter(|frame| {
            frame.width == self.width
                && frame.height == self.height
                && frame.cells.len() == self.cells.len()
        });
        let mut current: Option<(Color, Color, bool)> = None;

        for (y, row) in self.cells.chunks(self.width as usize).enumerate() {
            let mut index = 0;
            while index < row.len() {
                let unchanged = previous.is_some_and(|frame| {
                    frame.cells[y * self.width as usize + index] == row[index]
                });
                if unchanged {
                    index += 1;
                    continue;
                }

                queue!(out, MoveTo(index as u16, y as u16))?;
                let style = (row[index].fg, row[index].bg, row[index].bold);
                if current != Some(style) {
                    queue!(
                        out,
                        SetForegroundColor(style.0),
                        SetBackgroundColor(style.1),
                        SetAttribute(if style.2 {
                            Attribute::Bold
                        } else {
                            Attribute::NormalIntensity
                        })
                    )?;
                    current = Some(style);
                }

                let mut run = String::new();
                while index < row.len()
                    && (row[index].fg, row[index].bg, row[index].bold) == style
                    && previous.is_none_or(|frame| {
                        frame.cells[y * self.width as usize + index] != row[index]
                    })
                {
                    run.push(row[index].ch);
                    index += 1;
                }
                queue!(out, Print(run))?;
            }
        }
        queue!(out, ResetColor, SetAttribute(Attribute::Reset))?;
        Ok(())
    }

    pub fn to_ansi_bytes(&self) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.queue_to(&mut bytes)?;
        Ok(bytes)
    }
}

/// Presents completed frames atomically when the terminal supports synchronized
/// updates and falls back gracefully to differential output when it does not.
#[derive(Clone, Debug, Default)]
pub struct FramePresenter {
    previous: Option<Frame>,
}

impl FramePresenter {
    pub fn present<W: Write>(&mut self, frame: &Frame, out: &mut W) -> io::Result<()> {
        const BEGIN_SYNC: &[u8] = b"\x1b[?2026h";
        const END_SYNC: &[u8] = b"\x1b[?2026l";

        let mut update = Vec::with_capacity(frame.cells.len() * 2);
        update.extend_from_slice(BEGIN_SYNC);
        frame.queue_changes_to(self.previous.as_ref(), &mut update)?;
        update.extend_from_slice(END_SYNC);
        out.write_all(&update)?;
        out.flush()?;
        self.previous = Some(frame.clone());
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub position: Vec2,
    pub angle: f64,
    pub fov: f64,
}

impl Camera {
    pub fn new(position: Vec2, angle: f64) -> Self {
        Self {
            position,
            angle,
            fov: PI / 3.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityVisual {
    Trooper,
    Imp,
    Demon,
    Cacodemon,
    Boss,
    Barrel,
    Fireball,
}

#[derive(Clone, Copy, Debug)]
pub struct RenderEntity {
    pub position: Vec2,
    pub visual: EntityVisual,
    pub health_fraction: f32,
    pub hit_flash: f32,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickupVisual {
    Bullets,
    Shells,
    Medkit,
    Armor,
    Shotgun,
    Chaingun,
    Key,
}

#[derive(Clone, Copy, Debug)]
pub struct RenderPickup {
    pub position: Vec2,
    pub visual: PickupVisual,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponVisual {
    Pistol,
    Shotgun,
    Chaingun,
}

#[derive(Clone, Copy, Debug)]
pub struct WeaponView {
    pub visual: WeaponVisual,
    /// Walking sway in radians. `0.0` is centered.
    pub bob: f32,
    /// Normalized backwards kick, usually in `0.0..=1.0`.
    pub recoil: f32,
    /// Normalized muzzle flash, usually in `0.0..=1.0`.
    pub muzzle_flash: f32,
}

impl Default for WeaponView {
    fn default() -> Self {
        Self {
            visual: WeaponVisual::Pistol,
            bob: 0.0,
            recoil: 0.0,
            muzzle_flash: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HudData {
    pub health: i32,
    pub armor: i32,
    pub ammo: i32,
    pub ammo_reserve: i32,
    pub score: u32,
    pub kills: u32,
    pub total_kills: u32,
    pub level_name: String,
    pub has_key: bool,
}

impl Default for HudData {
    fn default() -> Self {
        Self {
            health: 100,
            armor: 0,
            ammo: 12,
            ammo_reserve: 48,
            score: 0,
            kills: 0,
            total_kills: 0,
            level_name: String::new(),
            has_key: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    Info,
    Pickup,
    Warning,
}

#[derive(Clone, Debug)]
pub struct RenderMessage {
    pub text: String,
    /// Remaining display time. Used for fade coloring.
    pub ttl: f32,
    pub kind: MessageKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Overlay {
    Title,
    Help,
    Paused,
    Dead,
    Victory,
    #[default]
    None,
}

/// A read-only visual snapshot assembled by the gameplay layer.
pub struct RenderScene<'a> {
    pub map: &'a Map,
    pub camera: Camera,
    pub entities: &'a [RenderEntity],
    pub pickups: &'a [RenderPickup],
    pub hud: &'a HudData,
    pub weapon: WeaponView,
    pub messages: &'a [RenderMessage],
    pub overlay: Overlay,
    pub show_minimap: bool,
    pub damage_flash: f32,
    pub elapsed: f32,
}

#[derive(Clone, Debug)]
pub struct Renderer {
    pub max_distance: f64,
}

impl Default for Renderer {
    fn default() -> Self {
        Self { max_distance: 32.0 }
    }
}

impl Renderer {
    pub fn render(&self, scene: &RenderScene<'_>, width: u16, height: u16) -> Frame {
        let mut frame = Frame::new(width, height);
        if width < MIN_WIDTH || height < MIN_HEIGHT {
            self.render_too_small(&mut frame);
            return frame;
        }

        let view_height = height - HUD_HEIGHT;
        self.render_background(&mut frame, view_height);
        let mut depth = self.render_walls(&mut frame, scene, view_height);
        self.render_billboards(&mut frame, scene, view_height, &mut depth);
        self.render_crosshair(&mut frame, view_height);
        self.render_weapon(&mut frame, scene.weapon, view_height);

        if scene.show_minimap {
            self.render_minimap(&mut frame, scene, view_height);
        }
        self.render_messages(&mut frame, scene.messages, view_height);
        self.render_damage_flash(&mut frame, scene.damage_flash, view_height);
        self.render_hud(&mut frame, scene.hud, view_height);
        self.render_overlay(&mut frame, scene.overlay, view_height);
        frame
    }

    fn render_too_small(&self, frame: &mut Frame) {
        frame.fill_rect(
            0,
            0,
            frame.width as i32,
            frame.height as i32,
            Cell::default(),
        );
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        let title = "TERMINAL TOO SMALL";
        let detail = format!("Need at least {MIN_WIDTH}x{MIN_HEIGHT}");
        frame.centered_text(
            frame.height as i32 / 2 - 1,
            title,
            Cell::new(' ', Color::Red, Color::Black).bold(),
        );
        frame.centered_text(
            frame.height as i32 / 2 + 1,
            &detail,
            Cell::new(' ', Color::DarkGrey, Color::Black),
        );
    }

    fn render_background(&self, frame: &mut Frame, view_height: u16) {
        let horizon = view_height as i32 / 2;
        for y in 0..view_height as i32 {
            let is_ceiling = y < horizon;
            let distance_from_horizon = (y - horizon).unsigned_abs() as usize;
            for x in 0..frame.width as i32 {
                let cell = if is_ceiling {
                    let sparkle = (x as usize * 17 + y as usize * 31).is_multiple_of(67)
                        && distance_from_horizon > 2;
                    Cell::new(
                        if sparkle { '.' } else { ' ' },
                        if sparkle {
                            Color::DarkGrey
                        } else {
                            Color::Black
                        },
                        Color::Rgb { r: 7, g: 7, b: 12 },
                    )
                } else {
                    let texture = (x as usize * 13 + y as usize * 7) % 23;
                    let ch = match texture {
                        0 => '.',
                        1 if distance_from_horizon > 4 => ',',
                        2 if distance_from_horizon > 8 => ':',
                        _ => ' ',
                    };
                    let shade = (20 + distance_from_horizon.min(16) as u8 * 2).min(50);
                    Cell::new(
                        ch,
                        Color::Rgb {
                            r: shade + 8,
                            g: shade,
                            b: shade.saturating_sub(8),
                        },
                        Color::Rgb {
                            r: shade / 3,
                            g: shade / 4,
                            b: shade / 5,
                        },
                    )
                };
                frame.set(x, y, cell);
            }
        }
    }

    fn render_walls(
        &self,
        frame: &mut Frame,
        scene: &RenderScene<'_>,
        view_height: u16,
    ) -> Vec<f64> {
        let width = frame.width as usize;
        let mut depth = vec![self.max_distance; width];
        let fov = scene.camera.fov.clamp(0.2, PI - 0.2);

        for (screen_x, depth_entry) in depth.iter_mut().enumerate() {
            let camera_x = (screen_x as f64 + 0.5) / width as f64 - 0.5;
            let angle_offset = camera_x * fov;
            let ray_angle = scene.camera.angle + angle_offset;
            let direction = Vec2 {
                x: ray_angle.cos(),
                y: ray_angle.sin(),
            };

            let Some(hit) = scene
                .map
                .cast_ray(scene.camera.position, direction, self.max_distance)
            else {
                continue;
            };
            let corrected = (hit.distance * angle_offset.cos()).max(0.001);
            *depth_entry = corrected;
            let projected = (view_height as f64 / corrected).round() as i32;
            let wall_height = projected.clamp(1, view_height as i32 * 3);
            let top = (view_height as i32 - wall_height) / 2;
            let bottom = top + wall_height;

            let along_wall = match hit.side {
                RaySide::Vertical => hit.point.y.fract().abs(),
                RaySide::Horizontal => hit.point.x.fract().abs(),
            };

            for y in top.max(0)..bottom.min(view_height as i32) {
                let wall_v = (y - top) as f64 / wall_height.max(1) as f64;
                let mortar = (along_wall * 8.0).fract() < 0.08 || (wall_v * 8.0).fract() < 0.055;
                let light = distance_light(corrected)
                    * if hit.side == RaySide::Horizontal {
                        0.74
                    } else {
                        1.0
                    }
                    * if mortar { 0.48 } else { 1.0 };
                // Quantized lighting keeps neighboring terminal cells on the
                // same ANSI style run. A per-map-cell tint retains texture
                // without flooding slower terminals with color escape codes.
                let quantized_light = (light * 8.0).round() / 8.0;
                let cell_tint = (((hit.cell.0 * 17 + hit.cell.1 * 31) % 3) as i8 - 1) * 3;
                let base = material_rgb(hit.material, hit.tile);
                let rgb = shade_rgb(base, quantized_light, cell_tint);
                let ch = wall_character(light, mortar, along_wall, wall_v);
                frame.set(
                    screen_x as i32,
                    y,
                    Cell::new(
                        ch,
                        Color::Rgb {
                            r: rgb.0,
                            g: rgb.1,
                            b: rgb.2,
                        },
                        Color::Black,
                    )
                    .bold(),
                );
            }
        }
        depth
    }

    fn render_billboards(
        &self,
        frame: &mut Frame,
        scene: &RenderScene<'_>,
        view_height: u16,
        depth: &mut [f64],
    ) {
        enum Billboard<'a> {
            Enemy(&'a RenderEntity),
            Pickup(&'a RenderPickup),
        }

        let mut billboards = Vec::<(f64, Billboard<'_>)>::new();
        for entity in scene.entities.iter().filter(|entity| entity.active) {
            billboards.push((
                squared_distance(scene.camera.position, entity.position),
                Billboard::Enemy(entity),
            ));
        }
        for pickup in scene.pickups.iter().filter(|pickup| pickup.active) {
            billboards.push((
                squared_distance(scene.camera.position, pickup.position),
                Billboard::Pickup(pickup),
            ));
        }
        billboards.sort_by(|a, b| b.0.total_cmp(&a.0));

        for (_, billboard) in billboards {
            match billboard {
                Billboard::Enemy(entity) => {
                    let (template, scale) = enemy_template(entity.visual);
                    self.draw_billboard(
                        frame,
                        scene.camera,
                        entity.position,
                        template,
                        scale,
                        view_height,
                        depth,
                        |ch| enemy_cell(ch, entity.visual, entity.hit_flash),
                    );
                }
                Billboard::Pickup(pickup) => {
                    let template = pickup_template(pickup.visual);
                    self.draw_billboard(
                        frame,
                        scene.camera,
                        pickup.position,
                        template,
                        0.48,
                        view_height,
                        depth,
                        |ch| pickup_cell(ch, pickup.visual),
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_billboard<F>(
        &self,
        frame: &mut Frame,
        camera: Camera,
        position: Vec2,
        template: &'static [&'static str],
        world_scale: f64,
        view_height: u16,
        depth: &mut [f64],
        color: F,
    ) where
        F: Fn(char) -> Cell,
    {
        let dx = position.x - camera.position.x;
        let dy = position.y - camera.position.y;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance < 0.08 {
            return;
        }
        let angle = normalize_angle(dy.atan2(dx) - camera.angle);
        let fov = camera.fov.clamp(0.2, PI - 0.2);
        if angle.abs() > fov * 0.72 {
            return;
        }
        let camera_depth = distance * angle.cos();
        if camera_depth <= 0.01 {
            return;
        }

        let source_height = template.len() as i32;
        let source_width = template
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(1) as i32;
        let sprite_height = ((view_height as f64 / camera_depth) * world_scale).round() as i32;
        let sprite_height = sprite_height.clamp(2, view_height as i32 * 2);
        // A terminal cell is typically about twice as tall as it is wide.
        let sprite_width =
            ((sprite_height as f64 * source_width as f64 / source_height.max(1) as f64) * 0.52)
                .round()
                .max(1.0) as i32;
        let center_x = (frame.width as f64 * (0.5 + angle / fov)).round() as i32;
        let bottom = view_height as i32 / 2 + sprite_height / 2;
        let left = center_x - sprite_width / 2;
        let top = bottom - sprite_height;

        for px in 0..sprite_width {
            let screen_x = left + px;
            if screen_x < 0 || screen_x >= frame.width as i32 {
                continue;
            }
            if camera_depth >= depth[screen_x as usize] {
                continue;
            }
            let source_x =
                ((2 * px + 1) * source_width / (2 * sprite_width)).clamp(0, source_width - 1);
            let source_x_start = (px * source_width / sprite_width).clamp(0, source_width - 1);
            let source_x_end = (((px + 1) * source_width + sprite_width - 1) / sprite_width)
                .clamp(source_x_start + 1, source_width);
            let mut drew_column = false;
            for py in 0..sprite_height {
                let screen_y = top + py;
                if screen_y < 0 || screen_y >= view_height as i32 {
                    continue;
                }
                let source_y = ((2 * py + 1) * source_height / (2 * sprite_height))
                    .clamp(0, source_height - 1);
                let ch = sample_scaled_glyph(
                    template[source_y as usize],
                    source_x,
                    source_x_start,
                    source_x_end,
                );
                if ch != ' ' {
                    frame.set(screen_x, screen_y, color(ch));
                    drew_column = true;
                }
            }
            if drew_column {
                depth[screen_x as usize] = camera_depth;
            }
        }
    }

    fn render_crosshair(&self, frame: &mut Frame, view_height: u16) {
        let x = frame.width as i32 / 2;
        let y = view_height as i32 / 2;
        let style = Cell::new('+', Color::Grey, Color::Black).bold();
        frame.set(x, y, style);
        if frame.width >= 90 {
            frame.set(x - 2, y, Cell { ch: '-', ..style });
            frame.set(x + 2, y, Cell { ch: '-', ..style });
        }
    }

    fn render_weapon(&self, frame: &mut Frame, weapon: WeaponView, view_height: u16) {
        let template: &[&str] = match weapon.visual {
            WeaponVisual::Pistol => &[
                "    __    ",
                "   |==|   ",
                "   |##|   ",
                "  _|##|_  ",
                " /######\\ ",
                " |##||##| ",
            ],
            WeaponVisual::Shotgun => &[
                "      ___________      ",
                " ____/===========\\____ ",
                "|#####################|",
                " \\____#########______/ ",
                "      |#######|        ",
                "      |##| |##|        ",
            ],
            WeaponVisual::Chaingun => &[
                "   ||  ||  ||   ",
                " __||__||__||__ ",
                "/==============\\",
                "|##############|",
                " \\____####____/ ",
                "      |##|      ",
            ],
        };
        let source_width = template
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(1) as i32;
        let bob_x = (weapon.bob.sin() * 2.0).round() as i32;
        let bob_y = (weapon.bob.cos().abs() * 1.2).round() as i32;
        let recoil_y = (weapon.recoil.clamp(0.0, 1.0) * 3.0).round() as i32;
        let left = frame.width as i32 / 2 - source_width / 2 + bob_x;
        let top = view_height as i32 - template.len() as i32 + bob_y + recoil_y;

        if weapon.muzzle_flash > 0.03 {
            let flash_y = top - 2;
            let flash_x = frame.width as i32 / 2 + bob_x;
            let outer = Cell::new('*', Color::Yellow, Color::DarkYellow).bold();
            frame.set(flash_x, flash_y, outer);
            frame.set(flash_x - 1, flash_y + 1, Cell { ch: '/', ..outer });
            frame.set(flash_x + 1, flash_y + 1, Cell { ch: '\\', ..outer });
            if weapon.muzzle_flash > 0.5 {
                frame.set(
                    flash_x,
                    flash_y - 1,
                    Cell::new('|', Color::White, Color::Yellow).bold(),
                );
                frame.set(
                    flash_x - 2,
                    flash_y,
                    Cell::new('-', Color::Yellow, Color::Black).bold(),
                );
                frame.set(
                    flash_x + 2,
                    flash_y,
                    Cell::new('-', Color::Yellow, Color::Black).bold(),
                );
            }
        }

        for (row, line) in template.iter().enumerate() {
            for (column, ch) in line.chars().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let cell = match ch {
                    '#' => Cell::new('▓', Color::DarkGrey, Color::Black).bold(),
                    '=' => Cell::new('═', Color::Grey, Color::Black).bold(),
                    _ => Cell::new(ch, Color::DarkGrey, Color::Black),
                };
                frame.set(left + column as i32, top + row as i32, cell);
            }
        }
    }

    fn render_minimap(&self, frame: &mut Frame, scene: &RenderScene<'_>, view_height: u16) {
        let map_width = scene.map.width.min(18);
        let map_height = scene.map.height.min(10);
        let panel_width = map_width as i32 + 2;
        let panel_height = map_height as i32 + 2;
        let panel_x = frame.width as i32 - panel_width - 1;
        let panel_y = 1;
        if panel_height >= view_height as i32 - 1 {
            return;
        }
        draw_panel(frame, panel_x, panel_y, panel_width, panel_height, " MAP ");

        let player_cell_x = scene.camera.position.x.floor() as isize;
        let player_cell_y = scene.camera.position.y.floor() as isize;
        let start_x = (player_cell_x - map_width as isize / 2)
            .clamp(0, scene.map.width.saturating_sub(map_width) as isize)
            as usize;
        let start_y = (player_cell_y - map_height as isize / 2)
            .clamp(0, scene.map.height.saturating_sub(map_height) as isize)
            as usize;

        for my in 0..map_height {
            for mx in 0..map_width {
                let world_x = start_x + mx;
                let world_y = start_y + my;
                let tile = scene.map.get(world_x, world_y).unwrap_or(Tile::Wall(0));
                let cell = match tile {
                    Tile::Wall(material) => {
                        Cell::new('#', rgb_color(material_rgb(material, tile)), Color::Black)
                    }
                    Tile::Door { open: false } => {
                        Cell::new('+', Color::DarkYellow, Color::Black).bold()
                    }
                    Tile::LockedDoor { open: false } => {
                        Cell::new('X', Color::Red, Color::Black).bold()
                    }
                    Tile::Exit => Cell::new('E', Color::Green, Color::Black).bold(),
                    _ => Cell::new('.', Color::DarkGrey, Color::Black),
                };
                frame.set(panel_x + 1 + mx as i32, panel_y + 1 + my as i32, cell);
            }
        }

        for pickup in scene.pickups.iter().filter(|pickup| pickup.active) {
            let x = pickup.position.x.floor() as isize - start_x as isize;
            let y = pickup.position.y.floor() as isize - start_y as isize;
            if x >= 0 && y >= 0 && x < map_width as isize && y < map_height as isize {
                frame.set(
                    panel_x + 1 + x as i32,
                    panel_y + 1 + y as i32,
                    Cell::new('*', Color::Cyan, Color::Black).bold(),
                );
            }
        }
        for enemy in scene.entities.iter().filter(|enemy| enemy.active) {
            let x = enemy.position.x.floor() as isize - start_x as isize;
            let y = enemy.position.y.floor() as isize - start_y as isize;
            if x >= 0 && y >= 0 && x < map_width as isize && y < map_height as isize {
                frame.set(
                    panel_x + 1 + x as i32,
                    panel_y + 1 + y as i32,
                    Cell::new('!', Color::Red, Color::Black).bold(),
                );
            }
        }
        let px = player_cell_x - start_x as isize;
        let py = player_cell_y - start_y as isize;
        if px >= 0 && py >= 0 && px < map_width as isize && py < map_height as isize {
            let arrow = direction_arrow(scene.camera.angle);
            frame.set(
                panel_x + 1 + px as i32,
                panel_y + 1 + py as i32,
                Cell::new(arrow, Color::White, Color::DarkRed).bold(),
            );
        }
    }

    fn render_messages(&self, frame: &mut Frame, messages: &[RenderMessage], view_height: u16) {
        let max_messages = ((view_height as usize) / 5).clamp(1, 4);
        for (row, message) in messages.iter().rev().take(max_messages).rev().enumerate() {
            let color = if message.ttl < 0.65 {
                Color::DarkGrey
            } else {
                match message.kind {
                    MessageKind::Info => Color::White,
                    MessageKind::Pickup => Color::Yellow,
                    MessageKind::Warning => Color::Red,
                }
            };
            let available = frame.width.saturating_sub(4) as usize;
            let text = truncate_chars(&message.text, available);
            frame.text(
                2,
                1 + row as i32,
                &text,
                Cell::new(' ', color, Color::Black).bold(),
            );
        }
    }

    fn render_damage_flash(&self, frame: &mut Frame, amount: f32, view_height: u16) {
        if amount <= 0.02 {
            return;
        }
        let intense = amount > 0.45;
        let cell = Cell::new(
            if intense { '▓' } else { '▒' },
            if intense { Color::Red } else { Color::DarkRed },
            Color::Black,
        );
        let thickness = if intense { 2 } else { 1 };
        for layer in 0..thickness {
            for x in layer..frame.width as i32 - layer {
                frame.set(x, layer, cell);
                frame.set(x, view_height as i32 - 1 - layer, cell);
            }
            for y in layer..view_height as i32 - layer {
                frame.set(layer, y, cell);
                frame.set(frame.width as i32 - 1 - layer, y, cell);
            }
        }
    }

    fn render_hud(&self, frame: &mut Frame, hud: &HudData, top: u16) {
        frame.fill_rect(
            0,
            top as i32,
            frame.width as i32,
            HUD_HEIGHT as i32,
            Cell::new(
                ' ',
                Color::White,
                Color::Rgb {
                    r: 20,
                    g: 20,
                    b: 20,
                },
            ),
        );
        for x in 0..frame.width as i32 {
            frame.set(
                x,
                top as i32,
                Cell::new('═', Color::DarkRed, Color::Black).bold(),
            );
        }

        let health_color = if hud.health <= 25 {
            Color::Red
        } else if hud.health <= 50 {
            Color::Yellow
        } else {
            Color::White
        };
        let first = format!(
            " HP {:>3}  ARM {:>3}  AMMO {:>3}/{:<3}",
            hud.health.max(0),
            hud.armor.max(0),
            hud.ammo.max(0),
            hud.ammo_reserve.max(0)
        );
        frame.text(
            0,
            top as i32 + 1,
            &truncate_chars(&first, frame.width as usize),
            Cell::new(
                ' ',
                health_color,
                Color::Rgb {
                    r: 20,
                    g: 20,
                    b: 20,
                },
            )
            .bold(),
        );

        let key = if hud.has_key { "  KEY [X]" } else { "" };
        let second = format!(
            " SCORE {:08}  KILLS {}/{}{}",
            hud.score, hud.kills, hud.total_kills, key
        );
        frame.text(
            0,
            top as i32 + 2,
            &truncate_chars(&second, frame.width as usize),
            Cell::new(
                ' ',
                Color::Grey,
                Color::Rgb {
                    r: 20,
                    g: 20,
                    b: 20,
                },
            ),
        );
        if !hud.level_name.is_empty() {
            let available = frame.width.saturating_sub(2) as usize;
            let level = truncate_chars(&hud.level_name, available);
            let level_x = frame.width as i32 - level.chars().count() as i32 - 1;
            frame.text(
                level_x.max(0),
                top as i32 + 2,
                &level,
                Cell::new(
                    ' ',
                    Color::DarkYellow,
                    Color::Rgb {
                        r: 20,
                        g: 20,
                        b: 20,
                    },
                )
                .bold(),
            );
        }

        let bar_width = frame.width.saturating_sub(2) as usize;
        let health_width =
            ((hud.health.clamp(0, 100) as f32 / 100.0) * bar_width as f32).round() as usize;
        for x in 0..bar_width {
            frame.set(
                1 + x as i32,
                top as i32 + 3,
                Cell::new(
                    if x < health_width { '▄' } else { '·' },
                    if x < health_width {
                        health_color
                    } else {
                        Color::DarkGrey
                    },
                    Color::Rgb {
                        r: 20,
                        g: 20,
                        b: 20,
                    },
                ),
            );
        }
    }

    fn render_overlay(&self, frame: &mut Frame, overlay: Overlay, view_height: u16) {
        match overlay {
            Overlay::None => {}
            Overlay::Title => self.render_title(frame, view_height),
            Overlay::Help => {
                let lines = [
                    "CONTROLS",
                    "",
                    "W/S      MOVE FORWARD / BACK",
                    "A/D      TURN",
                    "Z/C      STRAFE",
                    "ARROWS   MOVE / TURN",
                    "SPACE    FIRE",
                    "E        USE / OPEN / EXIT",
                    "1/2/3    SELECT WEAPON",
                    "M        TOGGLE MAP",
                    "H or ?   HELP",
                    "ESC      PAUSE",
                    "",
                    "PRESS H, ? OR ESC TO RETURN",
                ];
                centered_panel(frame, view_height, 38, &lines, Color::Yellow);
            }
            Overlay::Paused => {
                let lines = ["PAUSED", "", "ESC TO RESUME", "Q TO QUIT"];
                centered_panel(frame, view_height, 30, &lines, Color::Yellow);
            }
            Overlay::Dead => {
                let lines = [
                    "YOU DIED",
                    "",
                    "THE DEMONS FEAST TONIGHT",
                    "",
                    "R TO RESTART",
                ];
                centered_panel(frame, view_height, 36, &lines, Color::Red);
            }
            Overlay::Victory => {
                let lines = [
                    "VICTORY",
                    "",
                    "HELL HAS BEEN SILENCED",
                    "",
                    "ENTER TO CONTINUE",
                ];
                centered_panel(frame, view_height, 38, &lines, Color::Green);
            }
        }
    }

    fn render_title(&self, frame: &mut Frame, view_height: u16) {
        frame.fill_rect(
            0,
            0,
            frame.width as i32,
            view_height as i32,
            Cell::new(' ', Color::White, Color::Black),
        );
        let logo = [
            "██████╗  ██████╗  ██████╗ ███╗   ███╗",
            "██╔══██╗██╔═══██╗██╔═══██╗████╗ ████║",
            "██║  ██║██║   ██║██║   ██║██╔████╔██║",
            "██║  ██║██║   ██║██║   ██║██║╚██╔╝██║",
            "██████╔╝╚██████╔╝╚██████╔╝██║ ╚═╝ ██║",
            "╚═════╝  ╚═════╝  ╚═════╝ ╚═╝     ╚═╝",
        ];
        let start_y = (view_height as i32 / 2 - 6).max(1);
        for (row, line) in logo.iter().enumerate() {
            if line.chars().count() <= frame.width as usize {
                frame.centered_text(
                    start_y + row as i32,
                    line,
                    Cell::new(' ', Color::Red, Color::Black).bold(),
                );
            }
        }
        frame.centered_text(
            start_y + 8,
            "A FIRST-PERSON DESCENT INTO TERMINAL HELL",
            Cell::new(' ', Color::DarkYellow, Color::Black).bold(),
        );
        frame.centered_text(
            start_y + 11,
            "PRESS ENTER TO BEGIN",
            Cell::new(' ', Color::White, Color::Black).bold(),
        );
        frame.centered_text(
            start_y + 13,
            "H FOR HELP   Q TO QUIT",
            Cell::new(' ', Color::DarkGrey, Color::Black),
        );
    }
}

fn material_rgb(material: u8, tile: Tile) -> (u8, u8, u8) {
    match tile {
        Tile::Door { .. } => (150, 105, 42),
        Tile::LockedDoor { .. } => (145, 28, 28),
        _ => match material % 8 {
            0 => (128, 72, 52),
            1 => (142, 48, 42),
            2 => (102, 112, 92),
            3 => (116, 89, 58),
            4 => (86, 94, 120),
            5 => (126, 108, 78),
            6 => (94, 68, 94),
            _ => (120, 120, 112),
        },
    }
}

fn distance_light(distance: f64) -> f64 {
    (1.18 / (1.0 + distance * 0.11)).clamp(0.18, 1.0)
}

fn shade_rgb(base: (u8, u8, u8), factor: f64, noise: i8) -> (u8, u8, u8) {
    let channel =
        |value: u8| ((value as f64 * factor).round() as i16 + noise as i16).clamp(0, 255) as u8;
    (channel(base.0), channel(base.1), channel(base.2))
}

fn rgb_color(rgb: (u8, u8, u8)) -> Color {
    Color::Rgb {
        r: rgb.0,
        g: rgb.1,
        b: rgb.2,
    }
}

fn wall_character(light: f64, mortar: bool, u: f64, v: f64) -> char {
    if mortar {
        '░'
    } else if (u * 8.0).floor() as i32 % 2 == (v * 8.0).floor() as i32 % 2 {
        if light > 0.68 { '▓' } else { '▒' }
    } else if light > 0.48 {
        '█'
    } else {
        '▓'
    }
}

fn normalize_angle(mut angle: f64) -> f64 {
    while angle > PI {
        angle -= 2.0 * PI;
    }
    while angle < -PI {
        angle += 2.0 * PI;
    }
    angle
}

fn squared_distance(a: Vec2, b: Vec2) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

/// Select an opaque glyph from the source region represented by one scaled
/// terminal column. ASCII sprites contain intentional padding; sampling only
/// the region's center can otherwise make a distant one-column enemy vanish.
fn sample_scaled_glyph(row: &str, preferred: i32, start: i32, end: i32) -> char {
    let glyphs: Vec<char> = row.chars().collect();
    let glyph_at = |index: i32| glyphs.get(index as usize).copied().unwrap_or(' ');
    let preferred_glyph = glyph_at(preferred);
    if preferred_glyph != ' ' {
        return preferred_glyph;
    }

    (start..end)
        .filter_map(|index| {
            let glyph = glyph_at(index);
            (glyph != ' ').then_some(((index - preferred).abs(), glyph))
        })
        .min_by_key(|(distance, _)| *distance)
        .map_or(' ', |(_, glyph)| glyph)
}

fn direction_arrow(angle: f64) -> char {
    let normalized = normalize_angle(angle);
    if (-PI / 4.0..PI / 4.0).contains(&normalized) {
        '>'
    } else if (PI / 4.0..3.0 * PI / 4.0).contains(&normalized) {
        'v'
    } else if (-3.0 * PI / 4.0..-PI / 4.0).contains(&normalized) {
        '^'
    } else {
        '<'
    }
}

fn enemy_template(visual: EntityVisual) -> (&'static [&'static str], f64) {
    const TROOPER: &[&str] = &[
        "   ___   ",
        "  /o o\\  ",
        "  | ^ |  ",
        " /|===|\\ ",
        "/ |###| \\",
        "  /| |\\  ",
        " /_| |_\\ ",
    ];
    const IMP: &[&str] = &[
        "  \\^_^/  ",
        "  /o o\\  ",
        " /  ^  \\ ",
        "| /###\\ |",
        "|/#####\\|",
        "  /| |\\  ",
        " /_| |_\\ ",
    ];
    const DEMON: &[&str] = &[
        " /\\___/\\ ",
        "/ x   x \\",
        "|  ^^^  |",
        "| /###\\ |",
        " \\#####/ ",
        " /|#|#|\\ ",
        "/_|_|_|_\\",
    ];
    const CACODEMON: &[&str] = &[
        " .-^^^^^-. ",
        "/ O     O \\",
        "|    ^    |",
        "| \\vvvvv/ |",
        "\\  #####  /",
        " '-.___.-' ",
    ];
    const BOSS: &[&str] = &[
        " /\\_______/\\ ",
        "/ @       @ \\",
        "|  /^^^^^\\  |",
        "| |#######| |",
        "| |#######| |",
        " \\|#######|/ ",
        " /|##| |##|\\ ",
        "/_|##| |##|_\\",
    ];
    const BARREL: &[&str] = &[
        "  _____  ",
        " /=====\\ ",
        " |##!##| ",
        " |#####| ",
        " |##!##| ",
        " \\=====/ ",
    ];
    const FIREBALL: &[&str] = &[" .*. ", "*#@#*", " '*' "];
    match visual {
        EntityVisual::Trooper => (TROOPER, 0.9),
        EntityVisual::Imp => (IMP, 0.95),
        EntityVisual::Demon => (DEMON, 1.05),
        EntityVisual::Cacodemon => (CACODEMON, 1.0),
        EntityVisual::Boss => (BOSS, 1.45),
        EntityVisual::Barrel => (BARREL, 0.72),
        EntityVisual::Fireball => (FIREBALL, 0.32),
    }
}

fn enemy_cell(ch: char, visual: EntityVisual, hit_flash: f32) -> Cell {
    if hit_flash > 0.05 {
        return Cell::new(ch, Color::White, Color::DarkRed).bold();
    }
    let color = match ch {
        'o' | 'x' | '@' | 'O' => Color::Yellow,
        '#' => match visual {
            EntityVisual::Trooper => Color::DarkGreen,
            EntityVisual::Imp => Color::Red,
            EntityVisual::Demon => Color::DarkRed,
            EntityVisual::Cacodemon => Color::DarkMagenta,
            EntityVisual::Boss => Color::Red,
            EntityVisual::Barrel => Color::DarkYellow,
            EntityVisual::Fireball => Color::Yellow,
        },
        'v' | '^' => Color::White,
        '!' => Color::Yellow,
        _ => match visual {
            EntityVisual::Trooper => Color::Grey,
            EntityVisual::Imp => Color::DarkYellow,
            EntityVisual::Demon => Color::Red,
            EntityVisual::Cacodemon => Color::Magenta,
            EntityVisual::Boss => Color::DarkRed,
            EntityVisual::Barrel => Color::DarkYellow,
            EntityVisual::Fireball => Color::Red,
        },
    };
    Cell::new(
        match ch {
            '#' => '▓',
            _ => ch,
        },
        color,
        Color::Black,
    )
    .bold()
}

fn pickup_template(visual: PickupVisual) -> &'static [&'static str] {
    const BULLETS: &[&str] = &[" ||| ", "[===]", " ||| "];
    const SHELLS: &[&str] = &[" !!! ", "[###]", " !!! "];
    const MEDKIT: &[&str] = &[".---.", "| + |", "'---'"];
    const ARMOR: &[&str] = &[" /A\\ ", "/###\\", "\\___/"];
    const SHOTGUN: &[&str] = &[" _______ ", "========>", "   ||    "];
    const CHAINGUN: &[&str] = &[" ||| ", "[###]", " \\/  "];
    const KEY: &[&str] = &["  o  ", " /|==", "  |  "];
    match visual {
        PickupVisual::Bullets => BULLETS,
        PickupVisual::Shells => SHELLS,
        PickupVisual::Medkit => MEDKIT,
        PickupVisual::Armor => ARMOR,
        PickupVisual::Shotgun => SHOTGUN,
        PickupVisual::Chaingun => CHAINGUN,
        PickupVisual::Key => KEY,
    }
}

fn pickup_cell(ch: char, visual: PickupVisual) -> Cell {
    let color = match visual {
        PickupVisual::Bullets | PickupVisual::Shells => Color::Yellow,
        PickupVisual::Medkit => {
            if ch == '+' {
                Color::Red
            } else {
                Color::White
            }
        }
        PickupVisual::Armor => Color::Cyan,
        PickupVisual::Shotgun | PickupVisual::Chaingun => Color::Grey,
        PickupVisual::Key => Color::Yellow,
    };
    Cell::new(if ch == '#' { '▓' } else { ch }, color, Color::Black).bold()
}

fn draw_panel(frame: &mut Frame, x: i32, y: i32, width: i32, height: i32, title: &str) {
    frame.fill_rect(
        x,
        y,
        width,
        height,
        Cell::new(' ', Color::Grey, Color::Black),
    );
    for px in x..x + width {
        frame.set(px, y, Cell::new('─', Color::DarkRed, Color::Black));
        frame.set(
            px,
            y + height - 1,
            Cell::new('─', Color::DarkRed, Color::Black),
        );
    }
    for py in y..y + height {
        frame.set(x, py, Cell::new('│', Color::DarkRed, Color::Black));
        frame.set(
            x + width - 1,
            py,
            Cell::new('│', Color::DarkRed, Color::Black),
        );
    }
    frame.set(x, y, Cell::new('┌', Color::DarkRed, Color::Black));
    frame.set(
        x + width - 1,
        y,
        Cell::new('┐', Color::DarkRed, Color::Black),
    );
    frame.set(
        x,
        y + height - 1,
        Cell::new('└', Color::DarkRed, Color::Black),
    );
    frame.set(
        x + width - 1,
        y + height - 1,
        Cell::new('┘', Color::DarkRed, Color::Black),
    );
    frame.text(
        x + 2,
        y,
        title,
        Cell::new(' ', Color::Yellow, Color::Black).bold(),
    );
}

fn centered_panel(
    frame: &mut Frame,
    view_height: u16,
    desired_width: i32,
    lines: &[&str],
    accent: Color,
) {
    let width = desired_width.min(frame.width as i32 - 4).max(12);
    let height = lines.len() as i32 + 4;
    let x = (frame.width as i32 - width) / 2;
    let y = (view_height as i32 - height) / 2;
    draw_panel(frame, x, y, width, height, "");
    for (row, line) in lines.iter().enumerate() {
        let len = line.chars().count() as i32;
        let style = Cell::new(
            ' ',
            if row == 0 { accent } else { Color::White },
            Color::Black,
        )
        .bold();
        frame.text(x + (width - len) / 2, y + 2 + row as i32, line, style);
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    if max <= 3 {
        return text.chars().take(max).collect();
    }
    let mut result: String = text.chars().take(max - 3).collect();
    result.push_str("...");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_clips_writes() {
        let mut frame = Frame::new(3, 2);
        frame.set(-1, 0, Cell::new('X', Color::Red, Color::Black));
        frame.set(3, 0, Cell::new('X', Color::Red, Color::Black));
        frame.set(1, 1, Cell::new('Y', Color::Green, Color::Black));
        assert_eq!(frame.get(1, 1).unwrap().ch, 'Y');
        assert!(!frame.cells.iter().any(|cell| cell.ch == 'X'));
    }

    #[test]
    fn angle_normalization_stays_in_circle() {
        assert!((normalize_angle(PI * 3.0) - PI).abs() < 1e-10);
        assert!((normalize_angle(-PI * 3.0) + PI).abs() < 1e-10);
    }

    #[test]
    fn ansi_frame_contains_printable_content() {
        let mut frame = Frame::new(2, 1);
        frame.text(0, 0, "OK", Cell::new(' ', Color::Green, Color::Black));
        let output = String::from_utf8(frame.to_ansi_bytes().unwrap()).unwrap();
        assert!(output.contains("OK"));
    }

    #[test]
    fn presenter_does_not_repaint_an_unchanged_hud() {
        let mut frame = Frame::new(20, 4);
        frame.text(
            0,
            3,
            "HP 100  AMMO 72",
            Cell::new(' ', Color::White, Color::Black).bold(),
        );
        let mut presenter = FramePresenter::default();
        presenter.present(&frame, &mut Vec::new()).unwrap();

        let mut unchanged_update = Vec::new();
        presenter.present(&frame, &mut unchanged_update).unwrap();
        let output = String::from_utf8(unchanged_update).unwrap();
        assert!(output.starts_with("\u{1b}[?2026h"));
        assert!(output.ends_with("\u{1b}[?2026l"));
        assert!(!output.contains("HP 100"));
        assert!(
            output.len() < 32,
            "an unchanged frame should contain only synchronization/reset control bytes"
        );
    }

    #[test]
    fn truncation_counts_characters() {
        assert_eq!(truncate_chars("abcdefgh", 6), "abc...");
        assert_eq!(truncate_chars("abc", 6), "abc");
    }

    #[test]
    fn compass_mapping_matches_terminal_coordinates() {
        assert_eq!(direction_arrow(0.0), '>');
        assert_eq!(direction_arrow(std::f64::consts::FRAC_PI_2), 'v');
        assert_eq!(direction_arrow(-std::f64::consts::FRAC_PI_2), '^');
        assert_eq!(direction_arrow(PI), '<');
    }

    #[test]
    fn distant_billboard_preserves_visible_sprite_pixels() {
        let renderer = Renderer::default();
        let mut frame = Frame::new(80, 24);
        let mut depth = vec![renderer.max_distance; frame.width as usize];
        let template = &[
            "   ___   ",
            "  /o o\\  ",
            "  | ^ |  ",
            " /|===|\\ ",
            "/ |###| \\",
            "  /| |\\  ",
            " /_| |_\\ ",
        ];

        renderer.draw_billboard(
            &mut frame,
            Camera::new(Vec2::new(1.5, 1.5), 0.0),
            Vec2::new(11.5, 1.5),
            template,
            0.9,
            20,
            &mut depth,
            |ch| Cell::new(ch, Color::Magenta, Color::Black),
        );

        assert!(
            frame.cells.iter().any(|cell| cell.fg == Color::Magenta),
            "a distant one-column enemy should remain visible"
        );
    }
}
