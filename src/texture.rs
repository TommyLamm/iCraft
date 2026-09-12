use crate::resources::ResourcePackManager;
use image::{Rgba, RgbaImage};
use wgpu::{Device, Queue, Sampler, Texture, TextureView};

pub struct TextureAtlas {
    #[allow(dead_code)] // Owned for GPU lifetime; accessed via `view` and `sampler`.
    pub texture: Texture,
    pub view: TextureView,
    pub sampler: Sampler,
}

/// Shared LCG used by every procedural tile painter.
fn next_rand(seed: &mut u32, min: i16, max: i16) -> i16 {
    let val = crate::rng::lcg32_short(seed);
    let diff = max - min;
    if diff <= 0 {
        return min;
    }
    min + (val as i16 % diff)
}

fn paint_debug_tile(img: &mut RgbaImage, col: u32, row: u32) {
    let r = ((col * 37 + row * 17) % 200 + 40) as u8;
    let g = ((col * 53 + row * 29) % 200 + 40) as u8;
    let b = ((col * 19 + row * 61) % 200 + 40) as u8;
    for y in 0..16u32 {
        for x in 0..16u32 {
            img.put_pixel(col * 16 + x, row * 16 + y, Rgba([r, g, b, 255]));
        }
    }
}


fn draw_redstone_torch(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let is_stick = x == 7 && (6..=13).contains(&y);
            let is_redstone = x == 7 && y == 5;
            let is_glow = (6..=8).contains(&x) && (2..=4).contains(&y);
            let color = if is_stick {
                [125, 78, 42, 255]
            } else if is_redstone {
                [120, 15, 20, 255]
            } else if is_glow {
                [245, 45, 35, 255]
            } else {
                [0, 0, 0, 0]
            };
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba(color));
        }
    }
}










fn draw_crack_pattern(img: &mut RgbaImage, tx: u32, ty: u32, stage: u32) {
    // Determine crack pattern density based on stage (0..10)
    // We draw random dark gray lines.
    let mut seed = 54321u32.wrapping_add(stage);

    // Background is transparent Rgba([0, 0, 0, 0])
    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
        }
    }

    // Number of crack lines scales with stage
    let num_lines = (stage + 1) * 2;
    for _ in 0..num_lines {
        let mut cx = next_rand(&mut seed, 0, 16) as i32;
        let mut cy = next_rand(&mut seed, 0, 16) as i32;
        let length = next_rand(&mut seed, 3, 8) as i32;
        for _ in 0..length {
            if cx >= 0 && cx < 16 && cy >= 0 && cy < 16 {
                img.put_pixel(
                    tx * 16 + cx as u32,
                    ty * 16 + cy as u32,
                    Rgba([20, 20, 20, 200]),
                ); // Dark grey crack line
            }
            cx += next_rand(&mut seed, -1, 2) as i32;
            cy += next_rand(&mut seed, -1, 2) as i32;
        }
    }
}
























/// A single 16x16 atlas tile replacement sourced from an iCraft resource pack
/// with the built-in assets as the final fallback.
struct PackTile {
    col: u32,
    row: u32,
    /// Path relative to the pack's `textures` directory, e.g. `block/stone.png`.
    path: &'static str,
    /// Optional sub-rectangle (x, y, width, height) cropped from a larger
    /// entity skin / chest texture before scaling to 16x16.
    region: Option<[u32; 4]>,
    /// Optional alpha multiplier applied after scaling; used to keep water and
    /// ice translucent the way the procedural atlas drew them.
    alpha: Option<f32>,
    /// Optional RGB multiplier (tint) applied after scaling. Used where the
    /// vanilla/pack texture is a grayscale template (grass, leaves, water)
    /// that Minecraft tints at render time, which this atlas cannot do.
    tint: Option<[f32; 3]>,
}

const fn pack_tile(col: u32, row: u32, path: &'static str) -> PackTile {
    PackTile {
        col,
        row,
        path,
        region: None,
        alpha: None,
        tint: None,
    }
}

const fn pack_tile_region(
    col: u32,
    row: u32,
    path: &'static str,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> PackTile {
    PackTile {
        col,
        row,
        path,
        region: Some([x, y, w, h]),
        alpha: None,
        tint: None,
    }
}

const fn pack_tile_alpha(col: u32, row: u32, path: &'static str, alpha: f32) -> PackTile {
    PackTile {
        col,
        row,
        path,
        region: None,
        alpha: Some(alpha),
        tint: None,
    }
}

const fn pack_tile_tint(col: u32, row: u32, path: &'static str, tint: [f32; 3]) -> PackTile {
    PackTile {
        col,
        row,
        path,
        region: None,
        alpha: None,
        tint: Some(tint),
    }
}

const fn pack_tile_region_tint(
    col: u32,
    row: u32,
    path: &'static str,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    tint: [f32; 3],
) -> PackTile {
    PackTile {
        col,
        row,
        path,
        region: Some([x, y, w, h]),
        alpha: None,
        tint: Some(tint),
    }
}

