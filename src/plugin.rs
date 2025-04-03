use crate::HookRegistry;
use std::{
    borrow::Borrow,
    collections::{HashMap, hash_map},
    fmt::Debug,
    hash::Hash,
    iter::FusedIterator,
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LoadPluginError {
    #[error("plugin not found")]
    NotFound,
    #[error("plugin is already loaded")]
    AlreadyLoaded,
    #[error("plugin was not registered with a constructor")]
    MissingConstructor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EnablePluginError {
    #[error("plugin not found")]
    NotFound,
    #[error("plugin is not loaded")]
    NotLoaded,
    #[error("plugin is already enabled")]
    AlreadyEnabled,
}

#[cfg(not(any(feature = "downcast-rs", feature = "downcast")))]
use std::any::Any;

#[cfg(feature = "downcast")]
use downcast::{AnySync, downcast_sync};
#[cfg(feature = "downcast-rs")]
use downcast_rs::{DowncastSync, impl_downcast};

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
        pub trait Plugin<S = String, C = ()>: $($traits+)+ {
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
    enabled: bool,
    #[allow(clippy::type_complexity)]
    ctor: Option<fn() -> Box<dyn Plugin<S, C>>>,
    plugin: Option<Box<dyn Plugin<S, C>>>,
}

impl<S, M, C> PluginState<S, M, C> {
    #[allow(clippy::type_complexity)]
    fn new(
        manifest: M,
        ctor: Option<fn() -> Box<dyn Plugin<S, C>>>,
        plugin: Option<Box<dyn Plugin<S, C>>>,
    ) -> Self {
        Self {
            manifest,
            enabled: false,
            ctor,
            plugin,
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

    pub fn from_initializers(callbacks: impl IntoIterator<Item = fn(&mut Self)>) -> Self {
        let mut this = Self::new();
        for f in callbacks {
            f(&mut this);
        }
        this
    }

    pub fn exists<Q>(&self, id: &Q) -> bool
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.contains_key(id)
    }

    pub fn is_loaded<Q>(&self, id: &Q) -> bool
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins
            .get(id)
            .is_some_and(|state| state.plugin.is_some())
    }

    pub fn is_enabled<Q>(&self, id: &Q) -> bool
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get(id).is_some_and(|state| state.enabled)
    }

    pub fn get_manifest<Q>(&self, id: &Q) -> Option<&M>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get(id).map(|s| &s.manifest)
    }
}

impl<S, M, C> PluginRegistry<S, M, C> {
    pub fn hooks(&self) -> &HookRegistry<S> {
        &self.hooks
    }

