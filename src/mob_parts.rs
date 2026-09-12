//! Table-driven mob part descriptors (Plan 22).
//!
//! `render_mobs` looks up a static `&[MobPart]` per `EntityType`, then a small
//! animator resolves limb pitch / scale / texture overrides. Only dragon,
//! wither, and dropped-item keep fully custom emitters.

use crate::entity::{Entity, EntityType};
use crate::mob_renderer::{
    add_cuboid, MobInstance, PLAYER_ARM_COL, PLAYER_ARM_ROW, PLAYER_HEAD_COLS, PLAYER_HEAD_ROW,
};
use glam::Vec3;

/// One cuboid part of a mob silhouette.
#[derive(Clone, Copy, Debug)]
pub struct MobPart {
    pub size: [f32; 3],
    pub offset: [f32; 3],
    pub pivot: [f32; 3],
    pub tex_cols: [u32; 6],
    pub tex_row: u32,
    pub limb: Limb,
    pub scale: PartScale,
    pub pivot_mode: PivotMode,
    pub tex_mode: TexMode,
    pub flags: u8,
}

pub const FLAG_PIGLIN_ONLY: u8 = 1 << 0;
pub const FLAG_PIVOT_SCALED: u8 = 1 << 1;
pub const FLAG_BLAZE_HOVER: u8 = 1 << 2;
pub const FLAG_SHULKER_LID: u8 = 1 << 3;
pub const FLAG_MAX_LIGHT: u8 = 1 << 4;
pub const FLAG_CRYSTAL_SPIN: u8 = 1 << 5;

#[derive(Clone, Copy, Debug)]
pub enum Limb {
    Static,
    Look,
    Walk,
    WalkOpp,
    ZombieArm,
    SkelLeftArm,
    SkelRightArm,
    Flap,
    NegFlap,
    GrazingLook,
    Pitch(f32),
    SpinYaw,
    SpinYawNeg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartScale {
    None,
    Baby,
    BabyHead,
    CreeperSwell,
    SlimeSize,
    BreathPulse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PivotMode {
    Local,
    World,
    WolfBody,
    WolfHead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexMode {
    Fixed,
    SheepBody,
    PiglinHead,
    PiglinBody,
    HuskHead,
    HuskBody,
    PlayerHead,
    PlayerArm,
}


pub const ZOMBIE_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.25, 0.0],
        pivot: [0.0, 1.4, 0.0],
        tex_cols: [0, 1, 1, 1, 1, 1],
        tex_row: 9,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.5, 0.75, 0.25],
        offset: [0.0, 0.375, 0.0],
        pivot: [0.0, 0.65, 0.0],
        tex_cols: [2; 6],
        tex_row: 9,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.2, 0.75, 0.2],
        offset: [0.0, -0.325, 0.0],
        pivot: [-0.35, 1.3, 0.0],
        tex_cols: [3; 6],
        tex_row: 9,
        limb: Limb::ZombieArm,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.2, 0.75, 0.2],
        offset: [0.0, -0.325, 0.0],
        pivot: [0.35, 1.3, 0.0],
        tex_cols: [3; 6],
        tex_row: 9,
        limb: Limb::ZombieArm,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.2, 0.75, 0.2],
        offset: [0.0, -0.375, 0.0],
        pivot: [-0.125, 0.75, 0.0],
        tex_cols: [3; 6],
        tex_row: 9,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.2, 0.75, 0.2],
        offset: [0.0, -0.375, 0.0],
        pivot: [0.125, 0.75, 0.0],
        tex_cols: [3; 6],
        tex_row: 9,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const SKELETON_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.25, 0.0],
        pivot: [0.0, 1.4, 0.0],
        tex_cols: [4, 5, 5, 5, 5, 5],
        tex_row: 9,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.4, 0.75, 0.2],
        offset: [0.0, 0.375, 0.0],
        pivot: [0.0, 0.65, 0.0],
        tex_cols: [5; 6],
        tex_row: 9,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.15, 0.75, 0.15],
        offset: [0.0, -0.325, 0.0],
        pivot: [-0.275, 1.3, 0.0],
        tex_cols: [5; 6],
        tex_row: 9,
        limb: Limb::SkelLeftArm,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.15, 0.75, 0.15],
        offset: [0.0, -0.325, 0.0],
        pivot: [0.275, 1.3, 0.0],
        tex_cols: [5; 6],
        tex_row: 9,
        limb: Limb::SkelRightArm,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.15, 0.75, 0.15],
        offset: [0.0, -0.375, 0.0],
        pivot: [-0.1, 0.75, 0.0],
        tex_cols: [5; 6],
        tex_row: 9,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.15, 0.75, 0.15],
        offset: [0.0, -0.375, 0.0],
        pivot: [0.1, 0.75, 0.0],
        tex_cols: [5; 6],
        tex_row: 9,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const CREEPER_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.25, 0.0],
        pivot: [0.0, 1.125, 0.0],
        tex_cols: [6, 7, 7, 7, 7, 7],
        tex_row: 9,
        limb: Limb::Look,
        scale: PartScale::CreeperSwell,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.5, 0.75, 0.3],
        offset: [0.0, 0.375, 0.0],
        pivot: [0.0, 0.375, 0.0],
        tex_cols: [7; 6],
        tex_row: 9,
        limb: Limb::Static,
        scale: PartScale::CreeperSwell,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.25, 0.375, 0.25],
        offset: [0.0, -0.1875, 0.0],
        pivot: [-0.125, 0.375, 0.125],
        tex_cols: [7; 6],
        tex_row: 9,
        limb: Limb::Walk,
        scale: PartScale::CreeperSwell,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.25, 0.375, 0.25],
        offset: [0.0, -0.1875, 0.0],
        pivot: [0.125, 0.375, 0.125],
        tex_cols: [7; 6],
        tex_row: 9,
        limb: Limb::WalkOpp,
        scale: PartScale::CreeperSwell,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.25, 0.375, 0.25],
        offset: [0.0, -0.1875, 0.0],
        pivot: [-0.125, 0.375, -0.125],
        tex_cols: [7; 6],
        tex_row: 9,
        limb: Limb::WalkOpp,
        scale: PartScale::CreeperSwell,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.25, 0.375, 0.25],
        offset: [0.0, -0.1875, 0.0],
        pivot: [0.125, 0.375, -0.125],
        tex_cols: [7; 6],
        tex_row: 9,
        limb: Limb::Walk,
        scale: PartScale::CreeperSwell,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const ARROW_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.06, 0.06, 0.6],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.0, 0.0],
        tex_cols: [8; 6],
        tex_row: 9,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::World,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const SPLASHPOTION_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.06, 0.06, 0.6],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.0, 0.0],
        tex_cols: [8; 6],
        tex_row: 9,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::World,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const PIG_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.15, 0.2],
        pivot: [0.0, 0.8, 0.2],
        tex_cols: [0, 1, 1, 1, 1, 1],
        tex_row: 10,
        limb: Limb::Look,
        scale: PartScale::BabyHead,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.6, 0.6, 0.8],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.7, 0.0],
        tex_cols: [1; 6],
        tex_row: 10,
        limb: Limb::Static,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.2, 0.4, 0.2],
        offset: [0.0, -0.2, 0.0],
        pivot: [-0.25, 0.4, 0.25],
        tex_cols: [1; 6],
        tex_row: 10,
        limb: Limb::Walk,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.2, 0.4, 0.2],
        offset: [0.0, -0.2, 0.0],
        pivot: [0.25, 0.4, 0.25],
        tex_cols: [1; 6],
        tex_row: 10,
        limb: Limb::WalkOpp,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.2, 0.4, 0.2],
        offset: [0.0, -0.2, 0.0],
        pivot: [-0.25, 0.4, -0.25],
        tex_cols: [1; 6],
        tex_row: 10,
        limb: Limb::WalkOpp,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.2, 0.4, 0.2],
        offset: [0.0, -0.2, 0.0],
        pivot: [0.25, 0.4, -0.25],
        tex_cols: [1; 6],
        tex_row: 10,
        limb: Limb::Walk,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
];