/// Built-in 1.21.5-compatible texture fallback shipped with the repository.
#[cfg(test)]
const VANILLA_TEXTURES_DIR: &str = "assets/vanilla/textures";

/// Atlas layout: (col, row) -> resource-pack texture path. Every entry is
/// resolved from the selected pack stack first and falls back to the extracted
/// vanilla tree, keeping the procedural drawing as the final fallback so the
/// game always has a complete atlas.
const PACK_TILES: &[PackTile] = &[
    // Row 0: terrain blocks
    // Grass/leaves/water are grayscale templates in the pack and vanilla jar;
    // Minecraft tints them per-biome at render time, so bake a green/blue tint.
    pack_tile_tint(0, 0, "block/grass_block_top.png", [0.62, 1.0, 0.45]),
    pack_tile(1, 0, "block/grass_block_side.png"),
    pack_tile(2, 0, "block/dirt.png"),
    pack_tile(3, 0, "block/stone.png"),
    pack_tile(4, 0, "block/sand.png"),
    pack_tile(5, 0, "block/gravel.png"),
    pack_tile(6, 0, "block/oak_planks.png"),
    pack_tile_tint(7, 0, "block/oak_leaves.png", [0.55, 1.0, 0.45]),
    pack_tile(8, 0, "block/cobblestone.png"),
    pack_tile(9, 0, "block/bedrock.png"),
    // water_still is an animated 16x512 sheet; crop frame 0 and tint it blue.
    pack_tile_region_tint(
        10,
        0,
        "block/water_still.png",
        0,
        0,
        16,
        16,
        [0.25, 0.45, 1.1],
    ),
    pack_tile(11, 0, "block/coal_ore.png"),
    pack_tile(12, 0, "block/iron_ore.png"),
    pack_tile(13, 0, "block/gold_ore.png"),
    pack_tile(14, 0, "block/diamond_ore.png"),
    pack_tile(15, 0, "block/redstone_ore.png"),
    // Row 1: more terrain / furniture
    pack_tile(0, 1, "block/glass.png"),
    pack_tile(1, 1, "block/bricks.png"),
    pack_tile(2, 1, "block/stone_bricks.png"),
    pack_tile(3, 1, "block/snow.png"),
    pack_tile(4, 1, "block/grass_block_snow.png"),
    pack_tile_alpha(5, 1, "block/ice.png", 180.0 / 255.0),
    pack_tile(6, 1, "block/clay.png"),
    pack_tile(7, 1, "block/sandstone_top.png"),
    pack_tile(8, 1, "block/sandstone.png"),
    pack_tile(9, 1, "block/obsidian.png"),
    pack_tile(10, 1, "block/oak_log_top.png"),
    pack_tile(11, 1, "block/oak_log.png"),
    pack_tile(12, 1, "block/crafting_table_top.png"),
    pack_tile(13, 1, "block/crafting_table_side.png"),
    pack_tile(14, 1, "block/furnace_front.png"),
    pack_tile_region(15, 1, "entity/chest/normal.png", 14, 14, 14, 14),
    // Row 2: mechanisms
    pack_tile(0, 2, "block/tnt_top.png"),
    pack_tile(1, 2, "block/tnt_bottom.png"),
    pack_tile(2, 2, "block/tnt_side.png"),
    pack_tile(3, 2, "block/bookshelf.png"),
    pack_tile(4, 2, "block/torch.png"),
    pack_tile(5, 2, "block/redstone_dust_dot.png"),
    pack_tile(6, 2, "block/redstone_torch.png"),
    pack_tile(7, 2, "block/repeater.png"),
    pack_tile(8, 2, "block/comparator.png"),
    // 1.21.5 stone buttons/plates reuse the stone texture.
    pack_tile(9, 2, "block/stone.png"),
    pack_tile(10, 2, "block/lever.png"),
    pack_tile(11, 2, "block/stone.png"),
    pack_tile(12, 2, "block/piston_side.png"),
    pack_tile(13, 2, "block/piston_top_sticky.png"),
    pack_tile(14, 2, "block/redstone_lamp.png"),
    // lava_still is an animated 16x512 sheet; use its first frame.
    pack_tile_region(15, 2, "block/lava_still.png", 0, 0, 16, 16),
    // Row 3: resource items
    pack_tile(0, 3, "item/stick.png"),
    pack_tile(1, 3, "item/coal.png"),
    pack_tile(2, 3, "item/iron_ingot.png"),
    pack_tile(3, 3, "item/gold_ingot.png"),
    pack_tile(4, 3, "item/diamond.png"),
    pack_tile(5, 3, "item/redstone.png"),
    pack_tile(6, 3, "item/apple.png"),
    pack_tile(7, 3, "item/bread.png"),
    pack_tile(8, 3, "item/rotten_flesh.png"),
    pack_tile(9, 3, "item/bone.png"),
    pack_tile(10, 3, "item/bow.png"),
    pack_tile(11, 3, "item/gunpowder.png"),
    pack_tile(12, 3, "item/wheat.png"),
    pack_tile(13, 3, "item/wheat_seeds.png"),
    pack_tile(14, 3, "item/carrot.png"),
    pack_tile(15, 3, "gui/sprites/hud/air.png"),
    // Row 4: swords and special items
    pack_tile(0, 4, "item/stone_sword.png"),
    pack_tile(1, 4, "item/iron_sword.png"),
    pack_tile(2, 4, "item/diamond_sword.png"),
    pack_tile(3, 4, "item/nether_star.png"),
    pack_tile(4, 4, "item/end_crystal.png"),
    pack_tile(5, 4, "item/blaze_rod.png"),
    pack_tile(6, 4, "block/end_portal_frame_eye.png"),
    pack_tile(7, 4, "block/coal_block.png"),
    pack_tile(8, 4, "block/gold_block.png"),
    // Vanilla's frame model maps only y=3..16 of this texture onto its
    // 13/16-high sides. Crop those transparent top rows here so compact-atlas
    // cube/item renderers get the same opaque result with their full-tile UVs.
    pack_tile_region(9, 4, "block/end_portal_frame_side.png", 0, 3, 16, 13),
    // Dedicated Ender Dragon material crops: body, neck/tail, head and wing.
    pack_tile_region(10, 4, "entity/enderdragon/dragon.png", 64, 64, 24, 24),
    pack_tile_region(11, 4, "entity/enderdragon/dragon.png", 122, 40, 10, 10),
    pack_tile_region(12, 4, "entity/enderdragon/dragon.png", 192, 60, 12, 5),
    pack_tile_region(13, 4, "entity/enderdragon/dragon.png", 0, 100, 112, 80),
    pack_tile(14, 4, "block/diamond_block.png"),
    // Rows 5-7: tools
    pack_tile(0, 5, "item/stone_pickaxe.png"),
    pack_tile(1, 5, "item/iron_pickaxe.png"),
    pack_tile(2, 5, "item/diamond_pickaxe.png"),
    pack_tile(0, 6, "item/stone_axe.png"),
    pack_tile(1, 6, "item/iron_axe.png"),
    pack_tile(2, 6, "item/diamond_axe.png"),
    pack_tile(0, 7, "item/stone_shovel.png"),
    pack_tile(1, 7, "item/iron_shovel.png"),
    pack_tile(2, 7, "item/diamond_shovel.png"),
    pack_tile_region(3, 7, "entity/sheep/sheep.png", 8, 8, 6, 6),
    pack_tile_region(4, 7, "entity/sheep/sheep.png", 0, 8, 8, 6),
    pack_tile_region(5, 7, "entity/sheep/sheep.png", 4, 20, 4, 6),
    // Row 8: HUD icons plus dedicated blaze/wither skins
    pack_tile(0, 8, "gui/sprites/hud/heart/full.png"),
    pack_tile(1, 8, "gui/sprites/hud/heart/half.png"),
    pack_tile(2, 8, "gui/sprites/hud/heart/container.png"),
    pack_tile(3, 8, "gui/sprites/hud/food_full_hunger.png"),
    pack_tile(4, 8, "gui/sprites/hud/food_half_hunger.png"),
    pack_tile(5, 8, "gui/sprites/hud/food_empty_hunger.png"),
    pack_tile_region(6, 8, "entity/blaze.png", 8, 8, 8, 8),
    pack_tile_region(7, 8, "entity/blaze.png", 12, 0, 8, 12),
    pack_tile_region(8, 8, "entity/wither/wither.png", 8, 8, 8, 8),
    pack_tile_region(9, 8, "entity/wither/wither.png", 20, 20, 8, 12),
    pack_tile_region(10, 8, "entity/enderman/enderman.png", 8, 8, 8, 8),
    pack_tile_region(11, 8, "entity/enderman/enderman.png", 20, 20, 8, 12),
    pack_tile_region(12, 8, "entity/enderman/enderman.png", 44, 20, 4, 12),
    pack_tile_region(14, 8, "entity/enderman/enderman.png", 0, 8, 8, 8),
    pack_tile_region(15, 8, "entity/player/wide/steve.png", 0, 8, 8, 8),
    // Row 9: hostile mob skins
    pack_tile_region(0, 9, "entity/zombie/zombie.png", 8, 8, 8, 8),
    pack_tile_region(1, 9, "entity/zombie/zombie.png", 0, 8, 8, 8),
    pack_tile_region(2, 9, "entity/zombie/zombie.png", 20, 20, 8, 12),
    pack_tile_region(3, 9, "entity/zombie/zombie.png", 4, 20, 4, 12),
    pack_tile_region(4, 9, "entity/skeleton/skeleton.png", 8, 8, 8, 8),
    // The vanilla skeleton body front is a see-through ribcage; use the solid
    // right-arm region instead so the torso cube reads as bone.
    pack_tile_region(5, 9, "entity/skeleton/skeleton.png", 40, 18, 8, 12),
    pack_tile_region(6, 9, "entity/creeper/creeper.png", 8, 8, 8, 8),
    pack_tile_region(7, 9, "entity/creeper/creeper.png", 20, 20, 8, 12),
    pack_tile(8, 9, "item/arrow.png"),
    pack_tile(9, 9, "block/oak_planks.png"),
    pack_tile(10, 9, "item/string.png"),
    pack_tile_region(11, 9, "entity/piglin/piglin.png", 8, 8, 8, 8),
    pack_tile_region(12, 9, "entity/piglin/piglin.png", 20, 20, 8, 12),
    pack_tile_region(13, 9, "entity/zombie/husk.png", 8, 8, 8, 8),
    pack_tile_region(14, 9, "entity/zombie/husk.png", 20, 20, 8, 12),
    // Row 10: passive mob skins and dimension blocks
    pack_tile_region(0, 10, "entity/pig/temperate_pig.png", 8, 8, 8, 8),
    pack_tile_region(1, 10, "entity/pig/temperate_pig.png", 8, 0, 8, 8),
    pack_tile_region(2, 10, "entity/cow/cow.png", 8, 6, 8, 8),
    pack_tile_region(3, 10, "entity/cow/cow.png", 18, 18, 12, 10),
    pack_tile_region(4, 10, "entity/sheep/sheep.png", 8, 6, 8, 8),
    pack_tile(5, 10, "block/white_wool.png"),
    pack_tile_region(6, 10, "entity/sheep/sheep.png", 8, 24, 8, 8),
    pack_tile_region(7, 10, "entity/chicken/temperate_chicken.png", 0, 4, 8, 4),
    pack_tile_region(8, 10, "entity/chicken/temperate_chicken.png", 0, 15, 8, 8),
    pack_tile(9, 10, "block/nether_bricks.png"),
    pack_tile_region(10, 10, "entity/chest/ender.png", 14, 14, 14, 14),
    pack_tile(11, 10, "item/flint_and_steel.png"),
    pack_tile(12, 10, "item/ender_eye.png"),
    pack_tile(13, 10, "item/elytra.png"),
    pack_tile(14, 10, "entity/end_portal.png"),
    pack_tile(15, 10, "block/purpur_block.png"),
    // Row 11: tools, food and boss items
    pack_tile(0, 11, "item/shears.png"),
    pack_tile(1, 11, "item/bucket.png"),
    pack_tile(2, 11, "item/milk_bucket.png"),
    pack_tile(3, 11, "item/porkchop.png"),
    pack_tile(4, 11, "item/beef.png"),
    pack_tile(5, 11, "item/mutton.png"),
    pack_tile(6, 11, "item/chicken.png"),
    pack_tile(7, 11, "item/cooked_porkchop.png"),
    pack_tile(8, 11, "item/cooked_beef.png"),
    pack_tile(9, 11, "item/cooked_mutton.png"),
    pack_tile(10, 11, "item/cooked_chicken.png"),
    pack_tile(11, 11, "item/leather.png"),
    pack_tile(12, 11, "item/feather.png"),
    pack_tile(13, 11, "item/egg.png"),
    pack_tile(14, 11, "block/dragon_egg.png"),
    pack_tile_region(15, 11, "entity/skeleton/wither_skeleton.png", 8, 8, 8, 8),
    // Row 12: tree variants, plants and fire
    pack_tile(0, 12, "block/birch_log_top.png"),
    pack_tile(1, 12, "block/birch_log.png"),
    pack_tile(2, 12, "block/birch_planks.png"),
    pack_tile(3, 12, "block/birch_leaves.png"),
    pack_tile(4, 12, "block/spruce_log_top.png"),
    pack_tile(5, 12, "block/spruce_log.png"),
    pack_tile(6, 12, "block/spruce_planks.png"),
    pack_tile(7, 12, "block/spruce_leaves.png"),
    pack_tile(8, 12, "block/short_grass.png"),
    pack_tile(9, 12, "block/dandelion.png"),
    pack_tile(10, 12, "block/poppy.png"),
    pack_tile(11, 12, "block/cactus_side.png"),
    pack_tile(12, 12, "block/sugar_cane.png"),
    pack_tile(13, 12, "block/pumpkin_side.png"),
    pack_tile(14, 12, "block/melon_side.png"),
    // fire_0 is an animated 16x512 sheet; use its first frame.
    pack_tile_region(15, 12, "block/fire_0.png", 0, 0, 16, 16),
    // Row 13: enchanting / brewing / potion ingredients
    pack_tile(0, 13, "block/enchanting_table_side.png"),
    pack_tile(1, 13, "item/brewing_stand.png"),
    pack_tile(2, 13, "block/anvil.png"),
    pack_tile(3, 13, "item/lapis_lazuli.png"),
    pack_tile(4, 13, "item/iron_helmet.png"),
    pack_tile(5, 13, "item/iron_chestplate.png"),
    pack_tile(6, 13, "item/iron_leggings.png"),
    pack_tile(7, 13, "item/iron_boots.png"),
    pack_tile(8, 13, "item/glass_bottle.png"),
    pack_tile(9, 13, "item/potion.png"),
    pack_tile(10, 13, "item/splash_potion.png"),
    pack_tile(11, 13, "item/nether_wart.png"),
    pack_tile(12, 13, "item/sugar.png"),
    pack_tile(13, 13, "item/blaze_powder.png"),
    pack_tile(14, 13, "item/glistering_melon_slice.png"),
    pack_tile(15, 13, "item/ghast_tear.png"),
    // Row 14: brewing results, mechanisms and materials
    pack_tile(0, 14, "item/golden_carrot.png"),
    pack_tile(1, 14, "item/fermented_spider_eye.png"),
    pack_tile(2, 14, "item/magma_cream.png"),
    pack_tile(3, 14, "item/pufferfish.png"),
    pack_tile(4, 14, "item/spider_eye.png"),
    pack_tile(5, 14, "item/glowstone_dust.png"),
    pack_tile(6, 14, "item/redstone.png"),
    pack_tile(7, 14, "item/arrow.png"),
    pack_tile(8, 14, "block/redstone_lamp_on.png"),
    pack_tile(9, 14, "item/oak_door.png"),
    pack_tile(10, 14, "block/oak_trapdoor.png"),
    pack_tile(11, 14, "block/dispenser_front.png"),
    pack_tile(12, 14, "block/dropper_front.png"),
    pack_tile(13, 14, "block/note_block.png"),
    pack_tile(14, 14, "item/shulker_shell.png"),
    pack_tile(15, 14, "block/iron_block.png"),
    // Row 15: destroy stages (0-9) then dimension blocks (10-15).
    pack_tile(0, 15, "block/destroy_stage_0.png"),
    pack_tile(1, 15, "block/destroy_stage_1.png"),
    pack_tile(2, 15, "block/destroy_stage_2.png"),
    pack_tile(3, 15, "block/destroy_stage_3.png"),
    pack_tile(4, 15, "block/destroy_stage_4.png"),
    pack_tile(5, 15, "block/destroy_stage_5.png"),
    pack_tile(6, 15, "block/destroy_stage_6.png"),
    pack_tile(7, 15, "block/destroy_stage_7.png"),
    pack_tile(8, 15, "block/destroy_stage_8.png"),
    pack_tile(9, 15, "block/destroy_stage_9.png"),
    pack_tile(10, 15, "block/netherrack.png"),
    pack_tile(11, 15, "block/soul_sand.png"),
    pack_tile(12, 15, "block/glowstone.png"),
    pack_tile(13, 15, "block/nether_portal.png"),
    pack_tile(14, 15, "block/end_stone.png"),
    pack_tile(15, 15, "block/end_portal_frame_top.png"),
];

