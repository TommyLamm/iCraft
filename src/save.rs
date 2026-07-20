use crate::inventory::{GameMode, Inventory, Item, ItemStack};
use crate::world::{BlockType, Chunk};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LevelData {
    pub seed: u32,
    pub time: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ItemStackData {
    pub item: Item,
    pub count: u32,
    pub durability: u32,
    pub enchantments: crate::enchantment::EnchantmentSet,
    pub potion: Option<crate::brewing::PotionData>,
    pub custom_name: crate::enchantment::ItemName,
}

impl ItemStackData {
    pub fn to_item_stack(&self) -> ItemStack {
        ItemStack {
            item: self.item,
            count: self.count,
            durability: self.durability,
            enchantments: self.enchantments,
            potion: self.potion,
            custom_name: self.custom_name,
        }
    }
}

impl From<&ItemStack> for ItemStackData {
    fn from(stack: &ItemStack) -> Self {
        Self {
            item: stack.item,
            count: stack.count,
            durability: stack.durability,
            enchantments: stack.enchantments,
            potion: stack.potion,
            custom_name: stack.custom_name,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InventoryData {
    pub hotbar: Vec<Option<ItemStackData>>,
    pub main: Vec<Option<ItemStackData>>,
    pub armor: Vec<Option<ItemStackData>>,
    pub selected: usize,
}

impl From<&Inventory> for InventoryData {
    fn from(inv: &Inventory) -> Self {
        Self {
            hotbar: inv
                .hotbar
                .iter()
                .map(|o| o.as_ref().map(|s| ItemStackData::from(s)))
                .collect(),
            main: inv
                .main
                .iter()
                .map(|o| o.as_ref().map(|s| ItemStackData::from(s)))
                .collect(),
            armor: inv
                .armor
                .iter()
                .map(|o| o.as_ref().map(|s| ItemStackData::from(s)))
                .collect(),
            selected: inv.selected,
        }
    }
}

impl InventoryData {
    pub fn to_inventory(&self) -> Inventory {
        let mut inv = Inventory::new();
        for (i, opt) in self.hotbar.iter().enumerate() {
            if i < inv.hotbar.len() {
                inv.hotbar[i] = opt.as_ref().map(|s| s.to_item_stack());
            }
        }
        for (i, opt) in self.main.iter().enumerate() {
            if i < inv.main.len() {
                inv.main[i] = opt.as_ref().map(|s| s.to_item_stack());
            }
        }
        for (i, opt) in self.armor.iter().enumerate() {
            if i < inv.armor.len() {
                inv.armor[i] = opt.as_ref().map(|s| s.to_item_stack());
            }
        }
        inv.selected = self.selected;
        inv
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlayerData {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    pub hunger: f32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub oxygen: f32,
    pub experience: u32,
    pub experience_level: u32,
    pub game_mode: GameMode,
    pub inventory: InventoryData,
}

impl PlayerData {
    pub fn from_state(
        position: glam::Vec3,
        velocity: glam::Vec3,
        yaw: f32,
        pitch: f32,
        state: &crate::player::PlayerState,
        game_mode: GameMode,
        inventory: &Inventory,
    ) -> Self {
        Self {
            position: [position.x, position.y, position.z],
            velocity: [velocity.x, velocity.y, velocity.z],
            yaw,
            pitch,
            health: state.health,
            hunger: state.hunger,
            saturation: state.saturation,
            exhaustion: state.exhaustion,
            oxygen: state.oxygen,
            experience: state.experience,
            experience_level: state.experience_level,
            game_mode,
            inventory: InventoryData::from(inventory),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChunkSaveData {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub blocks: Vec<u8>,       // Zlib compressed u8 array of BlockType
    pub sky_light: Vec<u8>,    // Zlib compressed u8 array of sky light
    pub block_light: Vec<u8>,  // Zlib compressed u8 array of block light
    pub fluid_levels: Vec<u8>, // Zlib compressed u8 array of fluid levels
}

impl ChunkSaveData {
    pub fn from_chunk(chunk: &Chunk) -> Self {
        let mut blocks = Vec::with_capacity(16 * 256 * 16);
        let mut sky_light = Vec::with_capacity(16 * 256 * 16);
        let mut block_light = Vec::with_capacity(16 * 256 * 16);
        let mut fluid_levels = Vec::with_capacity(16 * 256 * 16);

        for x in 0..16 {
            for y in 0..256 {
                for z in 0..16 {
                    blocks.push(chunk.blocks[x][y][z] as u8);
                    sky_light.push(chunk.sky_light[x][y][z]);
                    block_light.push(chunk.block_light[x][y][z]);
                    fluid_levels.push(chunk.fluid_levels[x][y][z]);
                }
            }
        }

        Self {
            chunk_x: chunk.chunk_x,
            chunk_z: chunk.chunk_z,
            blocks: compress_bytes(&blocks).unwrap_or_default(),
            sky_light: compress_bytes(&sky_light).unwrap_or_default(),
            block_light: compress_bytes(&block_light).unwrap_or_default(),
            fluid_levels: compress_bytes(&fluid_levels).unwrap_or_default(),
        }
    }

    pub fn restore_to_chunk(&self, chunk: &mut Chunk) {
        let blocks = decompress_bytes(&self.blocks).unwrap_or_default();
        let sky_light = decompress_bytes(&self.sky_light).unwrap_or_default();
        let block_light = decompress_bytes(&self.block_light).unwrap_or_default();
        let fluid_levels = decompress_bytes(&self.fluid_levels).unwrap_or_default();

        if blocks.len() == 16 * 256 * 16 {
            let mut idx = 0;
            for x in 0..16 {
                for y in 0..256 {
                    for z in 0..16 {
                        chunk.blocks[x][y][z] = BlockType::from_u8(blocks[idx]);
                        idx += 1;
                    }
                }
            }
        }

        if sky_light.len() == 16 * 256 * 16 {
            let mut idx = 0;
            for x in 0..16 {
                for y in 0..256 {
                    for z in 0..16 {
                        chunk.sky_light[x][y][z] = sky_light[idx];
                        idx += 1;
                    }
                }
            }
        }

        if block_light.len() == 16 * 256 * 16 {
            let mut idx = 0;
            for x in 0..16 {
                for y in 0..256 {
                    for z in 0..16 {
                        chunk.block_light[x][y][z] = block_light[idx];
                        idx += 1;
                    }
                }
            }
        }

        if fluid_levels.len() == 16 * 256 * 16 {
            let mut idx = 0;
            for x in 0..16 {
                for y in 0..256 {
                    for z in 0..16 {
                        chunk.fluid_levels[x][y][z] = fluid_levels[idx];
                        idx += 1;
                    }
                }
            }
        }

        for x in 0..16 {
            for z in 0..16 {
                chunk.update_heightmap(x, z);
            }
        }
    }
}

pub enum SaveCommand {
    SaveChunk {
        dimension: crate::dimension::Dimension,
        data: ChunkSaveData,
    },
    SaveLevelAndPlayer(LevelData, PlayerData),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegionData {
    /// Maps local coordinate (0..32, 0..32) -> Bincode serialized ChunkSaveData bytes
    pub chunks: HashMap<(u8, u8), Vec<u8>>,
}

pub struct SaveManager {
    pub world_dir: PathBuf,
    region_cache: HashMap<(crate::dimension::Dimension, i32, i32), RegionData>,
}

pub fn compress_bytes(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

pub fn decompress_bytes(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(data);
    let mut result = Vec::new();
    decoder.read_to_end(&mut result)?;
    Ok(result)
}

impl SaveManager {
    pub fn new<P: AsRef<Path>>(world_dir: P) -> Self {
        let world_dir = world_dir.as_ref().to_path_buf();
        let regions_dir = world_dir.join("regions");
        if !regions_dir.exists() {
            fs::create_dir_all(&regions_dir).unwrap();
        }
        for name in ["nether", "end"] {
            let path = world_dir.join("dimensions").join(name).join("regions");
            if !path.exists() {
                fs::create_dir_all(path).unwrap();
            }
        }
        Self {
            world_dir,
            region_cache: HashMap::new(),
        }
    }

    fn region_dir(&self, dimension: crate::dimension::Dimension) -> PathBuf {
        match dimension {
            crate::dimension::Dimension::Overworld => self.world_dir.join("regions"),
            crate::dimension::Dimension::Nether => self
                .world_dir
                .join("dimensions")
                .join("nether")
                .join("regions"),
            crate::dimension::Dimension::End => self
                .world_dir
                .join("dimensions")
                .join("end")
                .join("regions"),
        }
    }

    pub fn load_chunk(&mut self, cx: i32, cz: i32) -> Option<ChunkSaveData> {
        self.load_chunk_in(crate::dimension::Dimension::Overworld, cx, cz)
    }

    pub fn load_chunk_in(
        &mut self,
        dimension: crate::dimension::Dimension,
        cx: i32,
        cz: i32,
    ) -> Option<ChunkSaveData> {
        let rx = cx.div_euclid(32);
        let rz = cz.div_euclid(32);
        let lx = cx.rem_euclid(32) as u8;
        let lz = cz.rem_euclid(32) as u8;
        let region_file = self
            .region_dir(dimension)
            .join(format!("r.{}.{}.bin", rx, rz));

        if !self.region_cache.contains_key(&(dimension, rx, rz)) {
            if region_file.exists() {
                if let Ok(mut file) = File::open(&region_file) {
                    let mut bytes = Vec::new();
                    if file.read_to_end(&mut bytes).is_ok() {
                        if let Ok(region_data) = bincode::deserialize::<RegionData>(&bytes) {
                            self.region_cache.insert((dimension, rx, rz), region_data);
                        }
                    }
                }
            }
        }

        let region = self
            .region_cache
            .entry((dimension, rx, rz))
            .or_insert_with(|| RegionData {
                chunks: HashMap::new(),
            });

        if let Some(chunk_bytes) = region.chunks.get(&(lx, lz)) {
            bincode::deserialize::<ChunkSaveData>(chunk_bytes).ok()
        } else {
            None
        }
    }

    pub fn save_chunk(&mut self, cx: i32, cz: i32, data: ChunkSaveData) -> io::Result<()> {
        self.save_chunk_in(crate::dimension::Dimension::Overworld, cx, cz, data)
    }

    pub fn save_chunk_in(
        &mut self,
        dimension: crate::dimension::Dimension,
        cx: i32,
        cz: i32,
        data: ChunkSaveData,
    ) -> io::Result<()> {
        let rx = cx.div_euclid(32);
        let rz = cz.div_euclid(32);
        let lx = cx.rem_euclid(32) as u8;
        let lz = cz.rem_euclid(32) as u8;
        let region_file = self
            .region_dir(dimension)
            .join(format!("r.{}.{}.bin", rx, rz));

        if !self.region_cache.contains_key(&(dimension, rx, rz)) {
            if region_file.exists() {
                if let Ok(mut file) = File::open(&region_file) {
                    let mut bytes = Vec::new();
                    if file.read_to_end(&mut bytes).is_ok() {
                        if let Ok(region_data) = bincode::deserialize::<RegionData>(&bytes) {
                            self.region_cache.insert((dimension, rx, rz), region_data);
                        }
                    }
                }
            }
        }

        let region = self
            .region_cache
            .entry((dimension, rx, rz))
            .or_insert_with(|| RegionData {
                chunks: HashMap::new(),
            });

        let serialized_chunk =
            bincode::serialize(&data).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        region.chunks.insert((lx, lz), serialized_chunk);

        let serialized_region =
            bincode::serialize(region).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let mut file = File::create(&region_file)?;
        file.write_all(&serialized_region)?;
        Ok(())
    }

    pub fn save_current_dimension(&self, dimension: crate::dimension::Dimension) -> io::Result<()> {
        fs::write(self.world_dir.join("dimension.dat"), [dimension as u8])
    }

    pub fn load_current_dimension(&self) -> crate::dimension::Dimension {
        match fs::read(self.world_dir.join("dimension.dat"))
            .ok()
            .and_then(|bytes| bytes.first().copied())
        {
            Some(1) => crate::dimension::Dimension::Nether,
            Some(2) => crate::dimension::Dimension::End,
            _ => crate::dimension::Dimension::Overworld,
        }
    }

    pub fn save_player_and_level(&self, level: &LevelData, player: &PlayerData) -> io::Result<()> {
        let level_file = self.world_dir.join("level.dat");
        let player_file = self.world_dir.join("player.dat");

        let serialized_level =
            bincode::serialize(level).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let serialized_player =
            bincode::serialize(player).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let mut lf = File::create(&level_file)?;
        lf.write_all(&serialized_level)?;

        let mut pf = File::create(&player_file)?;
        pf.write_all(&serialized_player)?;

        Ok(())
    }

    pub fn load_player_and_level(&self) -> io::Result<(LevelData, PlayerData)> {
        let level_file = self.world_dir.join("level.dat");
        let player_file = self.world_dir.join("player.dat");

        let mut lf = File::open(&level_file)?;
        let mut level_bytes = Vec::new();
        lf.read_to_end(&mut level_bytes)?;
        let level = bincode::deserialize::<LevelData>(&level_bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let mut pf = File::open(&player_file)?;
        let mut player_bytes = Vec::new();
        pf.read_to_end(&mut player_bytes)?;
        let player = match bincode::deserialize::<PlayerData>(&player_bytes) {
            Ok(player) => player,
            Err(current_error) => {
                // Task 21 extended stack/player metadata. Keep worlds written by
                // the pre-enchanting schema loadable by upgrading in memory.
                bincode::deserialize::<LegacyPlayerData>(&player_bytes)
                    .map(PlayerData::from)
                    .map_err(|_| io::Error::new(io::ErrorKind::Other, current_error))?
            }
        };

        Ok((level, player))
    }
}

#[derive(Deserialize)]
struct LegacyItemStackData {
    item: Item,
    count: u32,
    durability: u32,
}

#[derive(Deserialize)]
struct LegacyInventoryData {
    hotbar: Vec<Option<LegacyItemStackData>>,
    main: Vec<Option<LegacyItemStackData>>,
    armor: Vec<Option<LegacyItemStackData>>,
    selected: usize,
}

#[derive(Deserialize)]
struct LegacyPlayerData {
    position: [f32; 3],
    velocity: [f32; 3],
    yaw: f32,
    pitch: f32,
    health: f32,
    hunger: f32,
    saturation: f32,
    exhaustion: f32,
    oxygen: f32,
    game_mode: GameMode,
    inventory: LegacyInventoryData,
}

impl From<LegacyItemStackData> for ItemStackData {
    fn from(old: LegacyItemStackData) -> Self {
        Self {
            item: old.item,
            count: old.count,
            durability: old.durability,
            enchantments: Default::default(),
            potion: None,
            custom_name: Default::default(),
        }
    }
}

impl From<LegacyInventoryData> for InventoryData {
    fn from(old: LegacyInventoryData) -> Self {
        let upgrade = |items: Vec<Option<LegacyItemStackData>>| {
            items
                .into_iter()
                .map(|stack| stack.map(Into::into))
                .collect()
        };
        Self {
            hotbar: upgrade(old.hotbar),
            main: upgrade(old.main),
            armor: upgrade(old.armor),
            selected: old.selected,
        }
    }
}

impl From<LegacyPlayerData> for PlayerData {
    fn from(old: LegacyPlayerData) -> Self {
        Self {
            position: old.position,
            velocity: old.velocity,
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            hunger: old.hunger,
            saturation: old.saturation,
            exhaustion: old.exhaustion,
            oxygen: old.oxygen,
            experience: 0,
            experience_level: 0,
            game_mode: old.game_mode,
            inventory: old.inventory.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_serialization_roundtrips() {
        let level = LevelData {
            seed: 12345,
            time: 6000,
        };
        let encoded_level = bincode::serialize(&level).unwrap();
        let decoded_level: LevelData = bincode::deserialize(&encoded_level).unwrap();
        assert_eq!(level.seed, decoded_level.seed);
        assert_eq!(level.time, decoded_level.time);

        let player = PlayerData {
            position: [1.0, 2.0, 3.0],
            velocity: [0.1, 0.2, 0.3],
            yaw: 1.5,
            pitch: 0.5,
            health: 20.0,
            hunger: 20.0,
            saturation: 5.0,
            exhaustion: 0.0,
            oxygen: 300.0,
            experience: 120,
            experience_level: 12,
            game_mode: GameMode::Survival,
            inventory: InventoryData {
                hotbar: vec![Some(ItemStackData {
                    item: Item::Stone,
                    count: 64,
                    durability: 0,
                    enchantments: Default::default(),
                    potion: None,
                    custom_name: Default::default(),
                })],
                main: vec![None],
                armor: vec![None],
                selected: 0,
            },
        };
        let encoded_player = bincode::serialize(&player).unwrap();
        let decoded_player: PlayerData = bincode::deserialize(&encoded_player).unwrap();
        assert_eq!(player.position, decoded_player.position);
        assert_eq!(player.yaw, decoded_player.yaw);
        assert_eq!(player.health, decoded_player.health);
        assert_eq!(
            player.inventory.hotbar[0].as_ref().unwrap().item,
            Item::Stone
        );

        let mut original_blocks = vec![0u8; 16 * 256 * 16];
        original_blocks[0] = 1;
        original_blocks[100] = 3;
        let compressed_blocks = compress_bytes(&original_blocks).unwrap();
        let decompressed_blocks = decompress_bytes(&compressed_blocks).unwrap();
        assert_eq!(original_blocks, decompressed_blocks);
        println!(
            "Compressed size: {} bytes, Original: {} bytes",
            compressed_blocks.len(),
            original_blocks.len()
        );
    }

    #[test]
    fn enchanted_potion_stack_metadata_roundtrips() {
        let mut stack = ItemStack::new(Item::Potion, 1);
        stack
            .enchantments
            .add_or_upgrade(crate::enchantment::Enchantment::Unbreaking(3));
        stack.potion = Some(crate::brewing::PotionData {
            kind: crate::brewing::PotionKind::Speed,
            level: 2,
            duration_seconds: 90,
            splash: true,
        });
        stack.custom_name.set("Swift Brew");
        let encoded = bincode::serialize(&ItemStackData::from(&stack)).unwrap();
        let decoded: ItemStackData = bincode::deserialize(&encoded).unwrap();
        let decoded = decoded.to_item_stack();
        assert_eq!(decoded.enchantments, stack.enchantments);
        assert_eq!(decoded.potion, stack.potion);
        assert_eq!(decoded.custom_name.as_str(), "Swift Brew");
    }

    #[test]
    fn saved_chunk_restores_player_placed_blocks() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_chunk_save_{}_{}",
            std::process::id(),
            unique
        ));

        let mut original = Chunk::new(0, 0);
        original.blocks[8][100][8] = BlockType::Brick;

        let mut manager = SaveManager::new(&world_dir);
        manager
            .save_chunk(0, 0, ChunkSaveData::from_chunk(&original))
            .unwrap();

        let saved = manager.load_chunk(0, 0).expect("saved chunk should load");
        let mut restored = Chunk::new(0, 0);
        saved.restore_to_chunk(&mut restored);

        assert_eq!(restored.blocks[8][100][8], BlockType::Brick);

        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn dimension_chunk_namespaces_are_independent() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_dimension_save_{}_{}",
            std::process::id(),
            unique
        ));
        let mut manager = SaveManager::new(&world_dir);
        let cases = [
            (crate::dimension::Dimension::Overworld, BlockType::Brick),
            (crate::dimension::Dimension::Nether, BlockType::Netherrack),
            (crate::dimension::Dimension::End, BlockType::EndStone),
        ];

        for (dimension, marker) in cases {
            let mut chunk = Chunk::new(4, -3);
            chunk.blocks[7][90][11] = marker;
            manager
                .save_chunk_in(dimension, 4, -3, ChunkSaveData::from_chunk(&chunk))
                .unwrap();
        }

        drop(manager);
        let mut manager = SaveManager::new(&world_dir);
        for (dimension, marker) in cases {
            let saved = manager
                .load_chunk_in(dimension, 4, -3)
                .expect("dimension chunk should load");
            let mut restored = Chunk::new(4, -3);
            saved.restore_to_chunk(&mut restored);
            assert_eq!(restored.blocks[7][90][11], marker);
        }

        assert!(world_dir.join("regions/r.0.-1.bin").exists());
        assert!(world_dir
            .join("dimensions/nether/regions/r.0.-1.bin")
            .exists());
        assert!(world_dir.join("dimensions/end/regions/r.0.-1.bin").exists());
        fs::remove_dir_all(world_dir).unwrap();
    }

    #[test]
    fn current_dimension_sidecar_roundtrips_and_defaults_to_overworld() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let world_dir = std::env::temp_dir().join(format!(
            "icraft_dimension_state_{}_{}",
            std::process::id(),
            unique
        ));
        let manager = SaveManager::new(&world_dir);
        assert_eq!(
            manager.load_current_dimension(),
            crate::dimension::Dimension::Overworld
        );
        manager
            .save_current_dimension(crate::dimension::Dimension::End)
            .unwrap();
        assert_eq!(
            manager.load_current_dimension(),
            crate::dimension::Dimension::End
        );
        fs::remove_dir_all(world_dir).unwrap();
    }
}
