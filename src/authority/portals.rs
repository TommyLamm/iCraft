use super::{AuthorityCore, DimensionTransferIntent};
use crate::dimension::Dimension;
use crate::inventory::GameMode;
use crate::network::protocol::PlayerId;
use crate::server_world::FIXED_DT;
use crate::world::BlockType;
use glam::Vec3;
use crate::world::chunk_xz;

impl AuthorityCore {
    pub(crate) fn tick_portal_travel(&mut self, dimension: Dimension) {
        let ids: Vec<_> = self.session_ids_in_dimension(dimension).to_vec();

        for id in ids {
            let Some((position, game_mode, cooldown, contact_time, requested)) =
                self.sessions.get(&id).map(|session| {
                    (
                        session.position,
                        session.game_mode,
                        session.portal_cooldown,
                        session.portal_contact_time,
                        session.portal_requested,
                    )
                })
            else {
                continue;
            };

            let new_cooldown = (cooldown - FIXED_DT).max(0.0);
            if new_cooldown > 0.0 {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.portal_cooldown = new_cooldown;
                    session.portal_contact_time = 0.0;
                    session.portal_requested = false;
                }
                continue;
            }

            if !requested {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.portal_contact_time = 0.0;
                }
                continue;
            }

            let px = position[0].floor() as i32;
            let py = position[1].floor() as i32;
            let pz = position[2].floor() as i32;

            let feet = self
                .world_ref(dimension)
                .expect("loaded dimension missing from world map")
                .get_block(px, py, pz);
            let body = self
                .world_ref(dimension)
                .expect("loaded dimension missing from world map")
                .get_block(px, py + 1, pz);

            if feet == BlockType::EndGateway || body == BlockType::EndGateway {
                if dimension == Dimension::End {
                    let pos_vec = Vec3::from_array(position);
                    let dist = pos_vec.length();
                    let target_pos = if dist < 300.0 {
                        Vec3::new(1035.5, 89.0, 11.5)
                    } else {
                        Vec3::new(0.5, 65.0, 0.5)
                    };
                    // Same-dimension hop still uses the portal transfer
                    // intent so runtime can set teleport allowance and
                    // advance the next inbound pose.
                    if self.execute_portal_dimension_transfer(id, dimension, target_pos.to_array())
                    {
                        let revision = self
                            .world_mut(dimension)
                            .expect("loaded dimension missing from world map")
                            .revisions
                            .allocate();
                        if let Some(session) = self.sessions.get_mut(&id) {
                            session.last_revision = revision;
                            session.gameplay.revision = revision;
                        }
                        self.mark_session_update(id);
                    }
                    continue;
                }
            }

            if feet == BlockType::EndPortal || body == BlockType::EndPortal {
                let target_dim = if dimension == Dimension::End {
                    Dimension::Overworld
                } else {
                    Dimension::End
                };

                let target_pos = if target_dim == Dimension::End {
                    Vec3::new(0.5, 65.0, 0.5)
                } else {
                    let spawn = self.sessions.get(&id).and_then(|s| s.spawn_point);
                    if let Some(sp) = spawn {
                        Vec3::new(sp[0] as f32 + 0.5, sp[1] as f32, sp[2] as f32 + 0.5)
                    } else {
                        Vec3::new(0.5, 65.0, 0.5)
                    }
                };

                self.execute_portal_dimension_transfer(id, target_dim, target_pos.to_array());
                continue;
            }

            if feet == BlockType::NetherPortal || body == BlockType::NetherPortal {
                let new_contact = contact_time + FIXED_DT;
                if new_contact >= 1.0 || game_mode == GameMode::Creative {
                    let target_dim = if dimension == Dimension::Nether {
                        Dimension::Overworld
                    } else {
                        Dimension::Nether
                    };

                    let scaled = crate::dimension::transform_position(
                        dimension,
                        target_dim,
                        Vec3::from_array(position),
                    );
                    let (cx, cz) = chunk_xz(scaled.x.floor() as i32, scaled.z.floor() as i32);
                    let height = target_dim.height();

                    let target_pos = {
                        self.ensure_dimension(target_dim);
                        let target_world = self.world_mut(target_dim).unwrap();
                        // Portal linking is the only tick-path ensure outside
                        // interest/budget: the destination column pair is
                        // materialized so the frame can be written, then it
                        // becomes a normal evict candidate once nobody is there.
                        let target_y = target_world
                            .safe_spawn_y(scaled.x.floor() as i32, scaled.z.floor() as i32);
                        let (portal_blocks, spawn_vec) =
                            crate::dimension::build_linked_nether_portal_blocks(
                                cx, cz, target_y, height,
                            );
                        let mut mutations = Vec::new();
                        for ((bx, by, bz), btype) in portal_blocks {
                            if target_world.get_block(bx, by, bz) != BlockType::NetherPortal {
                                if let Ok(Some(mutation)) =
                                    target_world.set_block(bx, by, bz, btype, 0)
                                {
                                    mutations.push(mutation);
                                }
                            }
                        }
                        (spawn_vec, mutations)
                    };
                    self.pending_mutations.extend(target_pos.1);
                    self.execute_portal_dimension_transfer(id, target_dim, target_pos.0.to_array());
                } else {
                    if let Some(session) = self.sessions.get_mut(&id) {
                        session.portal_contact_time = new_contact;
                    }
                }
            } else {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.portal_contact_time = 0.0;
                    session.portal_requested = false;
                }
            }
        }
    }

    pub fn execute_portal_dimension_transfer(
        &mut self,
        id: PlayerId,
        target_dim: Dimension,
        target_pos: [f32; 3],
    ) -> bool {
        let Some(from) = self
            .sessions
            .get(&id)
            .and_then(|session| Dimension::from_wire(session.dimension))
        else {
            return false;
        };
        if !self.set_session_dimension(id, target_dim) {
            return false;
        }
        if let Some(session) = self.sessions.get_mut(&id) {
            session.position = target_pos;
            session.portal_contact_time = 0.0;
            session.portal_cooldown = 3.0;
            session.portal_requested = false;
        }
        self.pending_session_revisions.insert(id);
        self.pending_dimension_transfers
            .push(DimensionTransferIntent {
                player_id: id,
                from,
                to: target_dim,
                position: target_pos,
            });
        true
    }
}