/// Paste one 16x16 tile from a source image, cropping and nearest-neighbor
/// scaling it first when a region is specified.
fn paste_pack_tile(img: &mut RgbaImage, tile: &PackTile, src: &image::DynamicImage) {
    let cropped = match tile.region {
        Some([x, y, w, h]) => image::imageops::crop_imm(src, x, y, w, h).to_image(),
        None => src.to_rgba8(),
    };
    let scaled = image::imageops::resize(&cropped, 16, 16, image::imageops::FilterType::Nearest);
    let ox = tile.col * 16;
    let oy = tile.row * 16;
    for (dx, dy, pixel) in scaled.enumerate_pixels() {
        let mut px = *pixel;
        if let Some(alpha) = tile.alpha {
            px[3] = (px[3] as f32 * alpha).round() as u8;
        }
        if let Some(tint) = tile.tint {
            px[0] = (px[0] as f32 * tint[0]).round() as u8;
            px[1] = (px[1] as f32 * tint[1]).round() as u8;
            px[2] = (px[2] as f32 * tint[2]).round() as u8;
        }
        img.put_pixel(ox + dx, oy + dy, px);
    }
}

fn apply_resource_pack_with_manager(img: &mut RgbaImage, manager: &mut ResourcePackManager) {
    let mut pack_hits = 0usize;
    let mut misses = 0usize;
    for tile in PACK_TILES {
        let loaded = manager
            .resolve_texture(tile.path)
            .and_then(|bytes| image::load_from_memory(&bytes).ok());
        match loaded {
            Some(src) => {
                pack_hits += 1;
                paste_pack_tile(img, tile, &src);
            }
            None => {
                misses += 1;
                // PACK_TILES is the atlas definition; paint only on miss.
                if tile.row == 15 && tile.col < 10 {
                    draw_crack_pattern(img, tile.col, 15, tile.col);
                } else {
                    paint_debug_tile(img, tile.col, tile.row);
                }
            }
        }
    }
    eprintln!(
        "[texture] resource-pack atlas: {} resolved, {} paint-on-miss fallback",
        pack_hits, misses
    );
    for diagnostic in manager.take_diagnostics() {
        eprintln!("[texture] {}: {}", diagnostic.source, diagnostic.message);
    }
}