pub const COW_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.15, 0.2],
        pivot: [0.0, 1.1, 0.35],
        tex_cols: [2, 3, 3, 3, 3, 3],
        tex_row: 10,
        limb: Limb::Look,
        scale: PartScale::BabyHead,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.7, 0.8, 1.0],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 1.0, 0.0],
        tex_cols: [3; 6],
        tex_row: 10,
        limb: Limb::Static,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.22, 0.6, 0.22],
        offset: [0.0, -0.3, 0.0],
        pivot: [-0.25, 0.6, 0.35],
        tex_cols: [3; 6],
        tex_row: 10,
        limb: Limb::Walk,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.22, 0.6, 0.22],
        offset: [0.0, -0.3, 0.0],
        pivot: [0.25, 0.6, 0.35],
        tex_cols: [3; 6],
        tex_row: 10,
        limb: Limb::WalkOpp,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.22, 0.6, 0.22],
        offset: [0.0, -0.3, 0.0],
        pivot: [-0.25, 0.6, -0.35],
        tex_cols: [3; 6],
        tex_row: 10,
        limb: Limb::WalkOpp,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.22, 0.6, 0.22],
        offset: [0.0, -0.3, 0.0],
        pivot: [0.25, 0.6, -0.35],
        tex_cols: [3; 6],
        tex_row: 10,
        limb: Limb::Walk,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
];

pub const SHEEP_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.45, 0.45, 0.45],
        offset: [0.0, 0.15, 0.2],
        pivot: [0.0, 0.9, 0.3],
        tex_cols: [3, 4, 4, 4, 4, 4],
        tex_row: 7,
        limb: Limb::GrazingLook,
        scale: PartScale::BabyHead,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.6, 0.6, 0.9],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.8, 0.0],
        tex_cols: [5; 6],
        tex_row: 10,
        limb: Limb::Static,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::SheepBody,
        flags: 2,
    },
    MobPart {
        size: [0.2, 0.5, 0.2],
        offset: [0.0, -0.25, 0.0],
        pivot: [-0.25, 0.5, 0.3],
        tex_cols: [5; 6],
        tex_row: 7,
        limb: Limb::Walk,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.2, 0.5, 0.2],
        offset: [0.0, -0.25, 0.0],
        pivot: [0.25, 0.5, 0.3],
        tex_cols: [5; 6],
        tex_row: 7,
        limb: Limb::WalkOpp,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.2, 0.5, 0.2],
        offset: [0.0, -0.25, 0.0],
        pivot: [-0.25, 0.5, -0.3],
        tex_cols: [5; 6],
        tex_row: 7,
        limb: Limb::WalkOpp,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.2, 0.5, 0.2],
        offset: [0.0, -0.25, 0.0],
        pivot: [0.25, 0.5, -0.3],
        tex_cols: [5; 6],
        tex_row: 7,
        limb: Limb::Walk,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
];

