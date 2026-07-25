//! Gameplay simulation for Terminal Doom.
//!
//! This module deliberately knows nothing about terminals or key events.  A
//! frontend translates input into [`InputCommand`] values, calls [`Game::tick`],
//! and renders the public state.

use std::collections::VecDeque;
use std::f64::consts::{PI, TAU};

use crate::world::{InteractResult, Level, Map, SpawnKind, Vec2, builtin_levels};

const PLAYER_RADIUS: f64 = 0.22;
const PLAYER_SPEED: f64 = 2.8;
const TURN_SPEED: f64 = 2.25;
const PICKUP_RADIUS: f64 = 0.48;
const MAX_MESSAGES: usize = 6;

/// Coarse state of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    Playing,
    Dead,
    Victory,
}

/// Commands understood by the simulation. Multiple movement commands may be
/// supplied in one tick, which allows natural diagonal movement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputCommand {
    MoveForward,
    MoveBackward,
    StrafeLeft,
    StrafeRight,
    TurnLeft,
    TurnRight,
    /// Turn by an exact angle in radians. Useful for mouse-capable frontends.
    Turn(f64),
    Fire,
    Use,
    SelectWeapon(Weapon),
    NextWeapon,
    PreviousWeapon,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weapon {
    Pistol,
    Shotgun,
    Chaingun,
}

impl Weapon {
    pub const ALL: [Weapon; 3] = [Weapon::Pistol, Weapon::Shotgun, Weapon::Chaingun];