/// Restore the local/remote player skin tiles after the general resource-pack
/// pass. Player rendering uses atlas slots that do not overlap Piglin/Husk,
/// preventing the F5 avatar from inheriting a hostile-mob head.
fn compose_player_head_tiles_with_manager(img: &mut RgbaImage, manager: &mut ResourcePackManager) {
    const SKIN_PATH: &str = "entity/player/wide/steve.png";
    let Some(bytes) = manager.resolve_texture(SKIN_PATH) else {
        return;
    };
    let Ok(source) = image::load_from_memory(&bytes) else {
        return;
    };

    for tile in [
        pack_tile_region(15, 8, SKIN_PATH, 8, 8, 8, 8), // head front
        pack_tile_region(13, 8, SKIN_PATH, 24, 8, 8, 8), // head hair/back
        pack_tile_region(15, 9, SKIN_PATH, 44, 20, 4, 12), // right arm front
    ] {
        paste_pack_tile(img, &tile, &source);
    }
}

fn compose_enderman_eyes_with_manager(img: &mut RgbaImage, manager: &mut ResourcePackManager) {
    let Some(bytes) = manager.resolve_texture("entity/enderman/enderman_eyes.png") else {
        return;
    };
    let Ok(source) = image::load_from_memory(&bytes) else {
        return;
    };
    let crop = image::imageops::crop_imm(&source, 8, 8, 8, 8).to_image();
    let eyes = image::imageops::resize(&crop, 16, 16, image::imageops::FilterType::Nearest);
    for (x, y, overlay) in eyes.enumerate_pixels() {
        let base = *img.get_pixel(10 * 16 + x, 8 * 16 + y);
        let alpha = overlay[3] as f32 / 255.0;
        let blend = |channel: usize| {
            (overlay[channel] as f32 * alpha + base[channel] as f32 * (1.0 - alpha)).round() as u8
        };
        img.put_pixel(
            10 * 16 + x,
            8 * 16 + y,
            Rgba([blend(0), blend(1), blend(2), 255]),
        );
    }
}