pub const CHICKEN_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.25, 0.35, 0.25],
        offset: [0.0, 0.1, 0.15],
        pivot: [0.0, 0.45, 0.1],
        tex_cols: [7, 8, 8, 8, 8, 8],
        tex_row: 10,
        limb: Limb::Look,
        scale: PartScale::BabyHead,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.3, 0.3, 0.4],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.35, 0.0],
        tex_cols: [8; 6],
        tex_row: 10,
        limb: Limb::Static,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.05, 0.25, 0.25],
        offset: [0.0, -0.1, 0.0],
        pivot: [-0.175, 0.35, 0.0],
        tex_cols: [8; 6],
        tex_row: 10,
        limb: Limb::Flap,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.05, 0.25, 0.25],
        offset: [0.0, -0.1, 0.0],
        pivot: [0.175, 0.35, 0.0],
        tex_cols: [8; 6],
        tex_row: 10,
        limb: Limb::NegFlap,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.06, 0.2, 0.06],
        offset: [0.0, -0.1, 0.0],
        pivot: [-0.06, 0.2, 0.0],
        tex_cols: [8; 6],
        tex_row: 10,
        limb: Limb::Walk,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.06, 0.2, 0.06],
        offset: [0.0, -0.1, 0.0],
        pivot: [0.06, 0.2, 0.0],
        tex_cols: [8; 6],
        tex_row: 10,
        limb: Limb::WalkOpp,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
];

pub const PIGLIN_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.25, 0.0],
        pivot: [0.0, 1.45, 0.0],
        tex_cols: [11; 6],
        tex_row: 9,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::PiglinHead,
        flags: 0,
    },
    MobPart {
        size: [0.5, 0.7, 0.28],
        offset: [0.0, 0.35, 0.0],
        pivot: [0.0, 0.75, 0.0],
        tex_cols: [12; 6],
        tex_row: 9,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::PiglinBody,
        flags: 0,
    },
    MobPart {
        size: [0.2, 0.75, 0.2],
        offset: [0.0, -0.325, 0.0],
        pivot: [-0.35, 1.4, 0.0],
        tex_cols: [12; 6],
        tex_row: 15,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::PiglinBody,
        flags: 0,
    },
    MobPart {
        size: [0.2, 0.75, 0.2],
        offset: [0.0, -0.325, 0.0],
        pivot: [0.35, 1.4, 0.0],
        tex_cols: [12; 6],
        tex_row: 15,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::PiglinBody,
        flags: 0,
    },
    MobPart {
        size: [0.22, 0.75, 0.22],
        offset: [0.0, -0.375, 0.0],
        pivot: [-0.13, 0.75, 0.0],
        tex_cols: [12; 6],
        tex_row: 15,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::PiglinBody,
        flags: 0,
    },
    MobPart {
        size: [0.22, 0.75, 0.22],
        offset: [0.0, -0.375, 0.0],
        pivot: [0.13, 0.75, 0.0],
        tex_cols: [12; 6],
        tex_row: 15,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::PiglinBody,
        flags: 0,
    },
    MobPart {
        size: [0.18, 0.24, 0.08],
        offset: [0.0, 0.0, 0.0],
        pivot: [-0.34, 1.72, 0.0],
        tex_cols: [11; 6],
        tex_row: 9,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 1,
    },
    MobPart {
        size: [0.18, 0.24, 0.08],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.34, 1.72, 0.0],
        tex_cols: [11; 6],
        tex_row: 9,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 1,
    },
    MobPart {
        size: [0.2, 0.16, 0.12],
        offset: [0.0, 0.0, 0.29],
        pivot: [0.0, 1.68, 0.0],
        tex_cols: [11; 6],
        tex_row: 9,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 1,
    },
];

pub const HUSK_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.25, 0.0],
        pivot: [0.0, 1.45, 0.0],
        tex_cols: [13; 6],
        tex_row: 9,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::HuskHead,
        flags: 0,
    },
    MobPart {
        size: [0.5, 0.7, 0.28],
        offset: [0.0, 0.35, 0.0],
        pivot: [0.0, 0.75, 0.0],
        tex_cols: [14; 6],
        tex_row: 9,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::HuskBody,
        flags: 0,
    },
    MobPart {
        size: [0.2, 0.75, 0.2],
        offset: [0.0, -0.325, 0.0],
        pivot: [-0.35, 1.4, 0.0],
        tex_cols: [14; 6],
        tex_row: 15,
        limb: Limb::ZombieArm,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::HuskBody,
        flags: 0,
    },
    MobPart {
        size: [0.2, 0.75, 0.2],
        offset: [0.0, -0.325, 0.0],
        pivot: [0.35, 1.4, 0.0],
        tex_cols: [14; 6],
        tex_row: 15,
        limb: Limb::ZombieArm,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::HuskBody,
        flags: 0,
    },
    MobPart {
        size: [0.22, 0.75, 0.22],
        offset: [0.0, -0.375, 0.0],
        pivot: [-0.13, 0.75, 0.0],
        tex_cols: [14; 6],
        tex_row: 15,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::HuskBody,
        flags: 0,
    },
    MobPart {
        size: [0.22, 0.75, 0.22],
        offset: [0.0, -0.375, 0.0],
        pivot: [0.13, 0.75, 0.0],
        tex_cols: [14; 6],
        tex_row: 15,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::HuskBody,
        flags: 0,
    },
];

