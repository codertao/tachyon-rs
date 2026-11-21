//! The compiled-in weapon definitions (`g_WeaponDefs 0x55a560`, stride 0x110).
//! GENERATED from `decomp/notes/weapons.json` (see `decomp/notes/weapons.md`); do not edit by hand.
//! Distances are world units (raw / 65536); speed is units per second at 50 Hz.

use crate::des::{CLASS_BOMBER, CLASS_CAPSHIP, CLASS_FIGHTER, CLASS_STARBASE};

/// Every class bit (fighter, bomber, capship, starbase, turret).
pub const TARGET_ANY: u32 = 0x1f;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponClass { Gun, Laser, Missile, Rail, Rocket, Sapper, Special, Torpedo, Turret }

/// def+0x84: 0 homing missile, 1 torpedo, 2 unguided. Only the launch-phase length depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guidance { Missile, Torpedo, Unguided }

#[derive(Debug, Clone, Copy)]
pub struct WeaponDef {
    pub id: u8,
    pub internal: &'static str,
    pub name: &'static str,
    pub class: WeaponClass,
    pub model: &'static str,
    /// def+0x14, base damage before the shield/hull factors.
    pub damage: i32,
    /// Factors out of 256.
    pub shield_factor: i32,
    pub hull_factor: i32,
    pub system_factor: i32,
    pub refire_frames: u32,
    pub ai_refire_frames: u32,
    pub energy: i32,
    pub ammo: i32,
    pub speed: f32,
    pub life_s: f32,
    pub max_travel: f32,
    pub ai_range: f32,
    /// AI class bits this weapon may be aimed at (def+0x5c).
    pub targets: u32,
    pub guidance: Guidance,
    /// def+0x88 / +0x8c.
    pub turn_max: f32,
    pub turn_ramp: f32,
    pub sapper_flags: u32,
    pub sapper_time: u32,
}

