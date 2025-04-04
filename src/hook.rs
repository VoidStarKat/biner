use std::any::{Any, TypeId};
use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::hash::{BuildHasher, Hash, Hasher, RandomState};
use std::iter::FusedIterator;

pub trait HookSlot: 'static {
    type TraitObject: ?Sized + Any + Send + Sync;

    fn id() -> TypeId {
        TypeId::of::<Self>()
    }
}

type DynHook = dyn Any + Send + Sync;

struct Hook<Id> {
    plugin: Id,
    slot: TypeId,
    name: Option<Id>,
    ptr: Box<DynHook>,
}

impl<Id> Hook<Id> {
    fn new(plugin: Id, slot: TypeId, name: Option<Id>, ptr: Box<DynHook>) -> Self {
        Self {
            plugin,
            slot,
            name,
            ptr,
        }
    }
}

impl<Id> PartialEq for Hook<Id>
where
    Id: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.plugin == other.plugin && self.slot == other.slot && self.name == other.name
    }
}

impl<Id> Eq for Hook<Id> where Id: Eq {}

impl<Id> Hash for Hook<Id>
where
    Id: Hash,
{
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.plugin.hash(state);
        self.slot.hash(state);
        self.name.hash(state);
    }
}

impl<Id> Debug for Hook<Id>
where
    Id: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyedHook")
            .field("plugin", &self.plugin)
            .field("slot", &self.slot)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct HookRegistry<Id = &'static str, S = RandomState> {
    slot_hooks: HashMap<TypeId, HashMap<Id, Vec<Hook<Id>>, S>, S>,
}

impl<Id> HookRegistry<Id> {
    pub(crate) fn new() -> Self {
        Self {
            slot_hooks: HashMap::new(),
        }
    }
}

