use slotmap::{SlotMap, new_key_type};

new_key_type! {
    /// A generational reference to an entity in its owning store.
    ///
    /// Removing an entity invalidates its handle, even when its slot is reused.
    /// Handles are local to a store; they are not cross-world identifiers.
    pub struct EntityId;
}

/// Entity storage with stale-handle detection. Construct with `EntityStore::with_key()`.
pub type EntityStore<T> = SlotMap<EntityId, T>;

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> [u32; 2] {
        serde_json::from_str(include_str!("../../../tests/fixtures/core/entities.json")).unwrap()
    }

    #[test]
    fn removed_handle_does_not_resolve_to_a_reused_slot() {
        let [first, second] = fixture();
        let mut entities = EntityStore::with_key();
        let old = entities.insert(first);
        assert_eq!(entities.remove(old), Some(first));
        let replacement = entities.insert(second);

        assert_ne!(old, replacement);
        assert_eq!(entities.get(old), None);
        assert_eq!(entities.remove(old), None);
        assert_eq!(entities.get(replacement), Some(&second));
    }

    #[test]
    fn serialization_preserves_generations_and_live_handles() {
        let [first, second] = fixture();
        let mut entities = EntityStore::with_key();
        let removed = entities.insert(first);
        entities.remove(removed);
        let live = entities.insert(second);
        let serialized = serde_json::to_vec(&entities).unwrap();
        let restored: EntityStore<u32> = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(restored.get(removed), None);
        assert_eq!(restored.get(live), Some(&second));
    }
}