pub const BLAZE_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 1.5, 0.0],
        tex_cols: [6, 7, 7, 7, 7, 7],
        tex_row: 8,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 20,
    },
    MobPart {
        size: [0.34, 0.7, 0.34],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.92, 0.0],
        tex_cols: [7; 6],
        tex_row: 8,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 20,
    },
];

pub const SHULKER_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.95, 0.18, 0.95],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.09, 0.0],
        tex_cols: [9; 6],
        tex_row: 10,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.88, 0.36, 0.88],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.35, 0.0],
        tex_cols: [10; 6],
        tex_row: 10,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.34, 0.34, 0.34],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.63, 0.0],
        tex_cols: [9; 6],
        tex_row: 10,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.9, 0.36, 0.9],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.82, 0.0],
        tex_cols: [10; 6],
        tex_row: 10,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 8,
    },
];

pub const ENDERMAN_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.52, 0.52, 0.52],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 2.62, 0.0],
        tex_cols: [10, 14, 14, 14, 14, 14],
        tex_row: 8,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 16,
    },
    MobPart {
        size: [0.48, 0.82, 0.24],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 1.92, 0.0],
        tex_cols: [11; 6],
        tex_row: 8,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.14, 1.22, 0.14],
        offset: [0.0, -0.55, 0.0],
        pivot: [-0.32, 2.18, 0.0],
        tex_cols: [12; 6],
        tex_row: 8,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.14, 1.22, 0.14],
        offset: [0.0, -0.55, 0.0],
        pivot: [0.32, 2.18, 0.0],
        tex_cols: [12; 6],
        tex_row: 8,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.16, 1.48, 0.16],
        offset: [0.0, -0.74, 0.0],
        pivot: [-0.13, 1.48, 0.0],
        tex_cols: [12; 6],
        tex_row: 8,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.16, 1.48, 0.16],
        offset: [0.0, -0.74, 0.0],
        pivot: [0.13, 1.48, 0.0],
        tex_cols: [12; 6],
        tex_row: 8,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const ENDCRYSTAL_PARTS: &[MobPart] = &[
    MobPart {
        size: [1.25, 0.16, 1.25],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.12, 0.0],
        tex_cols: [3; 6],
        tex_row: 4,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.95, 0.16, 0.95],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.25, 0.0],
        tex_cols: [3; 6],
        tex_row: 4,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.65, 0.16, 0.65],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.38, 0.0],
        tex_cols: [3; 6],
        tex_row: 4,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.12, 0.85, 0.12],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.85, 0.0],
        tex_cols: [3; 6],
        tex_row: 4,
        limb: Limb::SpinYaw,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 48,
    },
    MobPart {
        size: [0.72, 0.72, 0.72],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 1.35, 0.0],
        tex_cols: [4; 6],
        tex_row: 4,
        limb: Limb::SpinYaw,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 16,
    },
    MobPart {
        size: [0.46, 0.46, 0.46],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 1.35, 0.0],
        tex_cols: [4; 6],
        tex_row: 4,
        limb: Limb::SpinYawNeg,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 16,
    },
];

pub const WITHERSKULL_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.3, 0.3, 0.3],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.0, 0.0],
        tex_cols: [8; 6],
        tex_row: 8,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::World,
        tex_mode: TexMode::Fixed,
        flags: 16,
    },
];

pub const DRAGONBREATH_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.22, 0.22, 0.22],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.0, 0.0],
        tex_cols: [6; 6],
        tex_row: 4,
        limb: Limb::Look,
        scale: PartScale::BreathPulse,
        pivot_mode: PivotMode::World,
        tex_mode: TexMode::Fixed,
        flags: 16,
    },
];

pub const HEARTPARTICLE_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.25, 0.25, 0.01],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.0, 0.0],
        tex_cols: [0; 6],
        tex_row: 8,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::World,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const REMOTEPLAYER_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.25, 0.0],
        pivot: [0.0, 1.4, 0.0],
        tex_cols: [15; 6],
        tex_row: 8,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::PlayerHead,
        flags: 0,
    },
    MobPart {
        size: [0.5, 0.75, 0.25],
        offset: [0.0, 0.375, 0.0],
        pivot: [0.0, 0.65, 0.0],
        tex_cols: [2; 6],
        tex_row: 9,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.25, 0.75, 0.25],
        offset: [0.0, -0.325, 0.0],
        pivot: [-0.375, 1.3, 0.0],
        tex_cols: [15; 6],
        tex_row: 9,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::PlayerArm,
        flags: 0,
    },
    MobPart {
        size: [0.25, 0.75, 0.25],
        offset: [0.0, -0.325, 0.0],
        pivot: [0.375, 1.3, 0.0],
        tex_cols: [15; 6],
        tex_row: 9,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::PlayerArm,
        flags: 0,
    },
    MobPart {
        size: [0.27, 0.28, 0.27],
        offset: [0.0, -0.09, 0.0],
        pivot: [-0.375, 1.3, 0.0],
        tex_cols: [2; 6],
        tex_row: 9,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.27, 0.28, 0.27],
        offset: [0.0, -0.09, 0.0],
        pivot: [0.375, 1.3, 0.0],
        tex_cols: [2; 6],
        tex_row: 9,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.25, 0.75, 0.25],
        offset: [0.0, -0.375, 0.0],
        pivot: [-0.125, 0.75, 0.0],
        tex_cols: [3; 6],
        tex_row: 9,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.25, 0.75, 0.25],
        offset: [0.0, -0.375, 0.0],
        pivot: [0.125, 0.75, 0.0],
        tex_cols: [3; 6],
        tex_row: 9,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const SPIDER_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.6, 0.5, 0.6],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.45, 0.4],
        tex_cols: [0, 1, 1, 1, 1, 1],
        tex_row: 11,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.9, 0.7, 0.9],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.5, -0.3],
        tex_cols: [1; 6],
        tex_row: 11,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const SLIME_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.25, 0.0],
        tex_cols: [2; 6],
        tex_row: 11,
        limb: Limb::Static,
        scale: PartScale::SlimeSize,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
];

