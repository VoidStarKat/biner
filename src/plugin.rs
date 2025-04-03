use crate::HookRegistry;
use std::fmt::Display;
use std::{
    any::Any,
    borrow::Borrow,
    collections::{HashMap, hash_map},
    fmt::Debug,
    hash::Hash,
    iter::FusedIterator,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LoadPluginError<Id> {
    #[error("plugin not found")]
    NotFound,
    #[error("plugin is already loaded")]
    AlreadyLoaded,
    #[error("plugin was not registered with a constructor")]
    MissingConstructor,

    #[error("plugin dependency `{0}` load error: {1}")]
    DependencyLoadError(Id, Box<LoadPluginError<Id>>),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EnablePluginError<Id> {
    #[error("plugin not found")]
    NotFound,
    #[error("plugin is not loaded")]
    NotLoaded,
    #[error("plugin is already enabled")]
    AlreadyEnabled,

    #[error("plugin dependency `{0}` load error: {1}")]
    DependencyLoadError(Id, LoadPluginError<Id>),
    #[error("plugin dependency `{0}` error: {1}")]
    DependencyEnableError(Id, Box<EnablePluginError<Id>>),
}

pub trait PluginManifest {
    type PluginId;

    fn id(&self) -> &Self::PluginId;
}

#[derive(Debug, Default)]
pub struct SimplePluginManifest<Id = &'static str> {
    id: Id,
    description: &'static str,
    dependencies: Vec<Id>,
}

impl<Id> SimplePluginManifest<Id> {
    pub fn new(id: Id, description: &'static str) -> Self {
        Self {
            id,
            description,
            dependencies: Vec::new(),
        }
    }

    pub fn with_dependencies(id: Id, description: &'static str, dependencies: Vec<Id>) -> Self {
        Self {
            id,
            description,
            dependencies,
        }
    }

    pub fn dependencies(&self) -> &[Id] {
        &self.dependencies
    }

    pub fn description(&self) -> &str {
        self.description
    }
}

impl<Id> PluginManifest for SimplePluginManifest<Id> {
    type PluginId = Id;

    fn id(&self) -> &Id {
        &self.id
    }
}

impl<Id> Display for SimplePluginManifest<Id>
where
    Id: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} plugin\n---\n{}", &self.id, &self.description)
    }
}

pub trait Plugin<Id = &'static str, Context = ()>: Any + Send + Sync {
    fn load(&mut self, _context: &mut Context, _hooks: &mut HookRegistry<Id>) {}
    fn unload(&mut self, _context: &mut Context) {}
    fn enable(&mut self, _context: &mut Context) {}
    fn disable(&mut self, _context: &mut Context) {}
}

impl<Id, Context> dyn Plugin<Id, Context> {
    pub fn downcast_ref<T: Plugin<Id, Context>>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref()
    }

    pub fn downcast_mut<T: Plugin<Id, Context>>(&mut self) -> Option<&mut T> {
        (self as &mut dyn Any).downcast_mut()
    }
}

pub type FnPluginConstructor<Id, Context> = fn() -> Box<dyn Plugin<Id, Context>>;

struct PluginState<Manifest, Context>
where
    Manifest: PluginManifest,
{
    manifest: Manifest,
    enabled: bool,
    ctor: Option<FnPluginConstructor<Manifest::PluginId, Context>>,
    plugin: Option<Box<dyn Plugin<Manifest::PluginId, Context>>>,
}

impl<Manifest, Context> PluginState<Manifest, Context>
where
    Manifest: PluginManifest,
{
    fn new(
        manifest: Manifest,
        ctor: Option<FnPluginConstructor<Manifest::PluginId, Context>>,
        plugin: Option<Box<dyn Plugin<Manifest::PluginId, Context>>>,
    ) -> Self {
        Self {
            manifest,
            enabled: false,
            ctor,
            plugin,
        }
    }
}

impl<Manifest, Context> Debug for PluginState<Manifest, Context>
where
    Manifest: PluginManifest + Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginState")
            .field("manifest", &self.manifest)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub struct PluginRegistry<Manifest = SimplePluginManifest, Context = ()>
where
    Manifest: PluginManifest,
{
    plugins: HashMap<Manifest::PluginId, PluginState<Manifest, Context>>,
    hooks: HookRegistry<Manifest::PluginId>,
}

impl<Manifest, Context> PluginRegistry<Manifest, Context>
where
    Manifest: PluginManifest,
    Manifest::PluginId: Eq + Hash,
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
        Manifest::PluginId: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.contains_key(id)
    }

    pub fn is_loaded<Q>(&self, id: &Q) -> bool
    where
        Manifest::PluginId: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins
            .get(id)
            .is_some_and(|state| state.plugin.is_some())
    }

    pub fn is_enabled<Q>(&self, id: &Q) -> bool
    where
        Manifest::PluginId: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get(id).is_some_and(|state| state.enabled)
    }
}

