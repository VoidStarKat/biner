use std::{
    borrow::Borrow,
    collections::{HashMap, hash_map},
    fmt::Debug,
    hash::Hash,
    iter::FusedIterator,
};

#[cfg(not(any(feature = "downcast-rs", feature = "downcast")))]
use std::any::Any;

#[cfg(feature = "downcast-rs")]
use downcast_rs::{DowncastSync, impl_downcast};

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
        pub trait Plugin<C>: $($traits+)+ {
            fn load(&mut self, _: &mut C) {}
            fn unload(&mut self, _: &mut C) {}
            fn enable(&mut self, _: &mut C) {}
            fn disable(&mut self, _: &mut C) {}
        }
    };
}

#[cfg(not(any(feature = "downcast-rs", feature = "downcast")))]
decl_plugin_trait!(Any, Send, Sync);

#[cfg(feature = "downcast-rs")]
decl_plugin_trait!(DowncastSync);

#[cfg(feature = "downcast-rs")]
impl_downcast!(sync Plugin<C>);

#[cfg(feature = "downcast")]
decl_plugin_trait!(AnySync);

#[cfg(feature = "downcast")]
downcast_sync!(<C> dyn Plugin<C>);

struct PluginState<M, C> {
    manifest: M,
    plugin: Box<dyn Plugin<C>>,
    enabled: bool,
}

impl<M, C> PluginState<M, C> {
    pub fn new(manifest: M, plugin: Box<dyn Plugin<C>>) -> Self {
        Self {
            manifest,
            plugin,
            enabled: false,
        }
    }
}

impl<M, C> std::fmt::Debug for PluginState<M, C>
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
pub struct PluginRegistry<M, C, S = String> {
    plugins: HashMap<S, PluginState<M, C>>,
}

impl<M, C, S> PluginRegistry<M, C, S>
where
    M: PluginManifest<S>,
{
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }
}

impl<M, C, S> PluginRegistry<M, C, S>
where
    M: PluginManifest<S>,
    S: AsRef<str>,
{
    pub fn plugin_ids(&self) -> PluginIdIter<M, C, S> {
        PluginIdIter {
            iter: self.plugins.keys(),
        }
    }
}

impl<M, C, S> PluginRegistry<M, C, S>
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

impl<M, C, S> PluginRegistry<M, C, S>
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

    pub fn get_plugin<Q>(&self, id: &Q) -> Option<&dyn Plugin<C>>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get(id).map(|s| s.plugin.as_ref())
    }

    pub fn get_plugin_mut<Q>(&mut self, id: &Q) -> Option<&mut dyn Plugin<C>>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get_mut(id).map(|s| s.plugin.as_mut())
    }
}

impl<M, C, S> PluginRegistry<M, C, S>
where
    M: PluginManifest<S>,
    S: Eq + Hash + Clone,
    C: 'static,
{
    pub fn load_plugin<Q>(&mut self, manifest: M, plugin: Q, context: &mut C) -> bool
    where
        Q: Into<Box<dyn Plugin<C>>>,
    {
        let id = manifest.id().clone();
        if !self.plugins.contains_key(&id) {
            self.plugins
                .entry(id)
                .insert_entry(PluginState::new(manifest, plugin.into()))
                .into_mut()
                .plugin
                .as_mut()
                .load(context);

            true
        } else {
            false
        }
    }
}

impl<M, C, S> PluginRegistry<M, C, S>
where
    S: Eq + Hash,
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
        T: Plugin<C>,
    {
        self.plugins
            .get(id)
            .and_then(|s| s.plugin.as_ref().downcast_ref())
    }

    #[cfg(feature = "downcast-rs")]
    pub fn get_mut<T, Q>(&mut self, id: &Q) -> Option<&mut T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<C>,
    {
        self.plugins
            .get_mut(id)
            .and_then(|s| s.plugin.as_mut().downcast_mut())
    }

    #[cfg(feature = "downcast")]
    pub fn get<T, Q>(&self, id: &Q) -> Option<&T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<C>,
    {
        self.plugins
            .get(id)
            .and_then(|s| s.plugin.as_ref().downcast_ref().ok())
    }

    #[cfg(feature = "downcast")]
    pub fn get_mut<T, Q>(&mut self, id: &Q) -> Option<&mut T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<C>,
    {
        self.plugins
            .get_mut(id)
            .and_then(|s| s.plugin.as_mut().downcast_mut().ok())
    }
}

#[derive(Debug)]
pub struct PluginIdIter<'a, M, C, S> {
    iter: hash_map::Keys<'a, S, PluginState<M, C>>,
}

impl<'a, M, C, S> Iterator for PluginIdIter<'a, M, C, S>
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

impl<M, C, S> FusedIterator for PluginIdIter<'_, M, C, S> where S: AsRef<str> {}

impl<M, C, S> ExactSizeIterator for PluginIdIter<'_, M, C, S> where S: AsRef<str> {}
