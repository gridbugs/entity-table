#[cfg(feature = "serialize")]
pub use serde; // Re-export serde so it can be referenced in macro body
#[cfg(feature = "serialize")]
use serde::{Deserialize, Serialize};
use std::{iter, mem, slice};

type Id = u32;
type Index = u32;

#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Entity {
    id: Id,
    index: Index,
}

#[derive(Clone, Debug, Default)]
struct IndexToId {
    vec: Vec<Option<Id>>,
}

impl IndexToId {
    fn new_with_capacity(capacity: usize) -> Self {
        let mut vec = Vec::with_capacity(capacity);
        vec.resize(capacity, None);
        Self { vec }
    }
    fn insert(&mut self, Entity { id, index }: Entity) {
        if index as usize >= self.vec.len() {
            self.vec.resize(index as usize + 1, None);
        }
        self.vec[index as usize] = Some(id);
    }
    fn remove(&mut self, index: Index) {
        if let Some(entry) = self.vec.get_mut(index as usize) {
            *entry = None;
        }
    }
    fn clear(&mut self) {
        self.vec.clear();
    }
    fn contains(&self, entity: Entity) -> bool {
        self.vec.get(entity.index as usize) == Some(&Some(entity.id))
    }
    fn entities(&self) -> Entities {
        Entities {
            iter: self.vec.iter().enumerate(),
        }
    }
}

pub struct Entities<'a> {
    iter: iter::Enumerate<slice::Iter<'a, Option<Id>>>,
}

impl<'a> Iterator for Entities<'a> {
    type Item = Entity;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some((index, maybe_id)) = self.iter.next() {
                if let Some(id) = maybe_id {
                    return Some(Entity {
                        index: index as u32,
                        id: *id,
                    });
                }
            } else {
                return None;
            }
        }
    }
}

#[cfg(feature = "serialize")]
#[derive(Serialize, Deserialize)]
struct IndexToIdSerialize {
    entities: Vec<Entity>,
    // In the typical use case there will be a large number of entities whose id and index fields
    // will match, beginning with the entity whose id and index are 0. This is because in most
    // games there is a large amount of static entities which make up the level that get allocated
    // before dynamic entities.
    num_matching_entities: u32,
}

#[cfg(feature = "serialize")]
impl IndexToId {
    fn to_serialize(&self) -> IndexToIdSerialize {
        let entities = self
            .vec
            .iter()
            .enumerate()
            .skip_while(|&(index, maybe_id)| maybe_id.map(|id| id == index as u32).unwrap_or(false))
            .filter_map(|(index, maybe_id)| {
                maybe_id.map(|id| Entity {
                    id,
                    index: index as u32,
                })
            })
            .collect();
        let num_matching_entities = self
            .vec
            .iter()
            .enumerate()
            .take_while(|&(index, maybe_id)| maybe_id.map(|id| id == index as u32).unwrap_or(false))
            .count() as u32;
        IndexToIdSerialize {
            entities,
            num_matching_entities,
        }
    }
    fn from_serialize(
        IndexToIdSerialize {
            entities,
            num_matching_entities,
        }: IndexToIdSerialize,
    ) -> Self {
        let vec = if let Some(max_index) = entities
            .iter()
            .map(|e| e.index)
            .max()
            .or(num_matching_entities.checked_sub(1))
        {
            let mut vec = vec![None; max_index as usize + 1];
            for id_and_index in 0..num_matching_entities {
                vec[id_and_index as usize] = Some(id_and_index);
            }
            for entity in entities {
                vec[entity.index as usize] = Some(entity.id);
            }
            vec
        } else {
            Vec::new()
        };
        Self { vec }
    }
}

#[cfg(feature = "serialize")]
impl Serialize for IndexToId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_serialize().serialize(s)
    }
}

#[cfg(feature = "serialize")]
impl<'a> Deserialize<'a> for IndexToId {
    fn deserialize<D: serde::Deserializer<'a>>(d: D) -> Result<Self, D::Error> {
        Deserialize::deserialize(d).map(Self::from_serialize)
    }
}

#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
#[derive(Debug, Default)]
pub struct EntityAllocator {
    next_id: Id,
    next_index: Index,
    index_to_id: IndexToId,
    free_indices: Vec<Index>,
}