impl<Manifest, Context> PluginRegistry<Manifest, Context>
where
    Manifest: PluginManifest,
{
    pub fn hooks(&self) -> &HookRegistry<Manifest::PluginId> {
        &self.hooks
    }

    pub fn hooks_mut(&mut self) -> &mut HookRegistry<Manifest::PluginId> {
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

    pub fn plugin_ids(&self) -> impl FusedIterator<Item = &Manifest::PluginId> {
        self.plugins.keys()
    }

    pub fn loaded_plugin_ids(&self) -> impl FusedIterator<Item = &Manifest::PluginId> {
        self.plugins
            .iter()
            .filter_map(|(k, p)| p.plugin.is_some().then_some(k))
    }

    pub fn enabled_plugin_ids(&self) -> impl FusedIterator<Item = &Manifest::PluginId> {
        self.plugins
            .iter()
            .filter_map(|(k, p)| p.enabled.then_some(k))
    }
}

impl<Manifest, Context> PluginRegistry<Manifest, Context>
where
    Manifest: PluginManifest,
    Manifest::PluginId: Eq + Hash + Clone,
{
    pub fn register(
        &mut self,
        manifest: Manifest,
        ctor: Option<FnPluginConstructor<Manifest::PluginId, Context>>,
    ) -> bool {
        let id = manifest.id().clone();
        if let hash_map::Entry::Vacant(e) = self.plugins.entry(id) {
            e.insert(PluginState::new(manifest, ctor, None));
            true
        } else {
            false
        }
    }
}

impl<Manifest, Context> PluginRegistry<Manifest, Context>
where
    Manifest: PluginManifest,
    Manifest::PluginId: Eq + Hash + 'static,
    Context: 'static,
{
    pub fn get_manifest<Q>(&self, id: &Q) -> Option<&Manifest>
    where
        Manifest::PluginId: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get(id).map(|s| &s.manifest)
    }

    pub fn remove<Q>(&mut self, id: &Q, context: &mut Context) -> bool
    where
        Manifest::PluginId: Borrow<Q>,
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

    pub fn load<Q>(
        &mut self,
        id: &Q,
        context: &mut Context,
    ) -> Result<(), LoadPluginError<Manifest::PluginId>>
    where
        Manifest::PluginId: Borrow<Q>,
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
        context: &mut Context,
    ) -> Result<(), LoadPluginError<Manifest::PluginId>>
    where
        Manifest::PluginId: Borrow<Q>,
        Q: Eq + Hash,
        P: Into<Box<dyn Plugin<Manifest::PluginId, Context>>>,
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

    pub fn unload<Q>(&mut self, id: &Q, context: &mut Context) -> bool
    where
        Manifest::PluginId: Borrow<Q>,
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

    pub fn enable<Q>(
        &mut self,
        id: &Q,
        context: &mut Context,
    ) -> Result<(), EnablePluginError<Manifest::PluginId>>
    where
        Manifest::PluginId: Borrow<Q>,
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

    pub fn disable<Q>(&mut self, id: &Q, context: &mut Context) -> bool
    where
        Manifest::PluginId: Borrow<Q>,
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

    pub fn get_loaded<T, Q>(&self, id: &Q) -> Option<&T>
    where
        Manifest::PluginId: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<Manifest::PluginId, Context>,
    {
        self.plugins.get(id)?.plugin.as_ref()?.downcast_ref()
    }

    pub fn get_loaded_mut<T, Q>(&mut self, id: &Q) -> Option<&mut T>
    where
        Manifest::PluginId: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<Manifest::PluginId, Context>,
    {
        self.plugins.get_mut(id)?.plugin.as_mut()?.downcast_mut()
    }

    pub fn get_enabled<T, Q>(&self, id: &Q) -> Option<&T>
    where
        Manifest::PluginId: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<Manifest::PluginId, Context>,
    {
        let state = self.plugins.get(id)?;
        if state.enabled {
            state.plugin.as_ref()?.downcast_ref()
        } else {
            None
        }
    }

    pub fn get_enabled_mut<T, Q>(&mut self, id: &Q) -> Option<&mut T>
    where
        Manifest::PluginId: Borrow<Q>,
        Q: Eq + Hash,
        T: Plugin<Manifest::PluginId, Context>,
    {
        let state = self.plugins.get_mut(id)?;
        if state.enabled {
            state.plugin.as_mut()?.downcast_mut()
        } else {
            None
        }
    }
}