pub const WITCH_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.25, 0.0],
        pivot: [0.0, 1.4, 0.0],
        tex_cols: [3; 6],
        tex_row: 11,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.6, 0.9, 0.35],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.65, 0.0],
        tex_cols: [4; 6],
        tex_row: 11,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const DROWNED_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.25, 0.0],
        pivot: [0.0, 1.4, 0.0],
        tex_cols: [5; 6],
        tex_row: 11,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.5, 0.75, 0.25],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.65, 0.0],
        tex_cols: [6; 6],
        tex_row: 11,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const GHAST_PARTS: &[MobPart] = &[
    MobPart {
        size: [3.5, 3.5, 3.5],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 2.0, 0.0],
        tex_cols: [7; 6],
        tex_row: 11,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const MAGMACUBE_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.25, 0.0],
        tex_cols: [8; 6],
        tex_row: 11,
        limb: Limb::Static,
        scale: PartScale::SlimeSize,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
];

pub const WITHERSKELETON_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 1.9, 0.0],
        tex_cols: [4, 5, 5, 5, 5, 5],
        tex_row: 9,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.45, 0.95, 0.22],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 1.1, 0.0],
        tex_cols: [5; 6],
        tex_row: 9,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const WOLF_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.4, 0.4, 0.4],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.0, 0.0],
        tex_cols: [9; 6],
        tex_row: 11,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::WolfHead,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.45, 0.45, 0.7],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.0, 0.0],
        tex_cols: [10; 6],
        tex_row: 11,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::WolfBody,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const CAT_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.35, 0.35, 0.35],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.6, 0.3],
        tex_cols: [11; 6],
        tex_row: 11,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.4, 0.35, 0.6],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.4, 0.0],
        tex_cols: [12; 6],
        tex_row: 11,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const HORSE_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.9, 0.5],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 1.3, 0.5],
        tex_cols: [13; 6],
        tex_row: 11,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.9, 0.9, 1.4],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.8, 0.0],
        tex_cols: [14; 6],
        tex_row: 11,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const BAT_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.3, 0.3, 0.3],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.3, 0.0],
        tex_cols: [15; 6],
        tex_row: 11,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const SQUID_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.7, 0.7, 0.7],
        offset: [0.0, 0.0, 0.0],
        pivot: [0.0, 0.5, 0.0],
        tex_cols: [0; 6],
        tex_row: 12,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const VILLAGER_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.25, 0.0],
        pivot: [0.0, 1.4, 0.0],
        tex_cols: [0, 1, 1, 1, 1, 1],
        tex_row: 8,
        limb: Limb::Look,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.5, 0.9, 0.35],
        offset: [0.0, 0.45, 0.0],
        pivot: [0.0, 0.5, 0.0],
        tex_cols: [3; 6],
        tex_row: 8,
        limb: Limb::Static,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.2, 0.5, 0.2],
        offset: [0.0, -0.25, 0.0],
        pivot: [-0.125, 0.5, 0.0],
        tex_cols: [4; 6],
        tex_row: 8,
        limb: Limb::Walk,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
    MobPart {
        size: [0.2, 0.5, 0.2],
        offset: [0.0, -0.25, 0.0],
        pivot: [0.125, 0.5, 0.0],
        tex_cols: [4; 6],
        tex_row: 8,
        limb: Limb::WalkOpp,
        scale: PartScale::Baby,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 2,
    },
];

