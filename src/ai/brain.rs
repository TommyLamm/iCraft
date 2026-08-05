use crate::ai::goal::{
    FollowOwnerGoal, Goal, GoalContext, MeleeAttackGoal, SitGoal, SwimGoal, WanderGoal,
};
use crate::entity::{Entity, EntityType};

pub struct Brain {
    pub goals: Vec<Box<dyn Goal>>,
    pub active_goal_index: Option<usize>,
}

impl Brain {
    pub fn new_for_entity(entity_type: EntityType) -> Self {
        let mut goals: Vec<Box<dyn Goal>> = Vec::new();

        // Universal Swim goal
        goals.push(Box::new(SwimGoal));

        // Pet-specific goals
        if entity_type == EntityType::Wolf || entity_type == EntityType::Cat {
            goals.push(Box::new(SitGoal));
            goals.push(Box::new(FollowOwnerGoal));
        }

        // Hostile attack goals
        if entity_type.is_hostile() || entity_type == EntityType::Wolf {
            let reach = if entity_type == EntityType::Spider {
                2.5
            } else if entity_type == EntityType::EnderDragon || entity_type == EntityType::Wither {
                6.0
            } else {
                1.8
            };
            goals.push(Box::new(MeleeAttackGoal::new(reach, 1.0)));
        }

        // Default Wander goal
        goals.push(Box::new(WanderGoal::new()));

        // Sort by priority (lowest numerical priority first)
        goals.sort_by_key(|g| g.priority());

        Self {
            goals,
            active_goal_index: None,
        }
    }

    pub fn tick(&mut self, entity: &mut Entity, ctx: &mut GoalContext) {
        // Select highest priority goal that can start
        let mut selected_index = None;
        for (idx, goal) in self.goals.iter_mut().enumerate() {
            if goal.can_start(entity, ctx) {
                selected_index = Some(idx);
                break;
            }
        }

        if selected_index != self.active_goal_index {
            if let Some(old_idx) = self.active_goal_index {
                if let Some(old_goal) = self.goals.get_mut(old_idx) {
                    old_goal.stop(entity, ctx);
                }
            }
            self.active_goal_index = selected_index;
            if let Some(new_idx) = selected_index {
                if let Some(new_goal) = self.goals.get_mut(new_idx) {
                    new_goal.start(entity, ctx);
                }
            }
        }

        if let Some(idx) = self.active_goal_index {
            if let Some(goal) = self.goals.get_mut(idx) {
                goal.tick(entity, ctx);
            }
        }
    }
}
