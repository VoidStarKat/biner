use crate::hook::{HookRegistry, HookSlot};
use std::{
    borrow::Borrow,
    collections::{HashMap, hash_map},
    fmt::Debug,
    hash::Hash,
    iter::FusedIterator,
};

#[cfg(feature = "downcast-rs")]
use downcast_rs::{DowncastSync, impl_downcast};
#[cfg(not(any(feature = "downcast-rs", feature = "downcast")))]
use std::any::Any;
use std::collections::HashSet;

#[cfg(feature = "downcast")]
use downcast::{AnySync, downcast_sync};

pub trait PluginManifest<S> {
    fn id(&self) -> &S;
}

#[derive(Debug, Default)]
pub struct SimplePluginManifest<S = String> {
    id: S,
    description: String,
}

impl<S> SimplePluginManifest<S> {
    pub fn new(id: S, description: String) -> Self {
        Self { id, description }
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

impl<S> PluginManifest<S> for SimplePluginManifest<S> {
    fn id(&self) -> &S {
        &self.id
    }
}

macro_rules! decl_plugin_trait {
    ($($traits:ident),+) => {
        pub trait Plugin<S, C>: $($traits+)+ {
            fn load(&mut self, _context: &mut C, _hooks: &mut HookRegistry<S>) {}
            fn unload(&mut self, _context: &mut C) {}
            fn enable(&mut self, _context: &mut C) {}
            fn disable(&mut self, _context: &mut C) {}
        }
    };
}

#[cfg(not(any(feature = "downcast-rs", feature = "downcast")))]
decl_plugin_trait!(Any, Send, Sync);

#[cfg(feature = "downcast-rs")]
decl_plugin_trait!(DowncastSync);

#[cfg(feature = "downcast-rs")]
impl_downcast!(sync Plugin<S, C>);

#[cfg(feature = "downcast")]
decl_plugin_trait!(AnySync);

#[cfg(feature = "downcast")]
downcast_sync!(<S, C> dyn Plugin<S, C>);

struct PluginState<S, M, C> {
    manifest: M,
    plugin: Box<dyn Plugin<S, C>>,
    enabled: bool,
}

impl<S, M, C> PluginState<S, M, C> {
    pub fn new(manifest: M, plugin: Box<dyn Plugin<S, C>>) -> Self {
        Self {
            manifest,
            plugin,
            enabled: false,
        }
    }
}

impl<S, M, C> Debug for PluginState<S, M, C>
where
    M: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginState")
            .field("manifest", &self.manifest)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub struct PluginRegistry<S = String, M = SimplePluginManifest<S>, C = ()> {
    plugins: HashMap<S, PluginState<S, M, C>>,
    hooks: HookRegistry<S>,
}

impl<S, M, C> PluginRegistry<S, M, C>
where
    M: PluginManifest<S>,
    S: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            hooks: HookRegistry::new(),
        }
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    pub fn hooks(&self) -> &HookRegistry<S> {
        &self.hooks
    }

    pub fn hooks_mut(&mut self) -> &mut HookRegistry<S> {
        &mut self.hooks
    }
}

impl<S, M, C> PluginRegistry<S, M, C>
where
    M: PluginManifest<S>,
    S: AsRef<str>,
{
    pub fn plugin_ids(&self) -> PluginIdIter<S, M, C> {
        PluginIdIter {
            iter: self.plugins.keys(),
        }
    }
}

impl<S, M, C> PluginRegistry<S, M, C>
where
    M: PluginManifest<S>,
    S: Eq + Hash,
{
    pub fn get_manifest<Q>(&self, id: &Q) -> Option<&M>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get(id).map(|s| &s.manifest)
    }
}