pub const IRONGOLEM_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.6, 0.4],
        offset: [0.0, 0.3, 0.0],
        pivot: [0.0, 2.0, 0.0],
        tex_cols: [3; 6],
        tex_row: 3,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [1.1, 1.2, 0.6],
        offset: [0.0, 0.6, 0.0],
        pivot: [0.0, 0.9, 0.0],
        tex_cols: [3; 6],
        tex_row: 3,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.3, 1.4, 0.3],
        offset: [0.0, -0.6, 0.0],
        pivot: [-0.75, 1.9, 0.0],
        tex_cols: [3; 6],
        tex_row: 3,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.3, 1.4, 0.3],
        offset: [0.0, -0.6, 0.0],
        pivot: [0.75, 1.9, 0.0],
        tex_cols: [3; 6],
        tex_row: 3,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.35, 0.9, 0.35],
        offset: [0.0, -0.45, 0.0],
        pivot: [-0.3, 0.9, 0.0],
        tex_cols: [3; 6],
        tex_row: 3,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.35, 0.9, 0.35],
        offset: [0.0, -0.45, 0.0],
        pivot: [0.3, 0.9, 0.0],
        tex_cols: [3; 6],
        tex_row: 3,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const PILLAGER_PARTS: &[MobPart] = &[
    MobPart {
        size: [0.5, 0.5, 0.5],
        offset: [0.0, 0.25, 0.0],
        pivot: [0.0, 1.4, 0.0],
        tex_cols: [5; 6],
        tex_row: 9,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.5, 0.75, 0.3],
        offset: [0.0, 0.375, 0.0],
        pivot: [0.0, 0.65, 0.0],
        tex_cols: [5; 6],
        tex_row: 9,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.2, 0.75, 0.2],
        offset: [0.0, -0.375, 0.0],
        pivot: [-0.35, 1.3, 0.0],
        tex_cols: [5; 6],
        tex_row: 9,
        limb: Limb::Pitch(-0.5),
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.2, 0.75, 0.2],
        offset: [0.0, -0.375, 0.0],
        pivot: [0.35, 1.3, 0.0],
        tex_cols: [5; 6],
        tex_row: 9,
        limb: Limb::Pitch(-0.5),
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.2, 0.75, 0.2],
        offset: [0.0, -0.375, 0.0],
        pivot: [-0.125, 0.75, 0.0],
        tex_cols: [5; 6],
        tex_row: 9,
        limb: Limb::Walk,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.2, 0.75, 0.2],
        offset: [0.0, -0.375, 0.0],
        pivot: [0.125, 0.75, 0.0],
        tex_cols: [5; 6],
        tex_row: 9,
        limb: Limb::WalkOpp,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const RAVAGER_PARTS: &[MobPart] = &[
    MobPart {
        size: [1.4, 1.2, 1.6],
        offset: [0.0, 0.6, 0.0],
        pivot: [0.0, 0.8, 0.0],
        tex_cols: [6; 6],
        tex_row: 9,
        limb: Limb::Static,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
    MobPart {
        size: [0.8, 0.8, 0.9],
        offset: [0.0, 0.4, 0.45],
        pivot: [0.0, 1.2, 0.7],
        tex_cols: [6; 6],
        tex_row: 9,
        limb: Limb::Look,
        scale: PartScale::None,
        pivot_mode: PivotMode::Local,
        tex_mode: TexMode::Fixed,
        flags: 0,
    },
];

pub const EXPERIENCEORB_PARTS: &[MobPart] = &[
];

pub const BOAT_PARTS: &[MobPart] = &[
];

pub const MINECART_PARTS: &[MobPart] = &[
];

pub const FISHINGHOOK_PARTS: &[MobPart] = &[
];


pub fn parts_for(ty: EntityType) -> Option<&'static [MobPart]> {
    Some(match ty {
        EntityType::Zombie => ZOMBIE_PARTS,
        EntityType::Skeleton => SKELETON_PARTS,
        EntityType::Creeper => CREEPER_PARTS,
        EntityType::Arrow => ARROW_PARTS,
        EntityType::SplashPotion => SPLASHPOTION_PARTS,
        EntityType::Pig => PIG_PARTS,
        EntityType::Cow => COW_PARTS,
        EntityType::Sheep => SHEEP_PARTS,
        EntityType::Chicken => CHICKEN_PARTS,
        EntityType::Piglin => PIGLIN_PARTS,
        EntityType::Husk => HUSK_PARTS,
        EntityType::Blaze => BLAZE_PARTS,
        EntityType::Shulker => SHULKER_PARTS,
        EntityType::Enderman => ENDERMAN_PARTS,
        EntityType::EndCrystal => ENDCRYSTAL_PARTS,
        EntityType::WitherSkull => WITHERSKULL_PARTS,
        EntityType::DragonBreath => DRAGONBREATH_PARTS,
        EntityType::HeartParticle => HEARTPARTICLE_PARTS,
        EntityType::RemotePlayer => REMOTEPLAYER_PARTS,
        EntityType::Spider => SPIDER_PARTS,
        EntityType::Slime => SLIME_PARTS,
        EntityType::Witch => WITCH_PARTS,
        EntityType::Drowned => DROWNED_PARTS,
        EntityType::Ghast => GHAST_PARTS,
        EntityType::MagmaCube => MAGMACUBE_PARTS,
        EntityType::WitherSkeleton => WITHERSKELETON_PARTS,
        EntityType::Wolf => WOLF_PARTS,
        EntityType::Cat => CAT_PARTS,
        EntityType::Horse => HORSE_PARTS,
        EntityType::Bat => BAT_PARTS,
        EntityType::Squid => SQUID_PARTS,
        EntityType::Villager => VILLAGER_PARTS,
        EntityType::IronGolem => IRONGOLEM_PARTS,
        EntityType::Pillager => PILLAGER_PARTS,
        EntityType::Ravager => RAVAGER_PARTS,
        EntityType::ExperienceOrb => EXPERIENCEORB_PARTS,
        EntityType::Boat => BOAT_PARTS,
        EntityType::Minecart => MINECART_PARTS,
        EntityType::FishingHook => FISHINGHOOK_PARTS,
        // Custom emitters live in mob_renderer.
        EntityType::EnderDragon | EntityType::Wither | EntityType::DroppedItem => return None,
    })
}