impl EntityAllocator {
    pub fn alloc(&mut self) -> Entity {
        let id = self.next_id;
        self.next_id += 1;
        let index = self.free_indices.pop().unwrap_or_else(|| {
            let index = self.next_index;
            self.next_index += 1;
            index
        });
        let entity = Entity { id, index };
        self.index_to_id.insert(entity);
        Entity { id, index }
    }
    pub fn exists(&self, entity: Entity) -> bool {
        self.index_to_id.vec[entity.index as usize] == Some(entity.id)
    }
    pub fn free(&mut self, entity: Entity) {
        if self.exists(entity) {
            self.index_to_id.remove(entity.index);
            self.free_indices.push(entity.index);
        }
    }
    pub fn clear(&mut self) {
        self.next_id = 0;
        self.next_index = 0;
        self.index_to_id.vec.clear();
        self.free_indices.clear();
    }
}

#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct ComponentTableEntry<T> {
    data: T,
    entity: Entity,
}

#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct ComponentTableEntries<T> {
    vec: Vec<ComponentTableEntry<T>>,
}

impl<T> Default for ComponentTableEntries<T> {
    fn default() -> Self {
        Self {
            vec: Default::default(),
        }
    }
}

impl<T> ComponentTableEntries<T> {
    fn entity_index_tables(&self) -> (Vec<Option<Index>>, IndexToId) {
        if let Some(max_index) = self.vec.iter().map(|entry| entry.entity.index).max() {
            let mut vec = Vec::with_capacity(max_index as usize + 1);
            vec.resize(max_index as usize + 1, None);
            let mut entity_index_to_entity_id =
                IndexToId::new_with_capacity(max_index as usize + 1);
            for (index, entry) in self.vec.iter().enumerate() {
                vec[entry.entity.index as usize] = Some(index as u32);
                entity_index_to_entity_id.insert(entry.entity);
            }
            (vec, entity_index_to_entity_id)
        } else {
            (Vec::new(), Default::default())
        }
    }
    pub fn into_component_table(self) -> ComponentTable<T> {
        let (entity_index_to_entry_index, entity_index_to_entity_id) = self.entity_index_tables();
        ComponentTable {
            entries: self,
            entity_index_to_entry_index,
            entity_index_to_entity_id,
        }
    }
    pub fn clear(&mut self) {
        self.vec.clear();
    }
}

#[derive(Debug, Clone)]
pub struct ComponentTable<T> {
    entries: ComponentTableEntries<T>,
    entity_index_to_entry_index: Vec<Option<Index>>,
    entity_index_to_entity_id: IndexToId,
}

impl<T> Default for ComponentTable<T> {
    fn default() -> Self {
        ComponentTableEntries::default().into_component_table()
    }
}

#[cfg(feature = "serialize")]
impl<T: Serialize> Serialize for ComponentTable<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.entries.serialize(s)
    }
}

#[cfg(feature = "serialize")]
impl<'a, T: Deserialize<'a>> Deserialize<'a> for ComponentTable<T> {
    fn deserialize<D: serde::Deserializer<'a>>(d: D) -> Result<Self, D::Error> {
        Deserialize::deserialize(d).map(ComponentTableEntries::into_component_table)
    }
}