pub static WEAPONS: &[WeaponDef] = &[
    WeaponDef { id: 0, internal: "GALSPAN_LIGHT_LASER", name: "Glint Lt Laser", class: WeaponClass::Laser, model: "glazl.pak",
        damage: 1250, shield_factor: 256, hull_factor: 70, system_factor: 0,
        refire_frames: 10, ai_refire_frames: 10, energy: 50, ammo: 0,
        speed: 375.0, life_s: 3.0, max_travel: 1125.0, ai_range: 437.5,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 1, internal: "GALSPAN_MEDIUM_LASER", name: "Flare Med Laser", class: WeaponClass::Laser, model: "glazm.pak",
        damage: 1750, shield_factor: 256, hull_factor: 150, system_factor: 0,
        refire_frames: 12, ai_refire_frames: 12, energy: 65, ammo: 0,
        speed: 375.0, life_s: 3.0, max_travel: 1125.0, ai_range: 625.0,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 2, internal: "GALSPAN_HEAVY_LASER", name: "Deimos Hvy Laser", class: WeaponClass::Laser, model: "glazh.pak",
        damage: 7500, shield_factor: 256, hull_factor: 153, system_factor: 0,
        refire_frames: 50, ai_refire_frames: 50, energy: 96, ammo: 0,
        speed: 407.5, life_s: 3.0, max_travel: 1222.0, ai_range: 625.0,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 3, internal: "BORA_LIGHT_LASER", name: "Arc Lt Laser", class: WeaponClass::Laser, model: "blazl.pak",
        damage: 1150, shield_factor: 256, hull_factor: 64, system_factor: 0,
        refire_frames: 10, ai_refire_frames: 10, energy: 55, ammo: 0,
        speed: 310.0, life_s: 3.0, max_travel: 930.0, ai_range: 437.5,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 4, internal: "BORA_MEDIUM_LASER", name: "Boran Med Laser", class: WeaponClass::Laser, model: "blazm.pak",
        damage: 1650, shield_factor: 256, hull_factor: 180, system_factor: 0,
        refire_frames: 12, ai_refire_frames: 12, energy: 70, ammo: 0,
        speed: 310.0, life_s: 3.0, max_travel: 930.0, ai_range: 625.0,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 5, internal: "BORA_HEAVY_LASER", name: "Hvy Mining Laser", class: WeaponClass::Laser, model: "blazh.pak",
        damage: 2500, shield_factor: 256, hull_factor: 128, system_factor: 0,
        refire_frames: 15, ai_refire_frames: 15, energy: 100, ammo: 0,
        speed: 342.5, life_s: 3.0, max_travel: 1027.0, ai_range: 625.0,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 6, internal: "ADVANCED_LASER", name: "Pulsar Laser", class: WeaponClass::Laser, model: "blaza.pak",
        damage: 1000, shield_factor: 256, hull_factor: 204, system_factor: 0,
        refire_frames: 5, ai_refire_frames: 5, energy: 130, ammo: 0,
        speed: 440.0, life_s: 3.0, max_travel: 1320.0, ai_range: 625.0,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 7, internal: "FORTIS_TORPEDO", name: "Solaris Torpedo", class: WeaponClass::Torpedo, model: "torpfrts.pak",
        damage: 35000, shield_factor: 256, hull_factor: 196, system_factor: 0,
        refire_frames: 25, ai_refire_frames: 500, energy: 10, ammo: 0,
        speed: 150.0, life_s: 15.0, max_travel: 2250.0, ai_range: 1000.0,
        targets: CLASS_CAPSHIP | CLASS_STARBASE, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 8, internal: "SECO_TORPEDO", name: "Hunter Torpedo", class: WeaponClass::Torpedo, model: "torpseco.pak",
        damage: 5000, shield_factor: 256, hull_factor: 196, system_factor: 0,
        refire_frames: 200, ai_refire_frames: 200, energy: 2000, ammo: 0,
        speed: 175.0, life_s: 8.0, max_travel: 1400.0, ai_range: 500.0,
        targets: TARGET_ANY, guidance: Guidance::Torpedo, turn_max: 10.0, turn_ramp: 0.125,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 9, internal: "BLAST_TORPEDO", name: "Blast Torpedo", class: WeaponClass::Torpedo, model: "torpblst.pak",
        damage: 12000, shield_factor: 256, hull_factor: 256, system_factor: 0,
        refire_frames: 250, ai_refire_frames: 250, energy: 8000, ammo: 0,
        speed: 112.5, life_s: 8.0, max_travel: 900.0, ai_range: 625.0,
        targets: TARGET_ANY, guidance: Guidance::Torpedo, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 10, internal: "ADVANCED_BLAST_TORPEDO", name: "Adv Blast Torpedo", class: WeaponClass::Torpedo, model: "torpabst.pak",
        damage: 12000, shield_factor: 256, hull_factor: 256, system_factor: 0,
        refire_frames: 250, ai_refire_frames: 250, energy: 12000, ammo: 0,
        speed: 175.0, life_s: 5.0, max_travel: 875.0, ai_range: 1250.0,
        targets: TARGET_ANY, guidance: Guidance::Torpedo, turn_max: 24.0, turn_ramp: 0.25,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 11, internal: "SHIELD_TORPEDO", name: "Disruptor Torpedo", class: WeaponClass::Torpedo, model: "torpshld.pak",
        damage: 40000, shield_factor: 256, hull_factor: 0, system_factor: 0,
        refire_frames: 1500, ai_refire_frames: 1500, energy: 8000, ammo: 0,
        speed: 200.0, life_s: 10.0, max_travel: 2000.0, ai_range: 1250.0,
        targets: TARGET_ANY, guidance: Guidance::Torpedo, turn_max: 18.0, turn_ramp: 0.25,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 12, internal: "TIGER_MISSILE", name: "Tiger Missile", class: WeaponClass::Missile, model: "misstigr.pak",
        damage: 8000, shield_factor: 64, hull_factor: 256, system_factor: 0,
        refire_frames: 25, ai_refire_frames: 200, energy: 2000, ammo: 12,
        speed: 188.1, life_s: 30.0, max_travel: 5644.0, ai_range: 1250.0,
        targets: TARGET_ANY, guidance: Guidance::Missile, turn_max: 32.0, turn_ramp: 0.25,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 13, internal: "HEAVY_MISSILE", name: "Sunspot Missile", class: WeaponClass::Missile, model: "misshevy.pak",
        damage: 14000, shield_factor: 182, hull_factor: 256, system_factor: 0,
        refire_frames: 25, ai_refire_frames: 200, energy: 2750, ammo: 8,
        speed: 187.5, life_s: 30.0, max_travel: 5625.0, ai_range: 1250.0,
        targets: TARGET_ANY, guidance: Guidance::Missile, turn_max: 20.0, turn_ramp: 0.5,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 14, internal: "DISABLER_MISSILE", name: "Tesla EMP Missile", class: WeaponClass::Missile, model: "missdisb.pak",
        damage: 25000, shield_factor: 96, hull_factor: 0, system_factor: 256,
        refire_frames: 250, ai_refire_frames: 250, energy: 2750, ammo: 6,
        speed: 130.0, life_s: 30.0, max_travel: 3900.0, ai_range: 1250.0,
        targets: TARGET_ANY, guidance: Guidance::Missile, turn_max: 16.0, turn_ramp: 0.25,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 15, internal: "SWARM_MISSILE", name: "Swarm Missile", class: WeaponClass::Missile, model: "missswrm.pak",
        damage: 17000, shield_factor: 180, hull_factor: 256, system_factor: 0,
        refire_frames: 25, ai_refire_frames: 250, energy: 2500, ammo: 8,
        speed: 212.5, life_s: 20.0, max_travel: 4250.0, ai_range: 1250.0,
        targets: TARGET_ANY, guidance: Guidance::Missile, turn_max: 16.0, turn_ramp: 0.25,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 16, internal: "REMOTE_MINE", name: "Remote Sapper", class: WeaponClass::Sapper, model: "mineremt.pak",
        damage: 2200, shield_factor: 256, hull_factor: 128, system_factor: 0,
        refire_frames: 14, ai_refire_frames: 14, energy: 96, ammo: 2,
        speed: 300.0, life_s: 3.0, max_travel: 900.0, ai_range: 437.5,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 8, sapper_time: 6000 },
    WeaponDef { id: 17, internal: "INERTIAL_MINE", name: "Inertia Sapper", class: WeaponClass::Sapper, model: "mineinrt.pak",
        damage: 2200, shield_factor: 256, hull_factor: 128, system_factor: 0,
        refire_frames: 14, ai_refire_frames: 14, energy: 96, ammo: 12,
        speed: 300.0, life_s: 3.0, max_travel: 900.0, ai_range: 437.5,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 1, sapper_time: 750 },
    WeaponDef { id: 18, internal: "POWER_MINE", name: "Power Sapper", class: WeaponClass::Sapper, model: "minepwer.pak",
        damage: 2200, shield_factor: 256, hull_factor: 128, system_factor: 0,
        refire_frames: 14, ai_refire_frames: 14, energy: 96, ammo: 16,
        speed: 300.0, life_s: 3.0, max_travel: 900.0, ai_range: 437.5,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 2, sapper_time: 1500 },
    WeaponDef { id: 19, internal: "SHIELD_MINE", name: "Phase Sapper", class: WeaponClass::Sapper, model: "mineshld.pak",
        damage: 2200, shield_factor: 256, hull_factor: 128, system_factor: 0,
        refire_frames: 14, ai_refire_frames: 14, energy: 96, ammo: 16,
        speed: 300.0, life_s: 3.0, max_travel: 900.0, ai_range: 437.5,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 4, sapper_time: 1000 },
    WeaponDef { id: 20, internal: "SPIRE_ROCKET", name: "Spire Rocket", class: WeaponClass::Rocket, model: "roktspir.pak",
        damage: 3500, shield_factor: 255, hull_factor: 255, system_factor: 0,
        refire_frames: 25, ai_refire_frames: 50, energy: 1000, ammo: 40,
        speed: 325.0, life_s: 10.0, max_travel: 3250.0, ai_range: 1250.0,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 21, internal: "PLASMA_ROCKET", name: "Plasma Rocket", class: WeaponClass::Rocket, model: "roktplas.pak",
        damage: 18000, shield_factor: 255, hull_factor: 100, system_factor: 0,
        refire_frames: 25, ai_refire_frames: 50, energy: 1000, ammo: 20,
        speed: 260.0, life_s: 10.0, max_travel: 2600.0, ai_range: 1250.0,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 22, internal: "HELIOS_ROCKET", name: "Helios Rocket", class: WeaponClass::Rocket, model: "rokthlos.pak",
        damage: 90000, shield_factor: 255, hull_factor: 255, system_factor: 0,
        refire_frames: 25, ai_refire_frames: 200, energy: 1000, ammo: 2,
        speed: 62.5, life_s: 30.0, max_travel: 1875.0, ai_range: 2500.0,
        targets: CLASS_CAPSHIP | CLASS_STARBASE, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 23, internal: "RAILGUN", name: "Railgun", class: WeaponClass::Rail, model: "railgun.pak",
        damage: 20000, shield_factor: 95, hull_factor: 256, system_factor: 0,
        refire_frames: 200, ai_refire_frames: 200, energy: 3500, ammo: 0,
        speed: 0.0, life_s: 3.0, max_travel: 0.0, ai_range: 437.5,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 24, internal: "GATLING_GUN", name: "Chatter Cannon", class: WeaponClass::Gun, model: "bullet.pak",
        damage: 500, shield_factor: 24, hull_factor: 256, system_factor: 0,
        refire_frames: 5, ai_refire_frames: 5, energy: 150, ammo: 750,
        speed: 300.0, life_s: 3.0, max_travel: 900.0, ai_range: 437.5,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 25, internal: "EMP_CANNON", name: "EMP Projector", class: WeaponClass::Gun, model: "empburst.pak",
        damage: 1500, shield_factor: 128, hull_factor: 0, system_factor: 256,
        refire_frames: 6, ai_refire_frames: 6, energy: 75, ammo: 0,
        speed: 250.0, life_s: 3.0, max_travel: 750.0, ai_range: 437.5,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 26, internal: "CORONA_DEVICE", name: "Corona Device", class: WeaponClass::Special, model: "corona.pak",
        damage: 12000, shield_factor: 256, hull_factor: 256, system_factor: 0,
        refire_frames: 50, ai_refire_frames: 50, energy: 22000, ammo: 0,
        speed: 250.0, life_s: 3.0, max_travel: 750.0, ai_range: 62.5,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 27, internal: "TRACTOR_WAVE", name: "Tractor Wave", class: WeaponClass::Special, model: "tractor.pak",
        damage: 1500, shield_factor: 128, hull_factor: 0, system_factor: 256,
        refire_frames: 50, ai_refire_frames: 50, energy: 15, ammo: 0,
        speed: 250.0, life_s: 327680.0, max_travel: 81920000.0, ai_range: 437.5,
        targets: TARGET_ANY, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 29, internal: "LIGHT TURRET", name: "LIGHT TURRET", class: WeaponClass::Turret, model: "gtlazl.pak",
        damage: 3000, shield_factor: 256, hull_factor: 128, system_factor: 0,
        refire_frames: 25, ai_refire_frames: 25, energy: 0, ammo: 0,
        speed: 400.0, life_s: 5.0, max_travel: 2000.0, ai_range: 1500.0,
        targets: CLASS_FIGHTER | CLASS_BOMBER, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 30, internal: "LARGE TURRET", name: "LARGE TURRET", class: WeaponClass::Turret, model: "gtlazh.pak",
        damage: 6000, shield_factor: 256, hull_factor: 128, system_factor: 0,
        refire_frames: 50, ai_refire_frames: 50, energy: 0, ammo: 0,
        speed: 400.0, life_s: 5.0, max_travel: 2000.0, ai_range: 1500.0,
        targets: CLASS_FIGHTER | CLASS_BOMBER, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 31, internal: "LARGE TURRET", name: "LARGE TURRET", class: WeaponClass::Turret, model: "turlaser.pak",
        damage: 8000, shield_factor: 256, hull_factor: 128, system_factor: 0,
        refire_frames: 50, ai_refire_frames: 50, energy: 0, ammo: 0,
        speed: 400.0, life_s: 5.0, max_travel: 2000.0, ai_range: 1500.0,
        targets: CLASS_FIGHTER | CLASS_BOMBER, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 32, internal: "LARGE TURRET", name: "LARGE TURRET", class: WeaponClass::Turret, model: "turlasr2.pak",
        damage: 10000, shield_factor: 256, hull_factor: 128, system_factor: 0,
        refire_frames: 50, ai_refire_frames: 50, energy: 0, ammo: 0,
        speed: 400.0, life_s: 5.0, max_travel: 2000.0, ai_range: 1500.0,
        targets: CLASS_FIGHTER | CLASS_BOMBER, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 33, internal: "MINE BLAST", name: "MINE BLAST", class: WeaponClass::Turret, model: "torpblst.pak",
        damage: 15000, shield_factor: 256, hull_factor: 256, system_factor: 0,
        refire_frames: 250, ai_refire_frames: 250, energy: 3000, ammo: 0,
        speed: 0.0, life_s: 0.02, max_travel: 0.0, ai_range: 625.0,
        targets: TARGET_ANY, guidance: Guidance::Torpedo, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 34, internal: "BEAM_A", name: "BEAM_A", class: WeaponClass::Turret, model: "beam_a.pak",
        damage: 70000, shield_factor: 256, hull_factor: 256, system_factor: 0,
        refire_frames: 500, ai_refire_frames: 500, energy: 3500, ammo: 0,
        speed: 0.0, life_s: 3.0, max_travel: 0.0, ai_range: 1187.5,
        targets: CLASS_CAPSHIP | CLASS_STARBASE, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 35, internal: "BEAM_A", name: "BEAM_A", class: WeaponClass::Turret, model: "beam_d.pak",
        damage: 70000, shield_factor: 256, hull_factor: 256, system_factor: 0,
        refire_frames: 500, ai_refire_frames: 500, energy: 3500, ammo: 0,
        speed: 0.0, life_s: 3.0, max_travel: 0.0, ai_range: 1187.5,
        targets: CLASS_CAPSHIP | CLASS_STARBASE, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 36, internal: "BEAM_A", name: "BEAM_A", class: WeaponClass::Turret, model: "beam_c.pak",
        damage: 70000, shield_factor: 256, hull_factor: 256, system_factor: 0,
        refire_frames: 500, ai_refire_frames: 500, energy: 3500, ammo: 0,
        speed: 0.0, life_s: 3.0, max_travel: 0.0, ai_range: 1187.5,
        targets: CLASS_CAPSHIP | CLASS_STARBASE, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
    WeaponDef { id: 37, internal: "BEAM_A", name: "BEAM_A", class: WeaponClass::Turret, model: "sbeam.pak",
        damage: 200000, shield_factor: 256, hull_factor: 256, system_factor: 0,
        refire_frames: 500, ai_refire_frames: 500, energy: 3500, ammo: 0,
        speed: 0.0, life_s: 3.0, max_travel: 0.0, ai_range: 1937.5,
        targets: CLASS_CAPSHIP | CLASS_STARBASE, guidance: Guidance::Unguided, turn_max: 0.0, turn_ramp: 0.0,
        sapper_flags: 0, sapper_time: 0 },
];

/// Look a weapon up by its id (0x00..=0x25; 0x1c is unused).
pub fn by_id(id: u8) -> Option<&'static WeaponDef> {
    WEAPONS.iter().find(|w| w.id == id)
}

