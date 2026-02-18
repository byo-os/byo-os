//! Bidirectional mapping between BYO qualified IDs and Bevy entities.

use bevy::prelude::*;
use std::collections::HashMap;

/// Bidirectional mapping from BYO qualified ID strings to Bevy `Entity`.
#[derive(Resource, Default)]
pub struct IdMap {
    to_entity: HashMap<String, Entity>,
    to_id: HashMap<Entity, String>,
}

#[allow(dead_code)]
impl IdMap {
    pub fn insert(&mut self, id: String, entity: Entity) {
        self.to_entity.insert(id.clone(), entity);
        self.to_id.insert(entity, id);
    }

    pub fn get_entity(&self, id: &str) -> Option<Entity> {
        self.to_entity.get(id).copied()
    }

    pub fn get_id(&self, entity: Entity) -> Option<&str> {
        self.to_id.get(&entity).map(|s| s.as_str())
    }

    pub fn remove_by_id(&mut self, id: &str) -> Option<Entity> {
        if let Some(entity) = self.to_entity.remove(id) {
            self.to_id.remove(&entity);
            Some(entity)
        } else {
            None
        }
    }

    pub fn remove_by_entity(&mut self, entity: Entity) -> Option<String> {
        if let Some(id) = self.to_id.remove(&entity) {
            self.to_entity.remove(&id);
            Some(id)
        } else {
            None
        }
    }
}