fn make_enderman_tiles_opaque(img: &mut RgbaImage) {
    for (col, base) in [
        (10, [7u8, 5, 9]),
        (11, [9u8, 7, 11]),
        (12, [8u8, 6, 10]),
        (14, [7u8, 5, 9]),
    ] {
        for y in 0..16u32 {
            for x in 0..16u32 {
                let pixel = *img.get_pixel(col * 16 + x, 8 * 16 + y);
                let alpha = pixel[3] as f32 / 255.0;
                let blend = |channel: usize| {
                    (pixel[channel] as f32 * alpha + base[channel] as f32 * (1.0 - alpha)).round()
                        as u8
                };
                img.put_pixel(
                    col * 16 + x,
                    8 * 16 + y,
                    Rgba([blend(0), blend(1), blend(2), 255]),
                );
            }
        }
    }
}

/// Create the filled top tile. The Eye of Ender artwork has transparent
/// pixels, so using it directly would make the underlying frame disappear.
fn compose_end_portal_frame_tiles(img: &mut RgbaImage) {
    const TOP: (u32, u32) = (15, 15);
    const FILLED_TOP: (u32, u32) = (6, 4);

    let mut top = [[Rgba([0, 0, 0, 0]); 16]; 16];
    let mut eye = [[Rgba([0, 0, 0, 0]); 16]; 16];
    for y in 0..16u32 {
        for x in 0..16u32 {
            top[y as usize][x as usize] = *img.get_pixel(TOP.0 * 16 + x, TOP.1 * 16 + y);
            eye[y as usize][x as usize] =
                *img.get_pixel(FILLED_TOP.0 * 16 + x, FILLED_TOP.1 * 16 + y);
        }
    }

    for y in 0..16u32 {
        for x in 0..16u32 {
            let base = top[y as usize][x as usize];
            let overlay = eye[y as usize][x as usize];
            let alpha = overlay[3] as f32 / 255.0;
            let blend = |channel: usize| {
                (overlay[channel] as f32 * alpha + base[channel] as f32 * (1.0 - alpha)).round()
                    as u8
            };
            img.put_pixel(
                FILLED_TOP.0 * 16 + x,
                FILLED_TOP.1 * 16 + y,
                Rgba([blend(0), blend(1), blend(2), 255]),
            );
        }
    }
}