    pub fn hooks_mut(&mut self) -> &mut HookRegistry<S> {
        &mut self.hooks
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    pub fn loaded_plugin_count(&self) -> usize {
        self.plugins.values().filter(|p| p.plugin.is_some()).count()
    }

    pub fn enabled_plugin_count(&self) -> usize {
        self.plugins.values().filter(|p| p.enabled).count()
    }

    pub fn plugin_ids(&self) -> impl FusedIterator<Item = &S> {
        self.plugins.keys()
    }

    pub fn loaded_plugin_ids(&self) -> impl FusedIterator<Item = &S> {
        self.plugins
            .iter()
            .filter_map(|(k, p)| p.plugin.is_some().then_some(k))
    }

    pub fn enabled_plugin_ids(&self) -> impl FusedIterator<Item = &S> {
        self.plugins
            .iter()
            .filter_map(|(k, p)| p.enabled.then_some(k))
    }
}

impl<S, M, C> PluginRegistry<S, M, C>
where
    M: PluginManifest<S>,
    S: Eq + Hash + Clone,
{
    #[allow(clippy::type_complexity)]
    pub fn register(&mut self, manifest: M, ctor: Option<fn() -> Box<dyn Plugin<S, C>>>) -> bool {
        let id = manifest.id().clone();
        if let hash_map::Entry::Vacant(e) = self.plugins.entry(id) {
            e.insert(PluginState::new(manifest, ctor, None));
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
    pub fn remove<Q>(&mut self, id: &Q, context: &mut C) -> bool
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        if let Some(state) = self.plugins.get_mut(id) {
            if state.plugin.is_some() {
                let plugin = state.plugin.as_mut().unwrap();
                if state.enabled {
                    plugin.disable(context);
                    state.enabled = false;
                }
                plugin.unload(context);
                self.hooks.remove_plugin_hooks(id);
            }
            self.plugins.remove(id);
            true
        } else {
            false
        }
    }

    pub fn load<P, Q>(&mut self, id: &Q, context: &mut C) -> Result<(), LoadPluginError>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        let state = self.plugins.get_mut(id).ok_or(LoadPluginError::NotFound)?;
        if state.plugin.is_some() {
            Err(LoadPluginError::AlreadyLoaded)
        } else {
            state
                .plugin
                .insert(state.ctor.ok_or(LoadPluginError::MissingConstructor)?())
                .load(context, &mut self.hooks);
            Ok(())
        }
    }

    pub fn load_with<P, Q>(
        &mut self,
        id: &Q,
        plugin: P,
        context: &mut C,
    ) -> Result<(), LoadPluginError>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        P: Into<Box<dyn Plugin<S, C>>>,
    {
        let state = self.plugins.get_mut(id).ok_or(LoadPluginError::NotFound)?;
        if state.plugin.is_some() {
            Err(LoadPluginError::AlreadyLoaded)
        } else {
            state
                .plugin
                .insert(plugin.into())
                .load(context, &mut self.hooks);
            Ok(())
        }
    }

    pub fn unload<Q>(&mut self, id: &Q, context: &mut C) -> bool
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        if let Some(state) = self.plugins.get_mut(id) {
            if state.plugin.is_some() {
                let plugin = state.plugin.as_mut().unwrap();
                if state.enabled {
                    plugin.disable(context);
                    state.enabled = false;
                }
                plugin.unload(context);
                self.hooks.remove_plugin_hooks(id);
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn enable<Q>(&mut self, id: &Q, context: &mut C) -> Result<(), EnablePluginError>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        let state = self
            .plugins
            .get_mut(id)
            .ok_or(EnablePluginError::NotFound)?;
        if state.plugin.is_none() {
            Err(EnablePluginError::NotLoaded)
        } else if state.enabled {
            Err(EnablePluginError::AlreadyEnabled)
        } else {
            state.plugin.as_mut().unwrap().enable(context);
            state.enabled = true;
            Ok(())
        }
    }

    pub fn disable<Q>(&mut self, id: &Q, context: &mut C) -> bool
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        if let Some(state) = self.plugins.get_mut(id) {
            if state.enabled {
                state.plugin.as_mut().unwrap().disable(context);
                state.enabled = false;
            }
            true
        } else {
            false
        }
    }

    #[cfg(feature = "downcast-rs")]
    pub fn get_loaded<T, Q>(&self, id: &Q) -> Option<&T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<S, C>,
    {
        self.plugins.get(id)?.plugin.as_ref()?.downcast_ref()
    }

    #[cfg(feature = "downcast-rs")]
    pub fn get_loaded_mut<T, Q>(&mut self, id: &Q) -> Option<&mut T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<S, C>,
    {
        self.plugins.get_mut(id)?.plugin.as_mut()?.downcast_mut()
    }

    #[cfg(feature = "downcast-rs")]
    pub fn get_enabled<T, Q>(&self, id: &Q) -> Option<&T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<S, C>,
    {
        let state = self.plugins.get(id)?;
        if state.enabled {
            state.plugin.as_ref()?.downcast_ref()
        } else {
            None
        }
    }

    #[cfg(feature = "downcast-rs")]
    pub fn get_enabled_mut<T, Q>(&mut self, id: &Q) -> Option<&mut T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<S, C>,
    {
        let state = self.plugins.get_mut(id)?;
        if state.enabled {
            state.plugin.as_mut()?.downcast_mut()
        } else {
            None
        }
    }

    #[cfg(feature = "downcast")]
    pub fn get_loaded<T, Q>(&self, id: &Q) -> Option<&T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<S, C>,
    {
        self.plugins.get(id)?.plugin.as_ref()?.downcast_ref().ok()
    }

    #[cfg(feature = "downcast")]
    pub fn get_loaded_mut<T, Q>(&mut self, id: &Q) -> Option<&mut T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<S, C>,
    {
        self.plugins
            .get_mut(id)?
            .plugin
            .as_mut()?
            .downcast_mut()
            .ok()
    }

    #[cfg(feature = "downcast")]
    pub fn get_enabled<T, Q>(&self, id: &Q) -> Option<&T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<S, C>,
    {
        let state = self.plugins.get(id)?;
        if state.enabled {
            state.plugin.as_ref()?.downcast_ref().ok()
        } else {
            None
        }
    }

    #[cfg(feature = "downcast")]
    pub fn get_enabled_mut<T, Q>(&mut self, id: &Q) -> Option<&mut T>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<S, C>,
    {
        let state = self.plugins.get_mut(id)?;
        if state.enabled {
            state.plugin.as_mut()?.downcast_mut().ok()
        } else {
            None
        }
    }
}