impl<T> ComponentTable<T> {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.entity_index_to_entry_index.clear();
        self.entity_index_to_entity_id.clear();
    }
    pub fn is_empty(&self) -> bool {
        self.entries.vec.is_empty()
    }
    pub fn len(&self) -> usize {
        self.entries.vec.len()
    }
    pub fn entries(&self) -> &ComponentTableEntries<T> {
        &self.entries
    }
    pub fn insert(&mut self, entity: Entity, data: T) -> Option<T> {
        if let Some(maybe_entry_index) = self
            .entity_index_to_entry_index
            .get_mut(entity.index as usize)
        {
            if let Some(entry_index) = maybe_entry_index {
                debug_assert!((*entry_index as usize) < self.entries.vec.len());
                let entry = &mut self.entries.vec[*entry_index as usize];
                if entry.entity == entity {
                    debug_assert!(self.entity_index_to_entity_id.contains(entity));
                    // This entity already has a component in this table, so we return the previous
                    // value associated with it and store the new value.
                    Some(mem::replace(&mut entry.data, data))
                } else {
                    // This entity doesn't have a component in this table, but there is a stale
                    // entry from a previous entity with the same index, which will be overwritten.
                    entry.entity.id = entity.id;
                    entry.data = data;
                    self.entity_index_to_entity_id.insert(entity);
                    None
                }
            } else {
                // This entity doesn't have a component in this table, but there is an entry in
                // `self.entity_index_to_entry_index` for the entity, so push a new component and
                // update `self.entity_index_to_entry_index` to point to the new component.
                *maybe_entry_index = Some(self.entries.vec.len() as u32);
                self.entries.vec.push(ComponentTableEntry { data, entity });
                self.entity_index_to_entity_id.insert(entity);
                None
            }
        } else {
            // There is no corresponding entry in `self.entity_index_to_entry_index`. Push the
            // component and add an entry in `self.entity_index_to_entry_index` that points to the
            // new component.
            self.entity_index_to_entry_index
                .resize(entity.index as usize, None);
            self.entity_index_to_entry_index
                .push(Some(self.entries.vec.len() as u32));
            self.entries.vec.push(ComponentTableEntry { data, entity });
            self.entity_index_to_entity_id.insert(entity);
            None
        }
    }
    pub fn contains(&self, entity: Entity) -> bool {
        if let Some(Some(entry_index)) = self.entity_index_to_entry_index.get(entity.index as usize)
        {
            debug_assert!((*entry_index as usize) < self.entries.vec.len());
            self.entries.vec[*entry_index as usize].entity.id == entity.id
        } else {
            false
        }
    }
    pub fn remove(&mut self, entity: Entity) -> Option<T> {
        if let Some(maybe_entry_index) = self
            .entity_index_to_entry_index
            .get_mut(entity.index as usize)
        {
            self.entity_index_to_entity_id.remove(entity.index);
            if let Some(entry_index) = maybe_entry_index.take() {
                debug_assert!((entry_index as usize) < self.entries.vec.len());
                if entry_index as usize == self.entries.vec.len() - 1 {
                    // Optimization when the component is the final one in the table, just pop it
                    // and return it.
                    self.entries.vec.pop().map(|entry| entry.data)
                } else {
                    // Move the final entry in the table over the entry to remove, and then update
                    // the index in `self.entity_index_to_entry_index` for the moved entity.
                    let entry = self.entries.vec.swap_remove(entry_index as usize);
                    let moved_index = self.entries.vec[entry_index as usize].entity.index;
                    self.entity_index_to_entry_index[moved_index as usize] = Some(entry_index);
                    Some(entry.data)
                }
            } else {
                None
            }
        } else {
            debug_assert!(!self.entity_index_to_entity_id.contains(entity));
            None
        }
    }
    pub fn get(&self, entity: Entity) -> Option<&T> {
        if let Some(Some(entry_index)) = self.entity_index_to_entry_index.get(entity.index as usize)
        {
            debug_assert!((*entry_index as usize) < self.entries.vec.len());
            let entry = &self.entries.vec[*entry_index as usize];
            if entry.entity.id == entity.id {
                Some(&entry.data)
            } else {
                None
            }
        } else {
            None
        }
    }
    pub fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
        if let Some(Some(entry_index)) = self.entity_index_to_entry_index.get(entity.index as usize)
        {
            debug_assert!((*entry_index as usize) < self.entries.vec.len());
            let entry = &mut self.entries.vec[*entry_index as usize];
            if entry.entity.id == entity.id {
                Some(&mut entry.data)
            } else {
                None
            }
        } else {
            None
        }
    }
    pub fn iter(&self) -> ComponentTableIter<T> {
        ComponentTableIter {
            iter: self.entries.vec.iter(),
        }
    }
    pub fn iter_mut(&mut self) -> ComponentTableIterMut<T> {
        ComponentTableIterMut {
            iter: self.entries.vec.iter_mut(),
        }
    }
    pub fn entities(&self) -> Entities {
        self.entity_index_to_entity_id.entities()
    }
}

pub struct ComponentTableIter<'a, T> {
    iter: slice::Iter<'a, ComponentTableEntry<T>>,
}

pub struct ComponentTableIterMut<'a, T> {
    iter: slice::IterMut<'a, ComponentTableEntry<T>>,
}

impl<'a, T> Iterator for ComponentTableIter<'a, T> {
    type Item = (Entity, &'a T);
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|entry| (entry.entity, &entry.data))
    }
}

impl<'a, T> Iterator for ComponentTableIterMut<'a, T> {
    type Item = (Entity, &'a mut T);
    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next()
            .map(|entry| (entry.entity, &mut entry.data))
    }
}

