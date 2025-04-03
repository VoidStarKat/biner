use std::any::Any;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::iter::FusedIterator;

pub trait HookSlot<Id = String>: 'static {
    type TraitObject: ?Sized + Any + Send + Sync;

    fn id() -> Id;
}

type DynHook = dyn Any + Send + Sync;

struct Hook<Id> {
    plugin: Id,
    slot: Id,
    name: Option<Id>,
    ptr: Box<DynHook>,
}

impl<Id> Hook<Id> {
    fn new(plugin: Id, slot: Id, name: Option<Id>, ptr: Box<DynHook>) -> Self {
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

#[derive(Debug, Default)]
pub struct HookRegistry<Id = String> {
    slot_hooks: HashMap<Id, HashMap<Id, Vec<Hook<Id>>>>,
}

impl<Id> HookRegistry<Id>
where
    Id: Eq + Hash,
{
    pub(crate) fn new() -> Self {
        Self {
            slot_hooks: HashMap::new(),
        }
    }

    pub fn exists<Q>(&self, plugin: &Q, slot: &Q) -> bool
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.get_first_hook(plugin, slot).is_some()
    }

    pub fn exists_exact<Q>(&self, plugin: &Q, slot: &Q, name: Option<&Q>) -> bool
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.get_exact_hook(plugin, slot, name).is_some()
    }