/// Look a weapon up by its internal name (`.des` `AI_WEAPON`), ignoring case.
pub fn by_name(name: &str) -> Option<&'static WeaponDef> {
    WEAPONS.iter().find(|w| w.internal.eq_ignore_ascii_case(name))
}

/// Turret part `SUB_TYPE` → weapon id (table at `0x533e90`; see `capships.md`).
pub fn turret_weapon(sub_type: i32) -> Option<u8> {
    Some(match sub_type {
        0 => 0x1d,
        1 => 0x1e,
        4 => 0x1f,
        7 => 0x20,
        2 => 0x22,
        3 => 0x23,
        6 => 0x24,
        5 => 0x25,
        _ => return None,
    })
}

/// def+0x04, the AI usage bits (1 laser-like, 2 missile, 4 torpedo, 8 sapper, 0x10 rocket/gun,
/// 0x20 Helios), by weapon id. Ammo powerups refill the mounts whose bits equal theirs
/// (`Resupply_GiveAmmo`), so the Gatling Gun takes rocket ammo and the Helios Rocket takes none.
pub const AI_USAGE: [u32; 0x27] = [
    0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x4, 0x4, 0x4, 0x4, 0x2, 0x2, 0x2, 0x2, 0x8, 0x8, 0x8, 0x8,
    0x10, 0x10, 0x20, 0x1, 0x10, 0x1, 0x1, 0x1, 0, 0x1, 0x1, 0x1, 0x1, 0x2, 0x1, 0x1, 0x1, 0x1, 0,
];

