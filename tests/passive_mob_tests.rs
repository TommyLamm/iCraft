use glam::Vec3;
use icraft::dimension::Dimension;
use icraft::entity::EntityManager;
use icraft::passive_mob::spawn_passive_mobs;

#[test]
fn passive_mobs_spawn_inside_signed_height_and_loaded_chunks() {
    let seed = 2_563_678_733;
    let dimension = Dimension::Overworld;
    let height = dimension.height();
    let mut chunks = icraft::chunk_manager::WorldColumns::new_in_dimension(2, dimension);
    for cx in -2..=2 {
        for cz in -2..=2 {
            chunks.insert_resident_chunk(
                (cx, cz),
                icraft::dimension::generate_chunk(dimension, cx, cz, seed),
            );
        }
    }

    let mut entities = EntityManager::new();
    for tick in 0..400 {
        spawn_passive_mobs(
            &mut entities,
            &chunks,
            Vec3::new(8.0, 80.0, 8.0),
            15,
            tick as f32 / 20.0,
        );
        if entities.count_passive() > 0 {
            break;
        }
    }
    assert!(
        entities.count_passive() > 0,
        "a loaded Overworld spawn region must establish a passive population"
    );

    for entity in entities
        .entities
        .iter()
        .filter(|entity| entity.entity_type.is_passive())
    {
        let y = entity.position.y.floor() as i32;
        assert!(
            height.contains_y(y),
            "passive spawn Y {y} must stay inside signed Overworld height {:?}",
            (height.min_y(), height.max_y_exclusive())
        );
        let cx = (entity.position.x.floor() as i32).div_euclid(16);
        let cz = (entity.position.z.floor() as i32).div_euclid(16);
        assert!(
            chunks.chunks.contains_key(&(cx, cz)),
            "passive spawn at ({}, {}, {}) must land in a loaded chunk, not ({cx}, {cz})",
            entity.position.x,
            entity.position.y,
            entity.position.z
        );
    }
}
