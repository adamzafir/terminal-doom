use std::collections::HashMap;
use std::io::{self, Stdout, Write};
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, SetTitle, disable_raw_mode,
    enable_raw_mode,
};
use terminal_doom::game::{EnemyKind, Game, GameState, InputCommand, PickupKind, Weapon};
use terminal_doom::render::{
    Camera, EntityVisual, HudData, MessageKind, Overlay, PickupVisual, RenderEntity, RenderMessage,
    RenderPickup, RenderScene, Renderer, WeaponView, WeaponVisual,
};
use terminal_doom::world::SpawnKind;

const FRAME_TIME: Duration = Duration::from_millis(33);
const HELD_GRACE: Duration = Duration::from_millis(150);

struct TerminalGuard;

impl TerminalGuard {
    fn enter(stdout: &mut Stdout) -> io::Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(
            stdout,
            EnterAlternateScreen,
            Hide,
            Clear(ClearType::All),
            SetTitle("Terminal Doom")
        ) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            crossterm::style::ResetColor,
            Show,
            LeaveAlternateScreen
        );
    }
}

#[derive(Default)]
struct Frontend {
    started: bool,
    paused: bool,
    help: bool,
    minimap: bool,
    quit: bool,
    held: HashMap<KeyCode, Instant>,
    pending: Vec<InputCommand>,
}

impl Frontend {
    fn handle_key(&mut self, key: KeyEvent, game: &mut Game) {
        if key.kind == KeyEventKind::Release {
            self.held.remove(&key.code);
            return;
        }

        let first_press = key.kind == KeyEventKind::Press;
        match key.code {
            KeyCode::Char('q' | 'Q') => self.quit = true,
            KeyCode::Char('h' | 'H' | '?') if first_press => {
                self.help = !self.help;
                self.clear_held();
            }
            KeyCode::Esc if first_press => {
                if self.help {
                    self.help = false;
                } else if self.started && game.state == GameState::Playing {
                    self.paused = !self.paused;
                } else if !self.started {
                    self.quit = true;
                }
                self.clear_held();
            }
            KeyCode::Char('p' | 'P') if first_press && self.started => {
                self.paused = !self.paused;
                self.clear_held();
            }
            KeyCode::Enter if first_press => match game.state {
                GameState::Victory => {
                    *game = Game::new();
                    self.started = true;
                    self.paused = false;
                    self.help = false;
                }
                _ if !self.started => {
                    self.started = true;
                    self.help = false;
                }
                _ => {}
            },
            KeyCode::Char('m' | 'M') if first_press => self.minimap = !self.minimap,
            KeyCode::Char('r' | 'R') if first_press => {
                self.pending.push(InputCommand::Restart);
                self.started = true;
                self.paused = false;
                self.help = false;
            }
            KeyCode::Char('e' | 'E') if first_press => self.pending.push(InputCommand::Use),
            KeyCode::Char('1') if first_press => self
                .pending
                .push(InputCommand::SelectWeapon(Weapon::Pistol)),
            KeyCode::Char('2') if first_press => self
                .pending
                .push(InputCommand::SelectWeapon(Weapon::Shotgun)),
            KeyCode::Char('3') if first_press => self
                .pending
                .push(InputCommand::SelectWeapon(Weapon::Chaingun)),
            KeyCode::Tab if first_press => self.pending.push(InputCommand::NextWeapon),
            code if is_continuous_key(&code) => {
                self.held.insert(code, Instant::now() + HELD_GRACE);
            }
            _ => {}
        }
    }

    fn commands(&mut self, now: Instant) -> Vec<InputCommand> {
        self.held.retain(|_, until| *until >= now);
        let mut commands = std::mem::take(&mut self.pending);
        for code in self.held.keys() {
            let command = match code {
                KeyCode::Char('w' | 'W') | KeyCode::Up => InputCommand::MoveForward,
                KeyCode::Char('s' | 'S') | KeyCode::Down => InputCommand::MoveBackward,
                KeyCode::Char('z' | 'Z') => InputCommand::StrafeLeft,
                KeyCode::Char('c' | 'C') => InputCommand::StrafeRight,
                KeyCode::Char('a' | 'A' | 'j' | 'J') | KeyCode::Left => InputCommand::TurnLeft,
                KeyCode::Char('d' | 'D' | 'l' | 'L') | KeyCode::Right => InputCommand::TurnRight,
                KeyCode::Char(' ') => InputCommand::Fire,
                _ => continue,
            };
            commands.push(command);
        }
        commands
    }