pub fn ai_usage(id: u8) -> u32 {
    AI_USAGE.get(id as usize).copied().unwrap_or(0)
}

/// Raw guided-weapon fields the server's missile simulation needs (`g_WeaponDefs`), in engine
/// units: angles 16.16 with 512 units per turn, speeds 16.16 units per frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissileRaw {
    pub id: u8,
    /// def+0x84: launch phase index into `LAUNCH_FRAMES`.
    pub guidance: u8,
    /// def+0x88 / +0x8c: max turn per frame and its ramp per frame.
    pub turn_max: i32,
    pub turn_ramp: i32,
    /// def+0x3c: life in frames after the launch phase.
    pub life: u32,
    /// def+0x54.
    pub speed: i32,
    /// def+0x9c: a blast's starting scale growth per frame.
    pub blast_growth: i32,
    /// def+0xd0: hit-test scale applied to the target's model (0 = the target's own +0xb8).
    pub cast_scale: i32,
    /// def+0xd8: bit 1 = a blast spares its shooter.
    pub flags_d8: u32,
}

/// Launch-phase frames by guidance class (`0x55a51c`, stride 8): missiles 6, torpedoes 2,
/// Helios 0.
pub const LAUNCH_FRAMES: [u32; 3] = [6, 2, 0];