#[cfg(not(feature = "serialize"))]
#[macro_export]
macro_rules! declare_entity_module_types {
    { $($component_name:ident: $component_type:ty,)* } => {

        #[derive(Debug, Clone)]
        pub struct Components {
            $(pub $component_name: $crate::ComponentTable<$component_type>,)*
        }

        #[derive(Debug, Clone)]
        pub struct EntityData {
            $(pub $component_name: Option<$component_type>,)*
        }

        #[derive(Debug, Clone)]
        pub struct EntityUpdate {
            $(pub $component_name: Option<Option<$component_type>>,)*
        }
    }
}

#[cfg(feature = "serialize")]
#[macro_export]
macro_rules! declare_entity_module_types {
    { $($component_name:ident: $component_type:ty,)* } => {

        #[derive(Debug, Clone, $crate::serde::Serialize, $crate::serde::Deserialize)]
        pub struct Components {
            $(pub $component_name: $crate::ComponentTable<$component_type>,)*
        }

        #[derive(Debug, Clone, $crate::serde::Serialize, $crate::serde::Deserialize)]
        pub struct EntityData {
            $(pub $component_name: Option<$component_type>,)*
        }

        #[derive(Debug, Clone, $crate::serde::Serialize, $crate::serde::Deserialize)]
        pub struct EntityUpdate {
            $(pub $component_name: Option<Option<$component_type>>,)*
        }
    }
}

#[macro_export]
macro_rules! entity_data_field_pun {
    { $field:ident : $value:expr } => { Some($value) };
    { $field:ident } => { Some($field) };
}

#[macro_export]
macro_rules! entity_data {
    { $($field:ident $(: $value:expr)?,)* .. $default:expr } => {
        EntityData {
            $($field : $crate::entity_data_field_pun!($field $(: $value)?),)*
            ..$default
        }
    };
    { $($field:ident $(: $value:expr)?),* $(,)? } => {
        $crate::entity_data! {
            $($field $(: $value)?,)*
            ..Default::default()
        }
    }
}

#[macro_export]
macro_rules! entity_update_field_pun {
    { $field:ident : $value:expr } => { Some($value) };
    { $field:ident } => { Some($field) };
}

#[macro_export]
macro_rules! entity_update {
    { $($field:ident $(: $value:expr)?,)* .. $default:expr } => {
        EntityUpdate {
            $($field : $crate::entity_update_field_pun!($field $(: $value)?),)*
            ..$default
        }
    };
    { $($field:ident $(: $value:expr)?),* $(,)? } => {
        $crate::entity_update! {
            $($field $(: $value)?,)*
            ..Default::default()
        }
    }
}