    fn clear_held(&mut self) {
        self.held.clear();
        self.pending.clear();
    }

    fn overlay(&self, game: &Game) -> Overlay {
        if self.help {
            Overlay::Help
        } else if !self.started {
            Overlay::Title
        } else if self.paused {
            Overlay::Paused
        } else {
            match game.state {
                GameState::Playing => Overlay::None,
                GameState::Dead => Overlay::Dead,
                GameState::Victory => Overlay::Victory,
            }
        }
    }
}

fn is_continuous_key(code: &KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Char(
            'w' | 'W'
                | 'a'
                | 'A'
                | 's'
                | 'S'
                | 'd'
                | 'D'
                | 'z'
                | 'Z'
                | 'c'
                | 'C'
                | 'j'
                | 'J'
                | 'l'
                | 'L'
                | ' ',
        ) | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Left
            | KeyCode::Right
    )
}

struct Presentation {
    entities: Vec<RenderEntity>,
    pickups: Vec<RenderPickup>,
    messages: Vec<RenderMessage>,
    hud: HudData,
    weapon: WeaponView,
}

impl Presentation {
    fn from_game(game: &Game) -> Self {
        let mut entities: Vec<_> = game
            .enemies
            .iter()
            .map(|enemy| RenderEntity {
                position: enemy.pos,
                visual: match enemy.kind {
                    EnemyKind::Trooper => EntityVisual::Trooper,
                    EnemyKind::Imp => EntityVisual::Imp,
                    EnemyKind::Demon => EntityVisual::Demon,
                },
                health_fraction: enemy.health_fraction() as f32,
                hit_flash: enemy.hit_flash as f32,
                active: enemy.alive,
            })
            .collect();
        entities.extend(game.projectiles.iter().map(|projectile| RenderEntity {
            position: projectile.pos,
            visual: EntityVisual::Fireball,
            health_fraction: 1.0,
            hit_flash: 0.0,
            active: projectile.active,
        }));

        let pickups = game
            .pickups
            .iter()
            .map(|pickup| RenderPickup {
                position: pickup.pos,
                visual: match pickup.kind {
                    PickupKind::Ammo => PickupVisual::Bullets,
                    PickupKind::Medkit => PickupVisual::Medkit,
                    PickupKind::Armor => PickupVisual::Armor,
                    PickupKind::Key => PickupVisual::Key,
                },
                active: pickup.active,
            })
            .collect();

        let messages = game
            .messages
            .iter()
            .map(|message| RenderMessage {
                text: message.text.clone(),
                ttl: message.ttl as f32,
                kind: if message.text.starts_with("Picked up") {
                    MessageKind::Pickup
                } else if message.text.contains("required") || message.text.starts_with("No ammo") {
                    MessageKind::Warning
                } else {
                    MessageKind::Info
                },
            })
            .collect();

        let (ammo, ammo_reserve) = match game.player.weapon {
            Weapon::Pistol | Weapon::Chaingun => {
                (game.player.ammo.bullets, game.player.ammo.max_bullets)
            }
            Weapon::Shotgun => (game.player.ammo.shells, game.player.ammo.max_shells),
        };
        let total_kills = game
            .levels
            .iter()
            .flat_map(|level| &level.spawns)
            .filter(|spawn| matches!(spawn.kind, SpawnKind::Enemy(_)))
            .count() as u32;
        let hud = HudData {
            health: game.player.health,
            armor: game.player.armor,
            ammo,
            ammo_reserve,
            score: game.score,
            kills: game.kills,
            total_kills,
            level_name: format!(
                "{}  {}/{}",
                game.level_name(),
                game.level_number(),
                game.total_levels()
            ),
            has_key: game.player.keys > 0,
        };
        let visual = match game.player.weapon {
            Weapon::Pistol => WeaponVisual::Pistol,
            Weapon::Shotgun => WeaponVisual::Shotgun,
            Weapon::Chaingun => WeaponVisual::Chaingun,
        };
        let weapon = WeaponView {
            visual,
            bob: (game.elapsed as f32 * 7.0).sin() * 0.08,
            recoil: (game.weapon_flash as f32 * 7.0).clamp(0.0, 1.0),
            muzzle_flash: (game.weapon_flash as f32 * 12.0).clamp(0.0, 1.0),
        };

        Self {
            entities,
            pickups,
            messages,
            hud,
            weapon,
        }
    }
}