    fn get_first_hook<Q>(&self, plugin: &Q, slot: &Q) -> Option<&Hook<Id>>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.slot_hooks.get(slot)?.get(plugin)?.first()
    }

    fn get_exact_hook<Q>(&self, plugin: &Q, slot: &Q, name: Option<&Q>) -> Option<&Hook<Id>>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.slot_hooks
            .get(slot)?
            .get(plugin)?
            .iter()
            .find(|h| h.name.as_ref().map(Borrow::borrow) == name)
    }

    fn get_first_hook_mut<Q>(&mut self, plugin: &Q, slot: &Q) -> Option<&mut Hook<Id>>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.slot_hooks.get_mut(slot)?.get_mut(plugin)?.first_mut()
    }

    fn get_exact_hook_mut<Q>(
        &mut self,
        plugin: &Q,
        slot: &Q,
        name: Option<&Q>,
    ) -> Option<&mut Hook<Id>>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.slot_hooks
            .get_mut(slot)?
            .get_mut(plugin)?
            .iter_mut()
            .find(|h| h.name.as_ref().map(Borrow::borrow) == name)
    }

    pub fn get_first<Slot, Q>(&self, plugin: &Q) -> Option<&Slot::TraitObject>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
        Slot: HookSlot<Id>,
    {
        let slot = Slot::id();
        Some(
            self.get_first_hook(plugin, slot.borrow())?
                .ptr
                .downcast_ref::<Box<Slot::TraitObject>>()?
                .as_ref(),
        )
    }

    pub fn get_exact<Slot, Q>(&self, plugin: &Q, name: Option<&Q>) -> Option<&Slot::TraitObject>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
        Slot: HookSlot<Id>,
    {
        let slot = Slot::id();
        Some(
            self.get_exact_hook(plugin, slot.borrow(), name)?
                .ptr
                .downcast_ref::<Box<Slot::TraitObject>>()?
                .as_ref(),
        )
    }

    pub fn get_first_mut<Slot, Q>(&mut self, plugin: &Q) -> Option<&mut Slot::TraitObject>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
        Slot: HookSlot<Id>,
    {
        let slot = Slot::id();
        Some(
            self.get_first_hook_mut(plugin, slot.borrow())?
                .ptr
                .downcast_mut::<Box<Slot::TraitObject>>()?
                .as_mut(),
        )
    }

    pub fn get_exact_mut<Slot, Q>(
        &mut self,
        plugin: &Q,
        name: Option<&Q>,
    ) -> Option<&mut Slot::TraitObject>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
        Slot: HookSlot<Id>,
    {
        let slot = Slot::id();
        Some(
            self.get_exact_hook_mut(plugin, slot.borrow(), name)?
                .ptr
                .downcast_mut::<Box<Slot::TraitObject>>()?
                .as_mut(),
        )
    }

    pub fn remove<Slot, Q>(
        &mut self,
        plugin: &Q,
        name: Option<&Q>,
    ) -> Option<Box<Slot::TraitObject>>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
        Slot: HookSlot<Id>,
    {
        let slot = Slot::id();
        let plugin_hooks = self.slot_hooks.get_mut(slot.borrow())?;
        let hooks = plugin_hooks.get_mut(plugin)?;
        let idx = hooks
            .iter()
            .position(|h| h.name.as_ref().map(Borrow::borrow) == name)?;
        Some(
            *hooks
                .swap_remove(idx)
                .ptr
                .downcast::<Box<Slot::TraitObject>>()
                .ok()?,
        )
    }

    pub fn remove_plugin_hooks<Q>(&mut self, plugin: &Q)
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
    {
        for plugin_hooks in self.slot_hooks.values_mut() {
            plugin_hooks.remove(plugin);
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

    pub fn plugin_slot_hooks<Slot, Q>(
        &self,
        plugin: &Q,
    ) -> impl FusedIterator<Item = &Slot::TraitObject>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
        Slot: HookSlot<Id>,
    {
        self.slot_hooks
            .get(Slot::id().borrow())
            .into_iter()
            .flat_map(|m| m.get(plugin))
            .flatten()
            .filter_map(|h| {
                h.ptr
                    .downcast_ref::<Box<Slot::TraitObject>>()
                    .map(|b| b.as_ref())
            })
    }

    pub fn plugin_slot_hooks_mut<Slot, Q>(
        &mut self,
        plugin: &Q,
    ) -> impl FusedIterator<Item = &mut Slot::TraitObject>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
        Slot: HookSlot<Id>,
    {
        self.slot_hooks
            .get_mut(Slot::id().borrow())
            .into_iter()
            .flat_map(|m| m.get_mut(plugin))
            .flatten()
            .filter_map(|h| {
                h.ptr
                    .downcast_mut::<Box<Slot::TraitObject>>()
                    .map(|b| b.as_mut())
            })
    }

    pub fn slot_hooks_and_plugin<Slot>(
        &self,
    ) -> impl FusedIterator<Item = (&Id, &Slot::TraitObject)>
    where
        Slot: HookSlot<Id>,
    {
        self.slot_hooks
            .get(Slot::id().borrow())
            .into_iter()
            .flatten()
            .flat_map(|m| {
                m.1.iter()
                    .filter_map(|h| h.ptr.downcast_ref::<Box<Slot::TraitObject>>())
                    .map(move |b| (m.0, b.as_ref()))
            })
    }

    pub fn slot_hooks_and_plugin_mut<Slot>(
        &mut self,
    ) -> impl FusedIterator<Item = (&Id, &mut Slot::TraitObject)>
    where
        Slot: HookSlot<Id>,
    {
        self.slot_hooks
            .get_mut(Slot::id().borrow())
            .into_iter()
            .flatten()
            .flat_map(|m| {
                m.1.iter_mut()
                    .filter_map(|h| h.ptr.downcast_mut::<Box<Slot::TraitObject>>())
                    .map(move |b| (m.0, b.as_mut()))
            })
    }
}

impl<Id> HookRegistry<Id>
where
    Id: Eq + Hash + Clone,
{
    pub fn register<Slot>(
        &mut self,
        hook: Box<Slot::TraitObject>,
        plugin: Id,
        name: Option<Id>,
    ) -> Result<(), Box<Slot::TraitObject>>
    where
        Slot: HookSlot<Id>,
    {
        let slot = Slot::id();
        let plugin_hooks = self.slot_hooks.entry(slot.clone()).or_default();
        let hooks = plugin_hooks.entry(plugin.clone()).or_default();
        if !hooks.iter().any(|h| &h.name == &name) {
            hooks.push(Hook::new(plugin, slot, name, Box::new(hook)));
            Ok(())
        } else {
            Err(hook)
        }
    }
}