impl<S, M, C> PluginRegistry<S, M, C>
where
    S: Eq + Hash,
{
    pub fn is_enabled<Q>(&self, id: &Q) -> Option<bool>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get(id).map(|s| s.enabled)
    }

    pub fn enabled_plugin_ids(&self) -> impl FusedIterator<Item = &S> {
        self.plugins
            .iter()
            .filter_map(|(id, plugin)| plugin.enabled.then_some(id))
    }

    pub fn get_plugin<Q>(&self, id: &Q) -> Option<&dyn Plugin<S, C>>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get(id).map(|s| s.plugin.as_ref())
    }

    pub fn get_plugin_mut<Q>(&mut self, id: &Q) -> Option<&mut dyn Plugin<S, C>>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get_mut(id).map(|s| s.plugin.as_mut())
    }

    pub fn enabled_hooks<Slot>(&self) -> impl FusedIterator<Item = &Slot::HookTraitObject>
    where
        Slot: HookSlot<S>,
    {
        let enabled: HashSet<_> = self
            .plugins
            .iter()
            .filter_map(|(id, plugin)| plugin.enabled.then_some(id))
            .collect();
        self.hooks
            .slot_hooks_and_plugin::<Slot>()
            .filter_map(move |(plugin, hook)| enabled.contains(plugin).then_some(hook))
    }

    pub fn enabled_hooks_mut<Slot>(
        &mut self,
    ) -> impl FusedIterator<Item = &mut Slot::HookTraitObject>
    where
        S: Clone,
        Slot: HookSlot<S>,
    {
        let enabled: HashSet<_> = self
            .plugins
            .iter()
            .filter_map(|(id, plugin)| plugin.enabled.then_some(id))
            .collect();
        self.hooks
            .slot_hooks_and_plugin_mut::<Slot>()
            .filter_map(move |(plugin, hook)| enabled.contains(plugin).then_some(hook))
    }
}

impl<S, M, C> PluginRegistry<S, M, C>
where
    M: PluginManifest<S>,
    S: Eq + Hash + Clone + 'static,
    C: 'static,
{
    pub fn load_plugin<Q>(&mut self, manifest: M, plugin: Q, context: &mut C) -> bool
    where
        Q: Into<Box<dyn Plugin<S, C>>>,
    {
        let id = manifest.id().clone();
        if !self.plugins.contains_key(&id) {
            self.plugins
                .entry(id)
                .insert_entry(PluginState::new(manifest, plugin.into()))
                .into_mut()
                .plugin
                .as_mut()
                .load(context, &mut self.hooks);

            true
        } else {
            false
        }
    }
}

impl<S, M, C> PluginRegistry<S, M, C>
where
    S: Eq + Hash + 'static,
    C: 'static,
{
    pub fn unload_plugin<Q>(&mut self, id: &Q, context: &mut C) -> bool
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        if let Some(state) = self.plugins.get_mut(id) {
            state.plugin.unload(context);
            self.plugins.remove(id);
            true
        } else {
            false
        }
    }

    pub fn enable_plugin<Q>(&mut self, id: &Q, context: &mut C) -> bool
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        if let Some(state) = self.plugins.get_mut(id) {
            if !state.enabled {
                state.plugin.enable(context);
                state.enabled = true;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn disable_plugin<Q>(&mut self, id: &Q, context: &mut C) -> bool
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        if let Some(state) = self.plugins.get_mut(id) {
            if state.enabled {
                state.enabled = false;
                state.plugin.disable(context);
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    #[cfg(feature = "downcast-rs")]
    pub fn get<T, Q>(&self, id: &Q) -> Option<&T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<S, C>,
    {
        self.plugins.get(id)?.plugin.as_ref().downcast_ref()
    }

    #[cfg(feature = "downcast-rs")]
    pub fn get_mut<T, Q>(&mut self, id: &Q) -> Option<&mut T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<S, C>,
    {
        self.plugins.get_mut(id)?.plugin.as_mut().downcast_mut()
    }

    #[cfg(feature = "downcast")]
    pub fn get<T, Q>(&self, id: &Q) -> Option<&T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<S, C>,
    {
        self.plugins.get(id)?.plugin.as_ref().downcast_ref().ok()
    }

    #[cfg(feature = "downcast")]
    pub fn get_mut<T, Q>(&mut self, id: &Q) -> Option<&mut T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<S, C>,
    {
        self.plugins
            .get_mut(id)?
            .plugin
            .as_mut()
            .downcast_mut()
            .ok()
    }
}

#[derive(Debug)]
pub struct PluginIdIter<'a, S, M, C> {
    iter: hash_map::Keys<'a, S, PluginState<S, M, C>>,
}

impl<'a, S, M, C> Iterator for PluginIdIter<'a, S, M, C>
where
    S: AsRef<str>,
{
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|id| id.as_ref())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }

    fn count(self) -> usize
    where
        Self: Sized,
    {
        self.iter.count()
    }
}

impl<S, M, C> FusedIterator for PluginIdIter<'_, S, M, C> where S: AsRef<str> {}

impl<S, M, C> ExactSizeIterator for PluginIdIter<'_, S, M, C> where S: AsRef<str> {}