/// Vanilla's dragon skin is a sparse UV sheet with transparent space between
/// model parts. Compact atlas crops must be composited over an opaque scale
/// color or those unused UV pixels become holes in our simplified cuboids.
fn make_dragon_tiles_opaque(img: &mut RgbaImage) {
    for (col, base) in [
        (10, [24u8, 20, 30]),
        (11, [30u8, 24, 38]),
        (12, [18u8, 14, 24]),
        (13, [36u8, 28, 44]),
    ] {
        for y in 0..16u32 {
            for x in 0..16u32 {
                let pixel = *img.get_pixel(col * 16 + x, 4 * 16 + y);
                let alpha = pixel[3] as f32 / 255.0;
                let blend = |channel: usize| {
                    (pixel[channel] as f32 * alpha + base[channel] as f32 * (1.0 - alpha)).round()
                        as u8
                };
                img.put_pixel(
                    col * 16 + x,
                    4 * 16 + y,
                    Rgba([blend(0), blend(1), blend(2), 255]),
                );
            }
        }
    }
}

impl TextureAtlas {
    /// Build the compact atlas while resolving assets through an already
    /// configured pack manager. Keeping the manager at the call site lets the
    /// menu/state selection apply consistently to textures and audio.
    pub fn new_procedural_with_manager(
        device: &Device,
        queue: &Queue,
        manager: &mut ResourcePackManager,
    ) -> Self {
        let build_started = std::time::Instant::now();
        // PACK_TILES is the atlas definition. Start empty and paint only when
        // a pack/vanilla tile misses (Plan 22 paint-on-miss).
        let mut img = RgbaImage::new(256, 256);

        apply_resource_pack_with_manager(&mut img, manager);
        compose_player_head_tiles_with_manager(&mut img, manager);
        compose_enderman_eyes_with_manager(&mut img, manager);
        make_enderman_tiles_opaque(&mut img);
        make_dragon_tiles_opaque(&mut img);
        compose_end_portal_frame_tiles(&mut img);

        eprintln!(
            "[texture] atlas build (paint-on-miss) took {:.2?} (PACK_TILES={})",
            build_started.elapsed(),
            PACK_TILES.len()
        );

        // Save to assets folder
        let _ = std::fs::create_dir_all("assets");
        let _ = img.save("assets/texture_atlas.png");

        let dimensions = img.dimensions();
        let size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Texture Atlas"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * dimensions.0),
                rows_per_image: Some(dimensions.1),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            texture,
            view,
            sampler,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    #[test]
    fn filled_end_portal_frame_composites_eye_over_opaque_frame() {
        let mut image = RgbaImage::new(256, 256);
        for y in 0..16 {
            for x in 0..16 {
                image.put_pixel(15 * 16 + x, 15 * 16 + y, Rgba([80, 120, 90, 255]));
                image.put_pixel(6 * 16 + x, 4 * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
        image.put_pixel(6 * 16 + 8, 4 * 16 + 8, Rgba([20, 220, 80, 255]));

        compose_end_portal_frame_tiles(&mut image);

        assert_eq!(image.get_pixel(6 * 16, 4 * 16).0, [80, 120, 90, 255]);
        assert_eq!(
            image.get_pixel(6 * 16 + 8, 4 * 16 + 8).0,
            [20, 220, 80, 255]
        );
    }

    #[test]
    fn redstone_torch_sprite_has_transparent_background_and_thin_artwork() {
        let mut image = RgbaImage::new(16, 16);
        draw_redstone_torch(&mut image, 0, 0);

        assert_eq!(image.get_pixel(0, 0).0[3], 0);
        assert_eq!(image.get_pixel(15, 15).0[3], 0);

        let opaque_pixels: Vec<(u32, u32)> = image
            .enumerate_pixels()
            .filter_map(|(x, y, pixel)| (pixel.0[3] != 0).then_some((x, y)))
            .collect();
        assert!(!opaque_pixels.is_empty());
        assert!(opaque_pixels
            .iter()
            .all(|&(x, y)| (6..=8).contains(&x) && (2..=13).contains(&y)));
        assert!(
            opaque_pixels.len() < 16 * 4,
            "redstone torch tile must remain mostly transparent"
        );
    }

    #[test]
    fn resource_pack_tiles_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for tile in PACK_TILES {
            assert!(
                seen.insert((tile.col, tile.row)),
                "duplicate resource-pack tile ({}, {}) for {}",
                tile.col,
                tile.row,
                tile.path
            );
        }
        assert!(!PACK_TILES.is_empty());
    }

    #[test]
    fn vanilla_end_portal_frame_side_texture_is_available() {
        let path =
            std::path::Path::new(VANILLA_TEXTURES_DIR).join("block/end_portal_frame_side.png");
        let image = image::open(&path).expect("End Portal Frame side texture must load");
        assert_eq!(image.dimensions(), (16, 16));
    }

    #[test]
    fn vanilla_ender_dragon_texture_is_available() {
        let path = std::path::Path::new(VANILLA_TEXTURES_DIR).join("entity/enderdragon/dragon.png");
        let image = image::open(&path).expect("Ender Dragon texture must load");
        assert_eq!(image.dimensions(), (256, 256));
    }

    #[test]
    fn vanilla_enderman_textures_are_available() {
        for name in ["enderman.png", "enderman_eyes.png"] {
            let path = std::path::Path::new(VANILLA_TEXTURES_DIR)
                .join("entity/enderman")
                .join(name);
            let image = image::open(&path).expect("Enderman texture must load");
            assert_eq!(image.dimensions(), (64, 32));
        }
    }

    #[test]
    fn resource_pack_atlas_applies_real_textures() {
        let mut manager = ResourcePackManager::discover_default();
        if manager.read_asset("block/stone.png").is_none()
            && !std::path::Path::new(VANILLA_TEXTURES_DIR).is_dir()
        {
            eprintln!("resource pack and vanilla textures unavailable; skipping");
            return;
        }
        let mut img = RgbaImage::new(256, 256);
        apply_resource_pack_with_manager(&mut img, &mut manager);
        compose_player_head_tiles_with_manager(&mut img, &mut manager);
        compose_enderman_eyes_with_manager(&mut img, &mut manager);
        make_enderman_tiles_opaque(&mut img);
        make_dragon_tiles_opaque(&mut img);
        compose_end_portal_frame_tiles(&mut img);

        // Stone tile must be fully opaque (opaque block texture).
        assert_eq!(img.get_pixel(3 * 16 + 8, 8).0[3], 255);
        // Water stays translucent (vanilla water_still already carries some
        // per-pixel transparency, multiplied by the 150/255 tile alpha).
        let water_alpha = img.get_pixel(10 * 16 + 8, 8).0[3];
        assert!(
            water_alpha > 0 && water_alpha < 255,
            "water tile is translucent"
        );
        // Item icons keep their transparent backgrounds.
        let mut stick_tile = Vec::new();
        for y in 0..16u32 {
            for x in 0..16u32 {
                stick_tile.push(img.get_pixel(x, 3 * 16 + y).0[3]);
            }
        }
        assert!(
            stick_tile.iter().any(|&a| a == 0),
            "stick icon has transparent pixels"
        );
        assert!(
            stick_tile.iter().any(|&a| a > 0),
            "stick icon has visible pixels"
        );

        for (col, row, label) in [
            (15, 15, "frame top"),
            (9, 4, "frame side"),
            (6, 4, "filled frame top"),
        ] {
            assert!(
                (0..16u32)
                    .all(|y| (0..16u32)
                        .all(|x| img.get_pixel(col * 16 + x, row * 16 + y).0[3] == 255)),
                "{label} must be fully opaque in the compact atlas"
            );
        }

        for col in 10..=13u32 {
            assert!(
                (0..16u32).all(
                    |y| (0..16u32).all(|x| img.get_pixel(col * 16 + x, 4 * 16 + y).0[3] == 255)
                ),
                "dragon atlas tile {col} must not contain transparent holes"
            );
        }

        let purple_eye_pixels = (0..16u32)
            .flat_map(|y| (0..16u32).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let pixel = img.get_pixel(10 * 16 + x, 8 * 16 + y).0;
                pixel[0] > 40 && pixel[2] > 40 && pixel[2] >= pixel[1]
            })
            .count();
        assert!(
            purple_eye_pixels > 0,
            "Enderman head atlas tile must contain the purple eye layer"
        );
        for col in [10u32, 11, 12, 14] {
            assert!(
                (0..16u32).all(
                    |y| (0..16u32).all(|x| img.get_pixel(col * 16 + x, 8 * 16 + y).0[3] == 255)
                ),
                "Enderman atlas tile {col} must be fully opaque"
            );
        }

        let preview = std::env::temp_dir().join("icraft_atlas_preview.png");
        let _ = img.save(&preview);
        eprintln!("atlas preview saved to {}", preview.display());
    }

    #[test]
    fn paint_on_miss_atlas_build_is_pack_first() {
        let mut manager = ResourcePackManager::discover_default();
        if manager.read_asset("block/stone.png").is_none()
            && !std::path::Path::new(VANILLA_TEXTURES_DIR).is_dir()
        {
            eprintln!("resource pack and vanilla textures unavailable; skipping");
            return;
        }
        let started = std::time::Instant::now();
        let mut img = RgbaImage::new(256, 256);
        apply_resource_pack_with_manager(&mut img, &mut manager);
        let elapsed = started.elapsed();
        // Baseline before Plan 22 painted every tile procedurally then overwrote
        // with PACK_TILES (full 256x256 raster twice). Paint-on-miss only walks
        // PACK_TILES once; record the wall time for the plan evidence section.
        eprintln!(
            "[texture] Plan22 paint-on-miss apply took {:.2?} over {} PACK_TILES",
            elapsed,
            PACK_TILES.len()
        );
        assert!(elapsed.as_secs() < 5, "atlas apply unexpectedly slow: {elapsed:?}");
        assert_eq!(img.get_pixel(3 * 16 + 8, 0).0[3], 255);
    }
}