struct AnimCtx<'a> {
    entity: &'a Entity,
    swing: f32,
    time: f32,
    light: f32,
}

fn limb_pitch(limb: Limb, ctx: &AnimCtx<'_>) -> f32 {
    let e = ctx.entity;
    match limb {
        Limb::Static => 0.0,
        Limb::Look => e.pitch,
        Limb::Walk => ctx.swing,
        Limb::WalkOpp => -ctx.swing,
        Limb::ZombieArm => -std::f32::consts::FRAC_PI_2,
        Limb::SkelLeftArm => {
            if e.target_player {
                -std::f32::consts::FRAC_PI_2 + e.pitch
            } else {
                -ctx.swing
            }
        }
        Limb::SkelRightArm => {
            if e.target_player {
                let draw = ((2.0f32 - e.action_cooldown) / 2.0f32).clamp(0.0, 1.0);
                -std::f32::consts::FRAC_PI_2 + e.pitch + 0.2 * (1.0 - draw)
            } else {
                ctx.swing
            }
        }
        Limb::Flap => {
            if e.velocity.y < 0.0 {
                (ctx.time * 40.0).sin() * 0.7
            } else {
                0.0
            }
        }
        Limb::NegFlap => -limb_pitch(Limb::Flap, ctx),
        Limb::GrazingLook => {
            if e.grass_eat_timer > 0.0 {
                std::f32::consts::FRAC_PI_4
            } else {
                e.pitch
            }
        }
        Limb::Pitch(v) => v,
        Limb::SpinYaw | Limb::SpinYawNeg => 0.0,
    }
}

fn part_yaw(limb: Limb, entity_yaw: f32, time: f32) -> f32 {
    match limb {
        Limb::SpinYaw => time * 1.7,
        Limb::SpinYawNeg => -time * 1.7 * 1.4,
        _ => entity_yaw,
    }
}

fn resolve_scale(scale: PartScale, entity: &Entity, time: f32) -> (f32, f32) {
    // returns (size_scale, pivot_scale)
    match scale {
        PartScale::None => (1.0, 1.0),
        PartScale::Baby => {
            let s = if entity.age < 0.0 { 0.5 } else { 1.0 };
            (s, s)
        }
        PartScale::BabyHead => {
            let body = if entity.age < 0.0 { 0.5 } else { 1.0 };
            let head = if entity.age < 0.0 { 0.75 } else { 1.0 };
            (head, body)
        }
        PartScale::CreeperSwell => {
            let s = if entity.is_ignited {
                let progress = ((1.5f32 - entity.action_cooldown) / 1.5f32).clamp(0.0, 1.0);
                1.0 + 0.15 * progress * (time * 35.0).sin().abs()
            } else {
                1.0
            };
            (s, 1.0)
        }
        PartScale::SlimeSize => {
            let s = entity.slime_size as f32;
            (s, s)
        }
        PartScale::BreathPulse => {
            let s = 0.85 + (time * 10.0).sin().abs() * 0.25;
            (s, 1.0)
        }
    }
}

fn resolve_cols(mode: TexMode, fallback: [u32; 6], entity: &Entity) -> [u32; 6] {
    match mode {
        TexMode::Fixed => fallback,
        TexMode::SheepBody => {
            let c = if entity.has_wool { 5 } else { 6 };
            [c; 6]
        }
        TexMode::PiglinHead => [11, 12, 12, 12, 12, 12],
        TexMode::PiglinBody => [12; 6],
        TexMode::HuskHead => [13, 14, 14, 14, 14, 14],
        TexMode::HuskBody => [14; 6],
        TexMode::PlayerHead => PLAYER_HEAD_COLS,
        TexMode::PlayerArm => [PLAYER_ARM_COL; 6],
    }
}

fn resolve_row(mode: TexMode, fallback: u32) -> u32 {
    match mode {
        TexMode::PlayerHead => PLAYER_HEAD_ROW,
        TexMode::PlayerArm => PLAYER_ARM_ROW,
        _ => fallback,
    }
}