fn run(stdout: &mut Stdout) -> io::Result<()> {
    let _guard = TerminalGuard::enter(stdout)?;
    let renderer = Renderer::default();
    let mut game = Game::new();
    let mut frontend = Frontend::default();
    let mut previous_tick = Instant::now();

    loop {
        let frame_start = Instant::now();
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(key) => frontend.handle_key(key, &mut game),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        if frontend.quit {
            break;
        }

        let now = Instant::now();
        let dt = now.duration_since(previous_tick).as_secs_f64();
        previous_tick = now;
        let commands = frontend.commands(now);
        if frontend.started && !frontend.paused && !frontend.help {
            game.tick(dt, &commands);
        }

        let presentation = Presentation::from_game(&game);
        let (width, height) = terminal::size()?;
        let scene = RenderScene {
            map: game.current_map(),
            camera: Camera::new(game.player.pos, game.player.angle),
            entities: &presentation.entities,
            pickups: &presentation.pickups,
            hud: &presentation.hud,
            weapon: presentation.weapon,
            messages: &presentation.messages,
            overlay: frontend.overlay(&game),
            show_minimap: frontend.minimap,
            damage_flash: (game.damage_flash as f32 * 5.5).clamp(0.0, 1.0),
            elapsed: game.elapsed as f32,
        };
        renderer.render(&scene, width, height).queue_to(stdout)?;
        stdout.flush()?;

        let elapsed = frame_start.elapsed();
        if elapsed < FRAME_TIME {
            std::thread::sleep(FRAME_TIME - elapsed);
        }
    }
    Ok(())
}

fn main() {
    let mut args = std::env::args().skip(1);
    if let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => {
                println!(
                    "terminal-doom {}\n\nUSAGE:\n    doom\n\nCONTROLS:\n    W/S       Move forward/backward\n    A/D       Turn left/right\n    Z/C       Strafe left/right\n    Arrows    Alternate movement/turning\n    Space     Fire\n    E         Open doors/use exits\n    1/2/3     Select weapon\n    H         Help\n    Q         Quit",
                    env!("CARGO_PKG_VERSION")
                );
                return;
            }
            "-V" | "--version" => {
                println!("terminal-doom {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            _ => {
                eprintln!("doom: unknown option {argument:?}\nTry 'doom --help'.");
                std::process::exit(2);
            }
        }
    }

    let mut stdout = io::stdout();
    if let Err(error) = run(&mut stdout) {
        eprintln!("terminal-doom: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_frame_visibly_contains_an_enemy() {
        let game = Game::new();
        let presentation = Presentation::from_game(&game);
        assert!(
            !presentation.entities.is_empty(),
            "gameplay should provide enemies to the renderer"
        );

        let renderer = Renderer::default();
        let populated_scene = RenderScene {
            map: game.current_map(),
            camera: Camera::new(game.player.pos, game.player.angle),
            entities: &presentation.entities,
            pickups: &presentation.pickups,
            hud: &presentation.hud,
            weapon: presentation.weapon,
            messages: &presentation.messages,
            overlay: Overlay::None,
            show_minimap: false,
            damage_flash: 0.0,
            elapsed: game.elapsed as f32,
        };
        let populated = renderer.render(&populated_scene, 80, 28);

        let empty_entities = [];
        let empty_scene = RenderScene {
            entities: &empty_entities,
            ..populated_scene
        };
        let empty = renderer.render(&empty_scene, 80, 28);
        let changed_cells = (0..populated.height)
            .flat_map(|y| (0..populated.width).map(move |x| (x, y)))
            .filter(|&(x, y)| populated.get(x, y) != empty.get(x, y))
            .count();

        assert!(
            changed_cells > 0,
            "the opening enemy should change visible cells in the first-person frame"
        );
    }
}