    pub fn name(self) -> &'static str {
        match self {
            Self::Pistol => "PISTOL",
            Self::Shotgun => "SHOTGUN",
            Self::Chaingun => "CHAINGUN",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Pistol => 0,
            Self::Shotgun => 1,
            Self::Chaingun => 2,
        }
    }

    fn cooldown(self) -> f64 {
        match self {
            Self::Pistol => 0.34,
            Self::Shotgun => 0.82,
            Self::Chaingun => 0.105,
        }
    }

    fn pellets(self) -> usize {
        match self {
            Self::Shotgun => 7,
            _ => 1,
        }
    }

    fn spread(self) -> f64 {
        match self {
            Self::Pistol => 0.025,
            Self::Shotgun => 0.15,
            Self::Chaingun => 0.055,
        }
    }

    fn damage(self) -> (i32, i32) {
        match self {
            Self::Pistol => (8, 14),
            Self::Shotgun => (6, 12), // per pellet
            Self::Chaingun => (6, 12),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ammo {
    pub bullets: i32,
    pub shells: i32,
    pub max_bullets: i32,
    pub max_shells: i32,
}

impl Default for Ammo {
    fn default() -> Self {
        Self {
            bullets: 72,
            shells: 18,
            max_bullets: 240,
            max_shells: 60,
        }
    }
}

impl Ammo {
    pub fn for_weapon(&self, weapon: Weapon) -> i32 {
        match weapon {
            Weapon::Pistol | Weapon::Chaingun => self.bullets,
            Weapon::Shotgun => self.shells,
        }
    }

    fn consume(&mut self, weapon: Weapon) -> bool {
        let stock = match weapon {
            Weapon::Pistol | Weapon::Chaingun => &mut self.bullets,
            Weapon::Shotgun => &mut self.shells,
        };
        if *stock <= 0 {
            false
        } else {
            *stock -= 1;
            true
        }
    }
}

#[derive(Debug, Clone)]
pub struct Player {
    pub pos: Vec2,
    pub angle: f64,
    pub health: i32,
    pub armor: i32,
    pub ammo: Ammo,
    pub weapon: Weapon,
    pub owned_weapons: [bool; 3],
    pub keys: u32,
    pub fire_cooldown: f64,
}

impl Player {
    pub fn new(pos: Vec2) -> Self {
        Self {
            pos,
            angle: 0.0,
            health: 100,
            armor: 0,
            ammo: Ammo::default(),
            weapon: Weapon::Pistol,
            // The maps focus on combat and do not need weapon-pickup placement.
            // Giving all three up front also makes weapon selection immediately
            // useful while ammo remains the limiting resource.
            owned_weapons: [true, true, true],
            keys: 0,
            fire_cooldown: 0.0,
        }
    }

    pub fn is_alive(&self) -> bool {
        self.health > 0
    }

    pub fn ammo_for_current_weapon(&self) -> i32 {
        self.ammo.for_weapon(self.weapon)
    }

    pub fn owns(&self, weapon: Weapon) -> bool {
        self.owned_weapons[weapon.index()]
    }

    /// Apply incoming damage and return the amount actually removed from health.
    pub fn take_damage(&mut self, damage: i32) -> i32 {
        if damage <= 0 || self.health <= 0 {
            return 0;
        }
        // Armor soaks one third of incoming damage until depleted.
        let absorbed = self.armor.min((damage + 2) / 3);
        self.armor -= absorbed;
        let health_damage = damage - absorbed;
        let old_health = self.health;
        self.health = (self.health - health_damage).max(0);
        old_health - self.health
    }

    /// Apply a pickup. Returns false when it would have no effect.
    pub fn collect(&mut self, pickup: PickupKind) -> bool {
        match pickup {
            PickupKind::Ammo => {
                if self.ammo.bullets >= self.ammo.max_bullets
                    && self.ammo.shells >= self.ammo.max_shells
                {
                    return false;
                }
                self.ammo.bullets = (self.ammo.bullets + 28).min(self.ammo.max_bullets);
                self.ammo.shells = (self.ammo.shells + 6).min(self.ammo.max_shells);
            }
            PickupKind::Medkit => {
                if self.health >= 100 {
                    return false;
                }
                self.health = (self.health + 30).min(100);
            }
            PickupKind::Armor => {
                if self.armor >= 100 {
                    return false;
                }
                self.armor = (self.armor + 40).min(100);
            }
            PickupKind::Key => {
                self.keys += 1;
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyKind {
    Trooper,
    Imp,
    Demon,
}

impl EnemyKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Trooper => "possessed trooper",
            Self::Imp => "imp",
            Self::Demon => "demon",
        }
    }

    fn from_spawn_id(id: u8) -> Self {
        match id % 3 {
            0 => Self::Trooper,
            1 => Self::Imp,
            _ => Self::Demon,
        }
    }

    fn max_health(self) -> i32 {
        match self {
            Self::Trooper => 32,
            Self::Imp => 58,
            Self::Demon => 115,
        }
    }

    fn radius(self) -> f64 {
        match self {
            Self::Trooper => 0.23,
            Self::Imp => 0.26,
            Self::Demon => 0.32,
        }
    }

    fn speed(self) -> f64 {
        match self {
            Self::Trooper => 0.85,
            Self::Imp => 0.72,
            Self::Demon => 1.18,
        }
    }

    fn score(self) -> u32 {
        match self {
            Self::Trooper => 100,
            Self::Imp => 250,
            Self::Demon => 500,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyState {
    Idle,
    Chasing,
    Attacking,
    Hurt,
    Dead,
}

#[derive(Debug, Clone)]
pub struct Enemy {
    pub pos: Vec2,
    pub kind: EnemyKind,
    pub health: i32,
    pub max_health: i32,
    pub state: EnemyState,
    pub facing: f64,
    pub alive: bool,
    pub attack_cooldown: f64,
    pub pain_timer: f64,
    pub hit_flash: f64,
}

impl Enemy {
    fn new(pos: Vec2, kind: EnemyKind) -> Self {
        let health = kind.max_health();
        Self {
            pos,
            kind,
            health,
            max_health: health,
            state: EnemyState::Idle,
            facing: 0.0,
            alive: true,
            attack_cooldown: 0.35,
            pain_timer: 0.0,
            hit_flash: 0.0,
        }
    }

    pub fn health_fraction(&self) -> f64 {
        (self.health.max(0) as f64 / self.max_health as f64).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickupKind {
    Ammo,
    Medkit,
    Armor,
    Key,
}

impl PickupKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ammo => "ammunition",
            Self::Medkit => "medkit",
            Self::Armor => "combat armor",
            Self::Key => "keycard",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Pickup {
    pub pos: Vec2,
    pub kind: PickupKind,
    pub active: bool,
    pub phase: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectileKind {
    Fireball,
}

#[derive(Debug, Clone)]
pub struct Projectile {
    pub pos: Vec2,
    pub velocity: Vec2,
    pub kind: ProjectileKind,
    pub damage: i32,
    pub active: bool,
    pub ttl: f64,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub text: String,
    pub ttl: f64,
}

/// Complete mutable state of a run.
#[derive(Debug, Clone)]
pub struct Game {
    /// Runtime levels. Door tiles are changed directly in these maps.
    pub levels: Vec<Level>,
    pub level_index: usize,
    pub player: Player,
    pub enemies: Vec<Enemy>,
    pub pickups: Vec<Pickup>,
    pub projectiles: Vec<Projectile>,
    pub state: GameState,
    pub score: u32,
    pub kills: u32,
    pub messages: VecDeque<Message>,
    pub elapsed: f64,
    pub weapon_flash: f64,
    pub damage_flash: f64,
    pub screen_shake: f64,
    level_templates: Vec<Level>,
    rng_state: u64,
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Game {
    pub fn new() -> Self {
        let levels = builtin_levels();
        assert!(
            !levels.is_empty(),
            "Terminal Doom requires at least one level"
        );
        let templates = levels.clone();
        let start = levels[0].player_start;
        let mut game = Self {
            levels,
            level_index: 0,
            player: Player::new(start),
            enemies: Vec::new(),
            pickups: Vec::new(),
            projectiles: Vec::new(),
            state: GameState::Playing,
            score: 0,
            kills: 0,
            messages: VecDeque::new(),
            elapsed: 0.0,
            weapon_flash: 0.0,
            damage_flash: 0.0,
            screen_shake: 0.0,
            level_templates: templates,
            rng_state: 0x4d59_5df4_d0f3_3173,
        };
        game.populate_level();
        game.face_open_space();
        game.push_message(format!("Entering {}", game.level_name()), 3.5);
        game
    }

    pub fn current_level(&self) -> &Level {
        &self.levels[self.level_index]
    }

    pub fn current_map(&self) -> &Map {
        &self.current_level().map
    }

    pub fn level_name(&self) -> &'static str {
        self.current_level().name
    }

    pub fn level_number(&self) -> usize {
        self.level_index + 1
    }

    pub fn total_levels(&self) -> usize {
        self.levels.len()
    }

    pub fn living_enemies(&self) -> usize {
        self.enemies.iter().filter(|enemy| enemy.alive).count()
    }

    /// Restart the current level with a fresh player and unmodified doors.
    pub fn restart(&mut self) {
        self.levels[self.level_index] = self.level_templates[self.level_index].clone();
        self.player = Player::new(self.current_level().player_start);
        self.state = GameState::Playing;
        self.score = 0;
        self.kills = 0;
        self.elapsed = 0.0;
        self.weapon_flash = 0.0;
        self.damage_flash = 0.0;
        self.screen_shake = 0.0;
        self.messages.clear();
        self.populate_level();
        self.face_open_space();
        self.push_message(format!("Restarting {}", self.level_name()), 3.0);
    }

    /// Advance to the next map, preserving combat resources. Returns false and
    /// enters victory state when the current map is the final one.
    pub fn next_level(&mut self) -> bool {
        if self.level_index + 1 >= self.levels.len() {
            self.state = GameState::Victory;
            self.projectiles.clear();
            self.push_message("HELL HAS BEEN CONQUERED".to_owned(), 10.0);
            return false;
        }

        self.level_index += 1;
        self.levels[self.level_index] = self.level_templates[self.level_index].clone();
        self.player.pos = self.current_level().player_start;
        self.player.angle = 0.0;
        self.player.keys = 0;
        self.player.fire_cooldown = 0.0;
        // A small intermission reward prevents attrition from soft-locking a run.
        self.player.health = (self.player.health + 20).min(100);
        self.player.ammo.bullets =
            (self.player.ammo.bullets + 24).min(self.player.ammo.max_bullets);
        self.player.ammo.shells = (self.player.ammo.shells + 6).min(self.player.ammo.max_shells);
        self.state = GameState::Playing;
        self.weapon_flash = 0.0;
        self.damage_flash = 0.0;
        self.populate_level();
        self.face_open_space();
        self.push_message(format!("Entering {}", self.level_name()), 3.5);
        true
    }

    /// Advance the simulation by `dt` seconds.
    ///
    /// Frontends should use a fixed or capped timestep. This method also caps
    /// unusually large deltas so pausing a debugger cannot teleport actors.
    pub fn tick(&mut self, dt: f64, commands: &[InputCommand]) {
        let dt = dt.clamp(0.0, 0.1);

        if commands.contains(&InputCommand::Restart) {
            self.restart();
            return;
        }
        if self.state != GameState::Playing {
            self.update_cosmetic_timers(dt);
            return;
        }

        self.elapsed += dt;
        self.update_cosmetic_timers(dt);
        self.player.fire_cooldown = (self.player.fire_cooldown - dt).max(0.0);

        let mut move_forward = 0.0;
        let mut move_sideways = 0.0;
        let mut turn = 0.0;
        let mut fire = false;
        let mut use_action = false;

        for command in commands {
            match *command {
                InputCommand::MoveForward => move_forward += 1.0,
                InputCommand::MoveBackward => move_forward -= 1.0,
                InputCommand::StrafeLeft => move_sideways -= 1.0,
                InputCommand::StrafeRight => move_sideways += 1.0,
                InputCommand::TurnLeft => turn -= TURN_SPEED * dt,
                InputCommand::TurnRight => turn += TURN_SPEED * dt,
                InputCommand::Turn(radians) => turn += radians,
                InputCommand::Fire => fire = true,
                InputCommand::Use => use_action = true,
                InputCommand::SelectWeapon(weapon) => self.select_weapon(weapon),
                InputCommand::NextWeapon => self.cycle_weapon(1),
                InputCommand::PreviousWeapon => self.cycle_weapon(-1),
                InputCommand::Restart => {}
            }
        }

        self.player.angle = wrap_angle(self.player.angle + turn);
        self.move_player(move_forward, move_sideways, dt);

        if use_action && self.use_world() {
            // Exiting swaps out all entity arrays, so do no more work this tick.
            return;
        }
        if fire {
            self.fire_weapon();
        }

        self.update_projectiles(dt);
        if self.state != GameState::Playing {
            return;
        }
        self.update_enemies(dt);
        if self.state != GameState::Playing {
            return;
        }
        self.collect_nearby_pickups();
        self.pickups.iter_mut().for_each(|p| p.phase += dt);
    }

    fn populate_level(&mut self) {
        self.enemies.clear();
        self.pickups.clear();
        self.projectiles.clear();
        let spawns = self.current_level().spawns.clone();
        for spawn in spawns {
            match spawn.kind {
                SpawnKind::Enemy(id) => self
                    .enemies
                    .push(Enemy::new(spawn.position, EnemyKind::from_spawn_id(id))),
                SpawnKind::Ammo => {
                    let phase = self.random_f64() * TAU;
                    self.pickups.push(Pickup {
                        pos: spawn.position,
                        kind: PickupKind::Ammo,
                        active: true,
                        phase,
                    });
                }
                SpawnKind::Medkit => {
                    let phase = self.random_f64() * TAU;
                    self.pickups.push(Pickup {
                        pos: spawn.position,
                        kind: PickupKind::Medkit,
                        active: true,
                        phase,
                    });
                }
                SpawnKind::Armor => {
                    let phase = self.random_f64() * TAU;
                    self.pickups.push(Pickup {
                        pos: spawn.position,
                        kind: PickupKind::Armor,
                        active: true,
                        phase,
                    });
                }
                SpawnKind::Key => {
                    let phase = self.random_f64() * TAU;
                    self.pickups.push(Pickup {
                        pos: spawn.position,
                        kind: PickupKind::Key,
                        active: true,
                        phase,
                    });
                }
            }
        }
    }

    fn face_open_space(&mut self) {
        // Prefer a direction with an unobstructed point a short distance ahead.
        // This keeps spawn points from initially staring into a wall without
        // depending on renderer-specific ray-hit details.
        let directions = [0.0, PI * 0.5, PI, PI * 1.5];
        if let Some(angle) = directions.into_iter().find(|angle| {
            let probe = self.player.pos + Vec2::from_angle(*angle) * 0.8;
            self.current_map().is_walkable(probe, PLAYER_RADIUS)
        }) {
            self.player.angle = angle;
        }
    }

    fn update_cosmetic_timers(&mut self, dt: f64) {
        self.weapon_flash = (self.weapon_flash - dt).max(0.0);
        self.damage_flash = (self.damage_flash - dt).max(0.0);
        self.screen_shake = (self.screen_shake - dt).max(0.0);
        for enemy in &mut self.enemies {
            enemy.hit_flash = (enemy.hit_flash - dt).max(0.0);
            enemy.pain_timer = (enemy.pain_timer - dt).max(0.0);
        }
        for message in &mut self.messages {
            message.ttl -= dt;
        }
        while self
            .messages
            .front()
            .is_some_and(|message| message.ttl <= 0.0)
        {
            self.messages.pop_front();
        }
    }

    fn move_player(&mut self, forward_amount: f64, sideways_amount: f64, dt: f64) {
        let forward = Vec2::from_angle(self.player.angle);
        let right = Vec2::new(-forward.y, forward.x);
        let mut movement = forward * forward_amount + right * sideways_amount;
        if movement.length() > 1.0 {
            movement = movement.normalized();
        }
        movement = movement * (PLAYER_SPEED * dt);

        // Sliding each axis independently feels much less sticky in corridors.
        let candidate_x = Vec2::new(self.player.pos.x + movement.x, self.player.pos.y);
        if self.current_map().is_walkable(candidate_x, PLAYER_RADIUS) {
            self.player.pos.x = candidate_x.x;
        }
        let candidate_y = Vec2::new(self.player.pos.x, self.player.pos.y + movement.y);
        if self.current_map().is_walkable(candidate_y, PLAYER_RADIUS) {
            self.player.pos.y = candidate_y.y;
        }
    }

    /// Returns true if interaction caused a level transition.
    fn use_world(&mut self) -> bool {
        let direction = Vec2::from_angle(self.player.angle);
        let result = {
            let has_key = self.player.keys > 0;
            self.levels[self.level_index]
                .map
                .interact(self.player.pos, direction, 1.25, has_key)
        };
        match result {
            InteractResult::Nothing => {}
            InteractResult::OpenedDoor => self.push_message("Door opened".to_owned(), 1.4),
            InteractResult::ClosedDoor => self.push_message("Door closed".to_owned(), 1.4),
            InteractResult::Locked => self.push_message("A keycard is required".to_owned(), 2.2),
            InteractResult::UnlockedDoor => {
                self.player.keys = self.player.keys.saturating_sub(1);
                self.push_message("Locked door opened".to_owned(), 2.2);
            }
            InteractResult::ClosedLockedDoor => {
                self.push_message("Security door closed".to_owned(), 1.4)
            }
            InteractResult::Exit => {
                self.next_level();
                return true;
            }
        }
        false
    }

    fn select_weapon(&mut self, weapon: Weapon) {
        if self.player.owns(weapon) {
            self.player.weapon = weapon;
        }
    }

    fn cycle_weapon(&mut self, direction: i32) {
        let current = self.player.weapon.index() as i32;
        for offset in 1..=Weapon::ALL.len() as i32 {
            let index = (current + direction * offset).rem_euclid(Weapon::ALL.len() as i32);
            let weapon = Weapon::ALL[index as usize];
            if self.player.owns(weapon) {
                self.player.weapon = weapon;
                return;
            }
        }
    }

    fn fire_weapon(&mut self) {
        if self.player.fire_cooldown > 0.0 {
            return;
        }
        let weapon = self.player.weapon;
        if !self.player.ammo.consume(weapon) {
            self.push_message(format!("No ammo for {}", weapon.name()), 1.5);
            self.player.fire_cooldown = 0.2;
            return;
        }

        self.player.fire_cooldown = weapon.cooldown();
        self.weapon_flash = match weapon {
            Weapon::Shotgun => 0.16,
            _ => 0.08,
        };
        self.screen_shake = match weapon {
            Weapon::Shotgun => 0.18,
            Weapon::Chaingun => 0.06,
            Weapon::Pistol => 0.04,
        };

        for _ in 0..weapon.pellets() {
            let spread = (self.random_f64() * 2.0 - 1.0) * weapon.spread();
            let ray_angle = self.player.angle + spread;
            if let Some(index) = self.enemy_along_ray(ray_angle, 18.0) {
                let (minimum, maximum) = weapon.damage();
                let damage = self.random_i32(minimum, maximum);
                self.damage_enemy(index, damage);
            }
        }
    }

    fn enemy_along_ray(&self, ray_angle: f64, max_distance: f64) -> Option<usize> {
        let direction = Vec2::from_angle(ray_angle);
        let mut best: Option<(usize, f64)> = None;
        for (index, enemy) in self.enemies.iter().enumerate() {
            if !enemy.alive {
                continue;
            }
            let delta = enemy.pos - self.player.pos;
            let along = delta.dot(direction);
            if along <= 0.0 || along > max_distance {
                continue;
            }
            let perpendicular = (delta - direction * along).length();
            if perpendicular > enemy.kind.radius() {
                continue;
            }
            if !self
                .current_map()
                .has_line_of_sight(self.player.pos, enemy.pos)
            {
                continue;
            }
            if best.is_none_or(|(_, previous_distance)| along < previous_distance) {
                best = Some((index, along));
            }
        }
        best.map(|(index, _)| index)
    }

    fn damage_enemy(&mut self, index: usize, damage: i32) {
        let mut killed = None;
        if let Some(enemy) = self.enemies.get_mut(index) {
            if !enemy.alive {
                return;
            }
            enemy.health -= damage;
            enemy.hit_flash = 0.10;
            enemy.pain_timer = 0.12;
            enemy.state = EnemyState::Hurt;
            if enemy.health <= 0 {
                enemy.health = 0;
                enemy.alive = false;
                enemy.state = EnemyState::Dead;
                killed = Some(enemy.kind);
            }
        }
        if let Some(kind) = killed {
            self.kills += 1;
            self.score += kind.score();
            self.push_message(format!("Killed {}", kind.name()), 1.35);
        }
    }

    fn update_enemies(&mut self, dt: f64) {
        enum Attack {
            Hitscan { damage: i32 },
            Fireball { origin: Vec2, direction: Vec2 },
            Bite { damage: i32 },
        }

        let player_pos = self.player.pos;
        let mut attacks = Vec::new();

        for index in 0..self.enemies.len() {
            if !self.enemies[index].alive {
                continue;
            }

            self.enemies[index].attack_cooldown =
                (self.enemies[index].attack_cooldown - dt).max(0.0);
            if self.enemies[index].pain_timer > 0.0 {
                self.enemies[index].state = EnemyState::Hurt;
                continue;
            }

            let enemy_pos = self.enemies[index].pos;
            let kind = self.enemies[index].kind;
            let delta = player_pos - enemy_pos;
            let distance = delta.length();
            let direction = if distance > 0.0001 {
                delta / distance
            } else {
                Vec2::new(1.0, 0.0)
            };
            self.enemies[index].facing = direction.y.atan2(direction.x);

            let can_see =
                distance < 13.0 && self.current_map().has_line_of_sight(enemy_pos, player_pos);
            if !can_see && self.enemies[index].state == EnemyState::Idle {
                continue;
            }

            let attack_range = match kind {
                EnemyKind::Trooper => 7.5,
                EnemyKind::Imp => 8.5,
                EnemyKind::Demon => 0.78,
            };
            if can_see && distance <= attack_range && self.enemies[index].attack_cooldown <= 0.0 {
                self.enemies[index].state = EnemyState::Attacking;
                let next_attack = match kind {
                    EnemyKind::Trooper => 1.0 + self.random_f64() * 0.5,
                    EnemyKind::Imp => 1.35 + self.random_f64() * 0.5,
                    EnemyKind::Demon => 0.82 + self.random_f64() * 0.3,
                };
                self.enemies[index].attack_cooldown = next_attack;
                match kind {
                    EnemyKind::Trooper => {
                        // Accuracy falls slightly with distance.
                        let hit_chance: f64 = (0.78_f64 - distance * 0.035_f64).clamp(0.32, 0.75);
                        if self.random_f64() < hit_chance {
                            attacks.push(Attack::Hitscan {
                                damage: self.random_i32(4, 11),
                            });
                        }
                    }
                    EnemyKind::Imp => attacks.push(Attack::Fireball {
                        origin: enemy_pos + direction * 0.32,
                        direction,
                    }),
                    EnemyKind::Demon => attacks.push(Attack::Bite {
                        damage: self.random_i32(10, 21),
                    }),
                }
                continue;
            }

            self.enemies[index].state = EnemyState::Chasing;
            if distance <= kind.radius() + PLAYER_RADIUS + 0.08 {
                continue;
            }
            let movement = direction * (kind.speed() * dt);
            let radius = kind.radius();
            let old = self.enemies[index].pos;
            let candidate_x = Vec2::new(old.x + movement.x, old.y);
            if self.current_map().is_walkable(candidate_x, radius) {
                self.enemies[index].pos.x = candidate_x.x;
            }
            let now = self.enemies[index].pos;
            let candidate_y = Vec2::new(now.x, now.y + movement.y);
            if self.current_map().is_walkable(candidate_y, radius) {
                self.enemies[index].pos.y = candidate_y.y;
            }
        }

        for attack in attacks {
            match attack {
                Attack::Hitscan { damage } | Attack::Bite { damage } => self.hurt_player(damage),
                Attack::Fireball { origin, direction } => {
                    self.projectiles.push(Projectile {
                        pos: origin,
                        velocity: direction * 2.7,
                        kind: ProjectileKind::Fireball,
                        damage: 16,
                        active: true,
                        ttl: 5.0,
                    });
                }
            }
        }
    }

    fn update_projectiles(&mut self, dt: f64) {
        for index in 0..self.projectiles.len() {
            if !self.projectiles[index].active {
                continue;
            }
            self.projectiles[index].ttl -= dt;
            if self.projectiles[index].ttl <= 0.0 {
                self.projectiles[index].active = false;
                continue;
            }

            // Substeps keep fast fireballs from passing through thin walls.
            let velocity = self.projectiles[index].velocity;
            let distance = velocity.length() * dt;
            let steps = ((distance / 0.12).ceil() as usize).max(1);
            let step = velocity * (dt / steps as f64);
            for _ in 0..steps {
                let candidate = self.projectiles[index].pos + step;
                if !self.current_map().is_walkable(candidate, 0.09) {
                    self.projectiles[index].active = false;
                    break;
                }
                self.projectiles[index].pos = candidate;
                if candidate.distance(self.player.pos) < PLAYER_RADIUS + 0.11 {
                    let damage = self.projectiles[index].damage;
                    self.projectiles[index].active = false;
                    self.hurt_player(damage);
                    break;
                }
            }
        }
        self.projectiles.retain(|projectile| projectile.active);
    }

    fn hurt_player(&mut self, damage: i32) {
        if self.state != GameState::Playing {
            return;
        }
        let health_damage = self.player.take_damage(damage);
        if health_damage > 0 {
            self.damage_flash = 0.18;
            self.screen_shake = self.screen_shake.max(0.12);
        }
        if !self.player.is_alive() {
            self.state = GameState::Dead;
            self.push_message("YOU DIED — press R to restart".to_owned(), 30.0);
        }
    }

    fn collect_nearby_pickups(&mut self) {
        for index in 0..self.pickups.len() {
            if !self.pickups[index].active
                || self.pickups[index].pos.distance(self.player.pos) > PICKUP_RADIUS
            {
                continue;
            }
            let kind = self.pickups[index].kind;
            if self.player.collect(kind) {
                self.pickups[index].active = false;
                self.score += match kind {
                    PickupKind::Key => 150,
                    PickupKind::Medkit | PickupKind::Armor => 50,
                    PickupKind::Ammo => 25,
                };
                self.push_message(format!("Picked up {}", kind.name()), 1.6);
            }
        }
    }

    fn push_message(&mut self, text: String, ttl: f64) {
        if self.messages.len() == MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.messages.push_back(Message { text, ttl });
    }

    fn random_u64(&mut self) -> u64 {
        // xorshift64*: tiny, deterministic, and more than adequate for combat
        // spread/damage. Determinism makes gameplay bugs reproducible.
        let mut x = self.rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn random_f64(&mut self) -> f64 {
        (self.random_u64() >> 11) as f64 * (1.0 / ((1_u64 << 53) as f64))
    }

    fn random_i32(&mut self, minimum: i32, maximum: i32) -> i32 {
        minimum + (self.random_u64() % (maximum - minimum + 1) as u64) as i32
    }
}

fn wrap_angle(angle: f64) -> f64 {
    angle.rem_euclid(TAU)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armor_absorbs_part_of_damage() {
        let mut player = Player::new(Vec2::new(1.0, 1.0));
        player.armor = 20;
        let health_damage = player.take_damage(30);
        assert_eq!(health_damage, 20);
        assert_eq!(player.health, 80);
        assert_eq!(player.armor, 10);
    }

    #[test]
    fn lethal_damage_stops_at_zero() {
        let mut player = Player::new(Vec2::new(1.0, 1.0));
        assert_eq!(player.take_damage(500), 100);
        assert_eq!(player.health, 0);
        assert!(!player.is_alive());
        assert_eq!(player.take_damage(10), 0);
    }

    #[test]
    fn pickups_respect_caps_and_reject_waste() {
        let mut player = Player::new(Vec2::new(0.0, 0.0));
        assert!(!player.collect(PickupKind::Medkit));
        player.health = 83;
        assert!(player.collect(PickupKind::Medkit));
        assert_eq!(player.health, 100);

        player.ammo.bullets = player.ammo.max_bullets;
        player.ammo.shells = player.ammo.max_shells;
        assert!(!player.collect(PickupKind::Ammo));
        player.ammo.shells -= 1;
        assert!(player.collect(PickupKind::Ammo));
        assert_eq!(player.ammo.shells, player.ammo.max_shells);
    }

    #[test]
    fn weapons_consume_the_correct_ammo_pool() {
        let mut ammo = Ammo::default();
        let bullets = ammo.bullets;
        let shells = ammo.shells;
        assert!(ammo.consume(Weapon::Pistol));
        assert!(ammo.consume(Weapon::Chaingun));
        assert!(ammo.consume(Weapon::Shotgun));
        assert_eq!(ammo.bullets, bullets - 2);
        assert_eq!(ammo.shells, shells - 1);
    }

    #[test]
    fn angle_wraps_both_directions() {
        assert!((wrap_angle(-PI * 0.5) - PI * 1.5).abs() < 1e-10);
        assert!((wrap_angle(TAU + 0.25) - 0.25).abs() < 1e-10);
    }
}