/// Emit table-driven cuboids for one entity. Returns true when the type is
/// table-backed (including intentionally empty meshes).
pub fn emit_table_parts(
    entity: &Entity,
    cuboid_instances: &mut Vec<MobInstance>,
    to_world: &dyn Fn(Vec3) -> Vec3,
    swing: f32,
    time: f32,
    light_val: f32,
) -> bool {
    let Some(parts) = parts_for(entity.entity_type) else {
        return false;
    };
    let ctx = AnimCtx {
        entity,
        swing,
        time,
        light: light_val,
    };
    let blaze_hover = (time * 2.2).sin() * 0.08;
    let lid_gap = 0.08 + (time * 1.3).sin().abs() * 0.1;
    let wolf_body = if entity.is_sitting {
        Vec3::new(0.0, 0.4, 0.0)
    } else {
        Vec3::new(0.0, 0.55, 0.0)
    };

    for part in parts {
        if part.flags & FLAG_PIGLIN_ONLY != 0 && entity.entity_type != EntityType::Piglin {
            continue;
        }
        let (size_s, pivot_s) = resolve_scale(part.scale, entity, time);
        let mut size = Vec3::from(part.size) * size_s;
        let mut offset = Vec3::from(part.offset) * size_s;
        let mut pivot_local = Vec3::from(part.pivot);
        if part.flags & FLAG_PIVOT_SCALED != 0 {
            pivot_local *= pivot_s;
        }
        if part.flags & FLAG_BLAZE_HOVER != 0 {
            pivot_local.y += blaze_hover;
        }
        if part.flags & FLAG_SHULKER_LID != 0 {
            pivot_local.y += lid_gap;
        }
        let pivot = match part.pivot_mode {
            PivotMode::Local => to_world(pivot_local),
            PivotMode::World => entity.position,
            PivotMode::WolfBody => to_world(wolf_body),
            PivotMode::WolfHead => to_world(wolf_body + Vec3::new(0.0, 0.25, 0.35)),
        };
        let mut light = light_val;
        if part.flags & FLAG_MAX_LIGHT != 0 {
            light = light.max(255.0);
        }
        // End crystal secondary orb pitches approximate the former hard-coded values.
        let pitch = match (part.limb, entity.entity_type) {
            (Limb::SpinYaw, EntityType::EndCrystal) if part.size[0] >= 0.7 => 0.65,
            (Limb::SpinYawNeg, EntityType::EndCrystal) => -0.45,
            _ => limb_pitch(part.limb, &ctx),
        };
        let yaw = part_yaw(part.limb, entity.yaw, time);
        let cols = resolve_cols(part.tex_mode, part.tex_cols, entity);
        let row = resolve_row(part.tex_mode, part.tex_row);
        let _ = (&mut size, &mut offset); // keep mut for clarity
        add_cuboid(
            cuboid_instances,
            size,
            offset,
            pivot,
            yaw,
            pitch,
            cols,
            row,
            light,
        );
    }

    // Small animator post-passes that are awkward as static parts.
    match entity.entity_type {
        EntityType::Skeleton => emit_skeleton_bow(entity, cuboid_instances, to_world, time, light_val),
        EntityType::Blaze => emit_blaze_rods(entity, cuboid_instances, to_world, time, light_val),
        _ => {}
    }
    true
}

fn emit_skeleton_bow(
    entity: &Entity,
    cuboid_instances: &mut Vec<MobInstance>,
    to_world: &dyn Fn(Vec3) -> Vec3,
    _time: f32,
    light_val: f32,
) {
    let target = entity.target_player;
    let aim_pitch = if target { entity.pitch } else { 0.0 };
    let left_arm_pitch = if target {
        -std::f32::consts::FRAC_PI_2 + aim_pitch
    } else {
        0.0
    };
    let draw_progress = if target {
        ((2.0f32 - entity.action_cooldown) / 2.0f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let left_shoulder = Vec3::new(-0.275, 1.3, 0.0);
    let cos_lp = left_arm_pitch.cos();
    let sin_lp = left_arm_pitch.sin();
    let hand_rel = Vec3::new(0.0, -0.65 * cos_lp, -0.65 * sin_lp);
    let bow_pivot = to_world(left_shoulder + hand_rel);
    add_cuboid(cuboid_instances, Vec3::new(0.08, 0.25, 0.08), Vec3::ZERO, bow_pivot, entity.yaw, aim_pitch, [9; 6], 9, light_val);
    add_cuboid(cuboid_instances, Vec3::new(0.06, 0.35, 0.06), Vec3::new(0.0, 0.25, 0.04), bow_pivot, entity.yaw, aim_pitch, [9; 6], 9, light_val);
    add_cuboid(cuboid_instances, Vec3::new(0.06, 0.35, 0.06), Vec3::new(0.0, -0.25, 0.04), bow_pivot, entity.yaw, aim_pitch, [9; 6], 9, light_val);
    let string_offset_z = -0.04 - 0.25 * draw_progress;
    add_cuboid(cuboid_instances, Vec3::new(0.02, 0.85, 0.02), Vec3::new(0.0, 0.0, string_offset_z), bow_pivot, entity.yaw, aim_pitch, [10; 6], 9, light_val);
}

fn emit_blaze_rods(
    _entity: &Entity,
    cuboid_instances: &mut Vec<MobInstance>,
    to_world: &dyn Fn(Vec3) -> Vec3,
    time: f32,
    light_val: f32,
) {
    let hover = (time * 2.2).sin() * 0.08;
    let blaze_light = light_val.max(255.0);
    for ring in 0..2 {
        for rod in 0..4 {
            let direction = if ring == 0 { 1.0 } else { -1.0 };
            let angle = direction * time * 1.8
                + rod as f32 * std::f32::consts::FRAC_PI_2
                + ring as f32 * std::f32::consts::FRAC_PI_4;
            let radius = if ring == 0 { 0.62 } else { 0.48 };
            let y = if ring == 0 {
                1.18 + (angle * 2.0).sin() * 0.1
            } else {
                0.55 + (angle * 2.0).cos() * 0.1
            };
            add_cuboid(
                cuboid_instances,
                Vec3::new(0.12, 0.62, 0.12),
                Vec3::ZERO,
                to_world(Vec3::new(angle.cos() * radius, y + hover, angle.sin() * radius)),
                angle,
                0.0,
                [6; 6],
                8,
                blaze_light,
            );
        }
    }
}
