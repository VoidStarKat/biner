use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::hash::{BuildHasher, Hash, Hasher, RandomState};
use std::iter::FusedIterator;

/// Defines a slot for extension by hooks. A type that implements this trait will be used
/// as the key for accessing hook objects. Since these slot types are never instantiated, zero-sized
/// types are usually sufficient.
///
/// # Examples
///
/// ```rust
/// use biner::HookSlot;
///
/// trait MyHookTrait: std::any::Any + Send + Sync {
///     // ... insert a custom hook interface for the MyHook slot
/// }
///
/// struct MyHook; // The slot type will be used as key
///
/// impl HookSlot for MyHook {
///     type TraitObject = dyn MyHookTrait; // Indicate the trait object to be used for hooks
/// }
/// ```
pub trait HookSlot: 'static {
    /// The trait object used for this slot. This should be a `dyn` trait object and is required
    /// to be set when implementing this trait for a hook slot.
    type TraitObject: ?Sized + Any + Send + Sync;

    /// Gets the unique identifier for this hook slot, which by default is just the `TypeId` of the
    /// slot's type, but is valid to give any unique type id.
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
        f.debug_struct("Hook")
            .field("plugin", &self.plugin)
            .field("slot", &self.slot)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Manages hooks for plugins. Hooks are types implementing hook traits (which can be any `'static`
/// trait that is [`Send`] and [`Sync`]). These hooks are then attached to "hook slots" by plugins.
/// Hook slots are a type implementing [`HookSlot`] and the slot types are used as generic type
/// keys to manage these hooks. Hooks are indexed internally by plugin, hook slot, and an optional
/// name discriminator. This means a plugin can register multiple hooks for the same slot as long
/// as each hook has a different name.
///
/// # Generic Arguments
///
/// `Id` is the type used for identifying plugins and hook names. This type should be a type that is
/// [`Ord`], [`Hash`], and [`Copy`]. It is generic so that interned strings, UUIDs, or other
/// different styles of ids can be used instead of `&'static str`. Plugin ids are determined by the
/// plugin host by specifying the [`PluginManifest::PluginId`][super::PluginManifest::PluginId] type.
///
/// `S` allows you to specify an alternative hasher for the internal indexes of the hooks.
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

    /// Gets whether any hooks have been added by the specified plugin for a hook slot.
    pub fn exists<Slot>(&self, plugin: Id) -> bool
    where
        Slot: HookSlot,
    {
        let slot = Slot::id();
        self.get_first_hook(plugin, slot).is_some()
    }

    /// Gets whether a hook with the exact name was added by the specified plugin for a hook slot.
    pub fn exists_exact<Slot>(&self, plugin: Id, name: Option<Id>) -> bool
    where
        Slot: HookSlot,
    {
        let slot = Slot::id();
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

    /// Get the first dyn object hook added by a plugin for the hook slot.
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

    /// Get the dyn object hook with the specified name added by a plugin for the hook slot.
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

    /// Get the first dyn mutable object hook added by a plugin for the hook slot.
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

    /// Get the dyn mutable object hook with the specified name added by a plugin for the hook slot.
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

    /// Remove a specific hook from the registry matching the plugin id, name, and hook slot. If a
    /// matching hook existed, returns the removed dyn object.
    pub fn remove<Slot>(&mut self, plugin: Id, name: Option<Id>) -> Option<Box<Slot::TraitObject>>
    where
        Slot: HookSlot,
    {
        let slot = Slot::id();
        let plugin_hooks = self.slot_hooks.get_mut(&slot)?;
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

    /// Remove all hooks added by a plugin.
    pub fn remove_plugin_hooks(&mut self, plugin: Id) {
        for plugin_hooks in self.slot_hooks.values_mut() {
            plugin_hooks.remove(&plugin);
        }
    }

    /// Shrink the capacities allocated internally by the registry.
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

    /// Get an iterator over the plugin hooks for the specified slot. This is often simply a single
    /// hook unless unique names are used when registering multiple hooks.
    pub fn plugin_slot_hooks<Slot>(
        &self,
        plugin: Id,
    ) -> impl FusedIterator<Item = &Slot::TraitObject>
    where
        Slot: HookSlot,
    {
        self.slot_hooks
            .get(&Slot::id())
            .into_iter()
            .flat_map(move |m| m.get(&plugin))
            .flatten()
            .filter_map(|h| {
                h.ptr
                    .downcast_ref::<Box<Slot::TraitObject>>()
                    .map(|b| b.as_ref())
            })
    }

    /// Get an iterator over the mutable plugin hooks for the specified slot. This is often simply a
    /// single hook unless unique names are used when registering multiple hooks.
    pub fn plugin_slot_hooks_mut<Slot>(
        &mut self,
        plugin: Id,
    ) -> impl FusedIterator<Item = &mut Slot::TraitObject>
    where
        Slot: HookSlot,
    {
        self.slot_hooks
            .get_mut(&Slot::id())
            .into_iter()
            .flat_map(move |m| m.get_mut(&plugin))
            .flatten()
            .filter_map(|h| {
                h.ptr
                    .downcast_mut::<Box<Slot::TraitObject>>()
                    .map(|b| b.as_mut())
            })
    }

    /// Get an iterator over all the hooks from all plugins registered to a slot, including the id
    /// of the plugin that registered that slot.
    pub fn slot_hooks_and_plugin<Slot>(&self) -> impl FusedIterator<Item = (Id, &Slot::TraitObject)>
    where
        Slot: HookSlot,
    {
        self.slot_hooks
            .get(&Slot::id())
            .into_iter()
            .flatten()
            .flat_map(|m| {
                m.1.iter()
                    .filter_map(|h| h.ptr.downcast_ref::<Box<Slot::TraitObject>>())
                    .map(move |b| (*m.0, b.as_ref()))
            })
    }

    /// Get an iterator over all the mutable hooks from all plugins registered to a slot, including \
    /// the id of the plugin that registered that slot.
    pub fn slot_hooks_and_plugin_mut<Slot>(
        &mut self,
    ) -> impl FusedIterator<Item = (Id, &mut Slot::TraitObject)>
    where
        Slot: HookSlot,
    {
        self.slot_hooks
            .get_mut(&Slot::id())
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
    /// Register a hook for a slot with the given plugin and optional name.
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