impl<Id, S> HookRegistry<Id, S>
where
    Id: Copy + Ord + Hash,
    S: BuildHasher,
{
    pub(crate) fn with_hasher(hash_builder: S) -> Self {
        Self {
            slot_hooks: HashMap::with_hasher(hash_builder),
        }
    }

    pub fn exists(&self, plugin: Id, slot: TypeId) -> bool {
        self.get_first_hook(plugin, slot).is_some()
    }

    pub fn exists_exact(&self, plugin: Id, slot: TypeId, name: Option<Id>) -> bool {
        self.get_exact_hook(plugin, slot, name).is_some()
    }

    fn get_first_hook(&self, plugin: Id, slot: TypeId) -> Option<&Hook<Id>> {
        self.slot_hooks.get(&slot)?.get(&plugin)?.first()
    }

    fn get_exact_hook(&self, plugin: Id, slot: TypeId, name: Option<Id>) -> Option<&Hook<Id>> {
        self.slot_hooks
            .get(&slot)?
            .get(&plugin)?
            .iter()
            .find(|h| h.name == name)
    }

    fn get_first_hook_mut(&mut self, plugin: Id, slot: TypeId) -> Option<&mut Hook<Id>> {
        self.slot_hooks
            .get_mut(&slot)?
            .get_mut(&plugin)?
            .first_mut()
    }

    fn get_exact_hook_mut(
        &mut self,
        plugin: Id,
        slot: TypeId,
        name: Option<Id>,
    ) -> Option<&mut Hook<Id>> {
        self.slot_hooks
            .get_mut(&slot)?
            .get_mut(&plugin)?
            .iter_mut()
            .find(|h| h.name == name)
    }

    pub fn get_first<Slot>(&self, plugin: Id) -> Option<&Slot::TraitObject>
    where
        Slot: HookSlot,
    {
        let slot = Slot::id();
        Some(
            self.get_first_hook(plugin, slot)?
                .ptr
                .downcast_ref::<Box<Slot::TraitObject>>()?
                .as_ref(),
        )
    }

    pub fn get_exact<Slot>(&self, plugin: Id, name: Option<Id>) -> Option<&Slot::TraitObject>
    where
        Slot: HookSlot,
    {
        let slot = Slot::id();
        Some(
            self.get_exact_hook(plugin, slot, name)?
                .ptr
                .downcast_ref::<Box<Slot::TraitObject>>()?
                .as_ref(),
        )
    }

    pub fn get_first_mut<Slot>(&mut self, plugin: Id) -> Option<&mut Slot::TraitObject>
    where
        Slot: HookSlot,
    {
        let slot = Slot::id();
        Some(
            self.get_first_hook_mut(plugin, slot)?
                .ptr
                .downcast_mut::<Box<Slot::TraitObject>>()?
                .as_mut(),
        )
    }

    pub fn get_exact_mut<Slot>(
        &mut self,
        plugin: Id,
        name: Option<Id>,
    ) -> Option<&mut Slot::TraitObject>
    where
        Slot: HookSlot,
    {
        let slot = Slot::id();
        Some(
            self.get_exact_hook_mut(plugin, slot, name)?
                .ptr
                .downcast_mut::<Box<Slot::TraitObject>>()?
                .as_mut(),
        )
    }

    pub fn remove<Slot>(&mut self, plugin: Id, name: Option<Id>) -> Option<Box<Slot::TraitObject>>
    where
        Slot: HookSlot,
    {
        let slot = Slot::id();
        let plugin_hooks = self.slot_hooks.get_mut(slot.borrow())?;
        let hooks = plugin_hooks.get_mut(&plugin)?;
        let idx = hooks.iter().position(|h| h.name == name)?;
        Some(
            *hooks
                .swap_remove(idx)
                .ptr
                .downcast::<Box<Slot::TraitObject>>()
                .ok()?,
        )
    }

    pub fn remove_plugin_hooks(&mut self, plugin: Id) {
        for plugin_hooks in self.slot_hooks.values_mut() {
            plugin_hooks.remove(&plugin);
        }
    }

    pub fn shrink_to_fit(&mut self) {
        for plugin_hooks in self.slot_hooks.values_mut() {
            plugin_hooks.retain(|_, v| {
                let retain = !v.is_empty();
                if !retain {
                    v.shrink_to_fit();
                }
                retain
            });
        }
        self.slot_hooks.retain(|_, m| {
            let retain = !m.is_empty();
            if !retain {
                m.shrink_to_fit();
            }
            retain
        });
        self.slot_hooks.shrink_to_fit();
    }

    pub fn plugin_slot_hooks<Slot>(
        &self,
        plugin: Id,
    ) -> impl FusedIterator<Item = &Slot::TraitObject>
    where
        Slot: HookSlot,
    {
        self.slot_hooks
            .get(Slot::id().borrow())
            .into_iter()
            .flat_map(move |m| m.get(&plugin))
            .flatten()
            .filter_map(|h| {
                h.ptr
                    .downcast_ref::<Box<Slot::TraitObject>>()
                    .map(|b| b.as_ref())
            })
    }

    pub fn plugin_slot_hooks_mut<Slot>(
        &mut self,
        plugin: Id,
    ) -> impl FusedIterator<Item = &mut Slot::TraitObject>
    where
        Slot: HookSlot,
    {
        self.slot_hooks
            .get_mut(Slot::id().borrow())
            .into_iter()
            .flat_map(move |m| m.get_mut(&plugin))
            .flatten()
            .filter_map(|h| {
                h.ptr
                    .downcast_mut::<Box<Slot::TraitObject>>()
                    .map(|b| b.as_mut())
            })
    }

    pub fn slot_hooks_and_plugin<Slot>(&self) -> impl FusedIterator<Item = (Id, &Slot::TraitObject)>
    where
        Slot: HookSlot,
    {
        self.slot_hooks
            .get(Slot::id().borrow())
            .into_iter()
            .flatten()
            .flat_map(|m| {
                m.1.iter()
                    .filter_map(|h| h.ptr.downcast_ref::<Box<Slot::TraitObject>>())
                    .map(move |b| (*m.0, b.as_ref()))
            })
    }

    pub fn slot_hooks_and_plugin_mut<Slot>(
        &mut self,
    ) -> impl FusedIterator<Item = (Id, &mut Slot::TraitObject)>
    where
        Slot: HookSlot,
    {
        self.slot_hooks
            .get_mut(Slot::id().borrow())
            .into_iter()
            .flatten()
            .flat_map(|m| {
                m.1.iter_mut()
                    .filter_map(|h| h.ptr.downcast_mut::<Box<Slot::TraitObject>>())
                    .map(move |b| (*m.0, b.as_mut()))
            })
    }
}

impl<Id, S> HookRegistry<Id, S>
where
    Id: Copy + Ord + Hash,
    S: BuildHasher + Default,
{
    pub fn register<Slot>(
        &mut self,
        hook: Box<Slot::TraitObject>,
        plugin: Id,
        name: Option<Id>,
    ) -> Result<(), Box<Slot::TraitObject>>
    where
        Slot: HookSlot,
    {
        let slot = Slot::id();
        let plugin_hooks = self.slot_hooks.entry(slot).or_default();
        let hooks = plugin_hooks.entry(plugin).or_default();
        if !hooks.iter().any(|h| h.name == name) {
            hooks.push(Hook::new(plugin, slot, name, Box::new(hook)));
            Ok(())
        } else {
            Err(hook)
        }
    }
}

impl<Id, S> Default for HookRegistry<Id, S>
where
    S: Default,
{
    fn default() -> Self {
        Self {
            slot_hooks: HashMap::default(),
        }
    }
}