pub const MISSILES: [MissileRaw; 10] = [
    MissileRaw { id: 8, guidance: 1, turn_max: 0xa0000, turn_ramp: 0x2000, life: 400, speed: 0x38000, blast_growth: 0, cast_scale: 0x30000, flags_d8: 0 },
    MissileRaw { id: 9, guidance: 1, turn_max: 0, turn_ramp: 0, life: 400, speed: 0x24000, blast_growth: 0xea60, cast_scale: 0x30000, flags_d8: 0 },
    MissileRaw { id: 10, guidance: 1, turn_max: 0x180000, turn_ramp: 0x4000, life: 250, speed: 0x38000, blast_growth: 0xea60, cast_scale: 0x30000, flags_d8: 0 },
    MissileRaw { id: 11, guidance: 1, turn_max: 0x120000, turn_ramp: 0x4000, life: 500, speed: 0x40000, blast_growth: 0xea60, cast_scale: 0x30000, flags_d8: 0 },
    MissileRaw { id: 12, guidance: 0, turn_max: 0x200000, turn_ramp: 0x4000, life: 1500, speed: 0x3c350, blast_growth: 0, cast_scale: 0x30000, flags_d8: 0 },
    MissileRaw { id: 13, guidance: 0, turn_max: 0x140000, turn_ramp: 0x8000, life: 1500, speed: 0x3c000, blast_growth: 0, cast_scale: 0x30000, flags_d8: 0 },
    MissileRaw { id: 14, guidance: 0, turn_max: 0x100000, turn_ramp: 0x4000, life: 1500, speed: 0x29999, blast_growth: 0, cast_scale: 0x30000, flags_d8: 0 },
    MissileRaw { id: 15, guidance: 0, turn_max: 0x100000, turn_ramp: 0x4000, life: 1000, speed: 0x44000, blast_growth: 0, cast_scale: 0x30000, flags_d8: 0 },
    MissileRaw { id: 22, guidance: 2, turn_max: 0, turn_ramp: 0, life: 1500, speed: 0x14000, blast_growth: 0, cast_scale: 0, flags_d8: 0 },
    MissileRaw { id: 33, guidance: 1, turn_max: 0, turn_ramp: 0, life: 1, speed: 0, blast_growth: 0x15f90, cast_scale: 0, flags_d8: 0 },
];

/// The guided (server-owned) weapons: those fired through 0x2004.
pub fn missile(id: u8) -> Option<&'static MissileRaw> {
    MISSILES.iter().find(|m| m.id == id)
}