#[macro_export]
macro_rules! declare_entity_module {
    { $module_name:ident { $($component_name:ident: $component_type:ty,)* } } => {
        mod $module_name {

            #[allow(unused_imports)]
            use super::*;

            $crate::declare_entity_module_types! {
                $($component_name: $component_type,)*
            }

            impl Default for Components {
                fn default() -> Self {
                    Self {
                        $($component_name: Default::default(),)*
                    }
                }
            }

            impl Default for EntityData {
                fn default() -> Self {
                    Self {
                        $($component_name: None,)*
                    }
                }
            }

            impl Default for EntityUpdate {
                fn default() -> Self {
                    Self {
                        $($component_name: None,)*
                    }
                }
            }

            impl Components {
                #[allow(unused)]
                pub fn clear(&mut self) {
                    $(self.$component_name.clear();)*
                }
                #[allow(unused)]
                pub fn remove_entity(&mut self, entity: $crate::Entity) {
                    $(self.$component_name.remove(entity);)*
                }
                #[allow(unused)]
                pub fn clone_entity_data(&self, entity: $crate::Entity) -> EntityData {
                    EntityData {
                        $($component_name: self.$component_name.get(entity).cloned(),)*
                    }
                }
                #[allow(unused)]
                pub fn remove_entity_data(&mut self, entity: $crate::Entity) -> EntityData {
                    EntityData {
                        $($component_name: self.$component_name.remove(entity),)*
                    }
                }
                #[allow(unused)]
                pub fn insert_entity_data(&mut self, entity: $crate::Entity, entity_data: EntityData) {
                    $(if let Some(field) = entity_data.$component_name {
                        self.$component_name.insert(entity, field);
                    })*
                }
                #[allow(unused)]
                pub fn update_entity_data(&mut self, entity: $crate::Entity, entity_data: EntityData) {
                    $(if let Some(field) = entity_data.$component_name {
                        self.$component_name.insert(entity, field);
                    } else {
                        self.$component_name.remove(entity);
                    })*
                }
                #[allow(unused)]
                pub fn apply_entity_update(&mut self, entity: $crate::Entity, entity_update: EntityUpdate) {
                    $(match entity_update.$component_name {
                        Some(Some(value)) => { self.$component_name.insert(entity, value); }
                        Some(None) => { self.$component_name.remove(entity); }
                        None => (),
                    })*
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn entity_alloc_remove() {
        let mut a = EntityAllocator::default();
        let e0 = a.alloc();
        let e1 = a.alloc();
        let e2 = a.alloc();
        a.free(e1);
        a.free(e1); // second free should be redundant
        let e3 = a.alloc();
        let e4 = a.alloc();
        assert_eq!([e0.id, e1.id, e2.id, e3.id, e4.id], [0, 1, 2, 3, 4]);
        assert_eq!(
            [e0.index, e1.index, e2.index, e3.index, e4.index],
            [0, 1, 2, 1, 3]
        );
    }

    #[test]
    fn component_table_insert_remove() {
        let mut a = EntityAllocator::default();
        let e0 = a.alloc();
        let e1 = a.alloc();
        let e2 = a.alloc();
        let mut c = ComponentTable::default();
        c.insert(e0, "zero".to_string());
        c.insert(e1, "one".to_string());
        c.insert(e2, "two".to_string());
        assert_eq!(c.get(e0).unwrap(), "zero");
        assert_eq!(c.get(e1).unwrap(), "one");
        assert_eq!(c.get(e2).unwrap(), "two");
        c.remove(e0);
        assert_eq!(c.get(e0), None);
        assert_eq!(c.get(e1).unwrap(), "one");
        assert_eq!(c.get(e2).unwrap(), "two");
        c.insert(e0, "zero again".to_string());
        assert_eq!(c.get(e0).unwrap(), "zero again");
        assert_eq!(c.get(e1).unwrap(), "one");
        assert_eq!(c.get(e2).unwrap(), "two");
        c.insert(e1, "one again".to_string());
        *c.get_mut(e2).unwrap() = "two again".to_string();
        assert_eq!(c.get(e0).unwrap(), "zero again");
        assert_eq!(c.get(e1).unwrap(), "one again");
        assert_eq!(c.get(e2).unwrap(), "two again");
        a.free(e0);
        let raw_entries = c
            .entries
            .vec
            .iter()
            .map(|e| e.data.clone())
            .collect::<Vec<_>>();
        assert_eq!(raw_entries, ["two again", "one again", "zero again"]);
        assert_eq!(c.entity_index_to_entry_index, [Some(2), Some(1), Some(0)]);
        let e3 = a.alloc();
        assert_eq!(e3.index, 0);
        assert_eq!(c.get(e3), None);
        c.insert(e3, "three".to_string());
        assert_eq!(c.get(e3).unwrap(), "three");
        let raw_entries = c
            .entries
            .vec
            .iter()
            .map(|e| e.data.clone())
            .collect::<Vec<_>>();
        assert_eq!(raw_entries, ["two again", "one again", "three"]);
        assert_eq!(c.entity_index_to_entry_index, [Some(2), Some(1), Some(0)]);
    }

    #[test]
    fn declare_entity_module_macro() {
        declare_entity_module! {
            components {
                coord: (i32, i32),
                name: String,
                health: i32,
            }
        }
        use components::Components;
        let mut entity_allocator = EntityAllocator::default();
        let mut components = Components::default();
        let e0 = entity_allocator.alloc();
        let e1 = entity_allocator.alloc();
        components.coord.insert(e0, (12, 19));
        components.name.insert(e0, "Foo".to_string());
        components.health.insert(e0, 42);
        components.coord.insert(e1, (0, 0));
        components.remove_entity(e1);
        entity_allocator.free(e1);
        assert!(!components.coord.contains(e1));
        let e0_data = components.remove_entity_data(e0);
        let e2 = entity_allocator.alloc();
        components.insert_entity_data(e2, e0_data);
        assert_eq!(components.name.get(e2).unwrap(), "Foo");
    }

    #[cfg(feature = "serialize")]
    #[test]
    fn serde() {
        declare_entity_module! {
            components {
                coord: (i32, i32),
                name: String,
            }
        }
        use components::{Components, EntityData};
        let mut entity_allocator = EntityAllocator::default();
        let mut components = Components::default();
        let e0 = entity_allocator.alloc();
        components.insert_entity_data(
            e0,
            EntityData {
                coord: Some((21, 42)),
                name: Some("foo".to_string()),
            },
        );
        let e1 = entity_allocator.alloc();
        components.insert_entity_data(
            e1,
            EntityData {
                coord: None,
                name: Some("bar".to_string()),
            },
        );
        let e2 = entity_allocator.alloc();
        components.insert_entity_data(
            e2,
            EntityData {
                coord: Some((2, 3)),
                name: Some("baz".to_string()),
            },
        );
        entity_allocator.free(e1);
        components.remove_entity(e1);
        let e3 = entity_allocator.alloc();
        components.insert_entity_data(
            e3,
            EntityData {
                coord: Some((11, 12)),
                name: Some("qux".to_string()),
            },
        );
        entity_allocator.free(e0);
        components.remove_entity(e0);
        let json = serde_json::to_string(&components).unwrap();
        let deserialized: Components = serde_json::from_str(&json).unwrap();
        assert_eq!(
            components.coord.get(e2).unwrap(),
            deserialized.coord.get(e2).unwrap()
        );
        assert_eq!(
            components.coord.get(e3).unwrap(),
            deserialized.coord.get(e3).unwrap()
        );
        assert_eq!(
            components.name.get(e2).unwrap(),
            deserialized.name.get(e2).unwrap()
        );
        assert_eq!(
            components.name.get(e3).unwrap(),
            deserialized.name.get(e3).unwrap()
        );
        assert!(!entity_allocator.exists(e0));
        assert!(!entity_allocator.exists(e1));
    }

    #[test]
    fn entity_data() {
        declare_entity_module! {
            components {
                coord: (i32, i32),
                name: String,
            }
        }
        use components::EntityData;
        let coord = (12, 1);
        let no_default = entity_data! { coord };
        let with_default = entity_data! { coord, ..Default::default() };
        let non_punned = entity_data! { coord: (12, 1) };
        assert_eq!(no_default.coord, with_default.coord);
        assert_eq!(with_default.coord, non_punned.coord);
    }

    #[test]
    fn entity_update() {
        declare_entity_module! {
            components {
                coord: (i32, i32),
                name: String,
                solid: (),
                age: u8,
            }
        }
        use components::{Components, EntityData, EntityUpdate};
        let mut entity_allocator = EntityAllocator::default();
        let entity = entity_allocator.alloc();
        let mut components = Components::default();
        components.insert_entity_data(
            entity,
            entity_data! {
                coord: (1, 2),
                name: "bar".to_string(),
            },
        );
        let name = Some("foo".to_string());
        let update = entity_update! {
            name,
            age: Some(30),
            solid: None,
        };
        components.apply_entity_update(entity, update);
        let data = components.remove_entity_data(entity);
        assert_eq!(data.name.unwrap(), "foo");
        assert_eq!(data.age.unwrap(), 30);
        assert_eq!(data.solid, None);
        assert_eq!(data.coord.unwrap(), (1, 2));
    }

    #[test]
    fn entity_iterator() {
        fn compare_entity_iterators(x: &ComponentTable<()>) {
            use std::collections::HashSet;
            let via_entities = x.entities().collect::<HashSet<_>>();
            let via_iter = x.iter().map(|(entity, _)| entity).collect::<HashSet<_>>();
            assert_eq!(via_entities, via_iter);
        }
        declare_entity_module! {
            components {
                x: (),
            }
        }
        use components::Components;
        let mut entity_allocator = EntityAllocator::default();
        let mut components = Components::default();
        compare_entity_iterators(&components.x);
        let e0 = entity_allocator.alloc();
        let e1 = entity_allocator.alloc();
        let e2 = entity_allocator.alloc();
        let e3 = entity_allocator.alloc();
        let e4 = entity_allocator.alloc();
        components.x.insert(e0, ());
        components.x.insert(e1, ());
        components.x.insert(e2, ());
        components.x.insert(e3, ());
        components.x.insert(e4, ());
        compare_entity_iterators(&components.x);
        components.x.remove(e2);
        compare_entity_iterators(&components.x);
        components.x.insert(e2, ());
        compare_entity_iterators(&components.x);
        entity_allocator.free(e3);
        let e5 = entity_allocator.alloc();
        components.x.insert(e5, ());
        compare_entity_iterators(&components.x);
    }
}
