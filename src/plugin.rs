use crate::HookRegistry;
use petgraph::algo;
use petgraph::prelude::*;
use std::fmt::Display;
use std::hash::{BuildHasher, RandomState};
use std::{
    any::Any,
    collections::{HashMap, hash_map},
    fmt::Debug,
    hash::Hash,
    iter::FusedIterator,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Error)]
pub enum RegisterPluginError<Id> {
    #[error("duplicate plugin `{0}` already registered")]
    Duplicate(Id),
    #[error("plugin `{0}` introduces a dependency cycle which cannot be resolved")]
    CyclicDependency(Id),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Error)]
pub enum LoadPluginError<Id> {
    #[error("plugin `{0}` not found")]
    NotFound(Id),
    #[error("attempted to load plugin `{0}` that was not registered with a constructor")]
    MissingConstructor(Id),

    #[error("dependency `{dependency}` required by `{plugin}` not found")]
    DependencyNotFound { plugin: Id, dependency: Id },
    #[error(
        "dependency `{dependency}` required by `{plugin}` does not match plugin requirements: {reason}"
    )]
    DependencyMismatch {
        plugin: Id,
        dependency: Id,
        reason: String,
    },
}

pub trait PluginManifest {
    type PluginId: Copy + Ord + Hash;

    fn id(&self) -> Self::PluginId;

    fn dependencies(&self) -> &[Self::PluginId] {
        &[]
    }

    fn dependency_matches(&self, _dependency: &Self) -> Result<(), String> {
        Ok(())
    }
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

    pub fn description(&self) -> &str {
        self.description
    }
}

impl<Id> PluginManifest for SimplePluginManifest<Id>
where
    Id: Copy + Ord + Hash,
{
    type PluginId = Id;

    fn id(&self) -> Id {
        self.id
    }

    fn dependencies(&self) -> &[Id] {
        &self.dependencies
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
    fn load(&mut self, _hooks: &mut HookRegistry<Id>, _context: &mut Context) {}
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

#[derive(Debug)]
pub struct PluginRegistry<Manifest = SimplePluginManifest, Context = (), S = RandomState>
where
    Manifest: PluginManifest,
    S: BuildHasher,
{
    plugins: HashMap<Manifest::PluginId, PluginState<Manifest, Context>, S>,
    hooks: HookRegistry<Manifest::PluginId, S>,
    dependency_graph: GraphMap<Manifest::PluginId, usize, Directed, S>,
}

impl<Manifest, Context, S> PluginRegistry<Manifest, Context, S>
where
    Manifest: PluginManifest,
    S: BuildHasher + Clone,
{
    pub fn with_hasher(hash_builder: S) -> Self {
        Self {
            plugins: HashMap::with_hasher(hash_builder.clone()),
            hooks: HookRegistry::with_hasher(hash_builder.clone()),
            dependency_graph: GraphMap::with_capacity_and_hasher(0, 0, hash_builder),
        }
    }

    pub fn with_capacity_and_hasher(count: usize, hash_builder: S) -> Self {
        Self {
            plugins: HashMap::with_capacity_and_hasher(count, hash_builder.clone()),
            hooks: HookRegistry::with_hasher(hash_builder.clone()),
            dependency_graph: GraphMap::with_capacity_and_hasher(count, 0, hash_builder),
        }
    }

    pub fn from_initializers_with_hasher(
        callbacks: impl IntoIterator<Item = fn(&mut Self)>,
        hash_builder: S,
    ) -> Self {
        let iter = callbacks.into_iter();
        let mut this = Self::with_capacity_and_hasher(iter.size_hint().0, hash_builder);
        for f in iter {
            f(&mut this);
        }
        this
    }
}

impl<Manifest, Context> PluginRegistry<Manifest, Context>
where
    Manifest: PluginManifest,
{
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            hooks: HookRegistry::new(),
            dependency_graph: DiGraphMap::new(),
        }
    }

    pub fn with_capacity(count: usize) -> Self {
        Self {
            plugins: HashMap::with_capacity(count),
            hooks: HookRegistry::new(),
            dependency_graph: GraphMap::with_capacity(count, 0),
        }
    }

    pub fn from_initializers(callbacks: impl IntoIterator<Item = fn(&mut Self)>) -> Self {
        let iter = callbacks.into_iter();
        let mut this = Self::with_capacity(iter.size_hint().0);
        for f in iter {
            f(&mut this);
        }
        this
    }

    pub fn exists(&self, id: Manifest::PluginId) -> bool {
        self.plugins.contains_key(&id)
    }

    pub fn is_loaded(&self, id: Manifest::PluginId) -> bool {
        self.plugins
            .get(&id)
            .is_some_and(|state| state.plugin.is_some())
    }

    pub fn is_enabled(&self, id: Manifest::PluginId) -> bool {
        self.plugins.get(&id).is_some_and(|state| state.enabled)
    }

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

    pub fn plugin_ids(&self) -> impl FusedIterator<Item = Manifest::PluginId> {
        self.plugins.keys().copied()
    }

    pub fn loaded_plugin_ids(&self) -> impl FusedIterator<Item = Manifest::PluginId> {
        self.plugins
            .iter()
            .filter_map(|(k, p)| p.plugin.is_some().then_some(k))
            .copied()
    }

    pub fn enabled_plugin_ids(&self) -> impl FusedIterator<Item = Manifest::PluginId> {
        self.plugins
            .iter()
            .filter_map(|(k, p)| p.enabled.then_some(k))
            .copied()
    }

    pub fn register(
        &mut self,
        manifest: Manifest,
        ctor: Option<FnPluginConstructor<Manifest::PluginId, Context>>,
    ) -> Result<Manifest::PluginId, RegisterPluginError<Manifest::PluginId>> {
        let id = manifest.id();
        if let hash_map::Entry::Vacant(e) = self.plugins.entry(id) {
            let state = &mut e.insert(PluginState::new(manifest, ctor, None));

            // Setup dependencies
            self.dependency_graph.add_node(id);
            for (i, &dep) in state.manifest.dependencies().iter().enumerate() {
                self.dependency_graph.add_edge(id, dep, i);
            }
            // Check for cycles
            if algo::is_cyclic_directed(&self.dependency_graph) {
                // Rollback graph additions
                for dep in self.dependency_graph.neighbors(id).collect::<Vec<_>>() {
                    self.dependency_graph.remove_edge(id, dep);
                    if self
                        .dependency_graph
                        .neighbors_directed(dep, Incoming)
                        .next()
                        .is_none()
                    {
                        self.dependency_graph.remove_node(dep);
                    }
                }

                if self
                    .dependency_graph
                    .neighbors_directed(id, Incoming)
                    .next()
                    .is_none()
                {
                    self.dependency_graph.remove_node(id);
                }
            }

            Ok(id)
        } else {
            Err(RegisterPluginError::Duplicate(id))
        }
    }

    pub fn get_manifest(&self, id: Manifest::PluginId) -> Option<&Manifest> {
        self.plugins.get(&id).map(|s| &s.manifest)
    }

    pub fn get_loaded<T>(&self, id: Manifest::PluginId) -> Option<&T>
    where
        T: Plugin<Manifest::PluginId, Context>,
    {
        self.plugins.get(&id)?.plugin.as_ref()?.downcast_ref()
    }

    pub fn get_loaded_mut<T>(&mut self, id: Manifest::PluginId) -> Option<&mut T>
    where
        T: Plugin<Manifest::PluginId, Context>,
    {
        self.plugins.get_mut(&id)?.plugin.as_mut()?.downcast_mut()
    }

    pub fn get_enabled<T>(&self, id: Manifest::PluginId) -> Option<&T>
    where
        T: Plugin<Manifest::PluginId, Context>,
    {
        let state = self.plugins.get(&id)?;
        if state.enabled {
            state.plugin.as_ref()?.downcast_ref()
        } else {
            None
        }
    }

    pub fn get_enabled_mut<T>(&mut self, id: Manifest::PluginId) -> Option<&mut T>
    where
        T: Plugin<Manifest::PluginId, Context>,
    {
        let state = self.plugins.get_mut(&id)?;
        if state.enabled {
            state.plugin.as_mut()?.downcast_mut()
        } else {
            None
        }
    }
}

impl<Manifest, Context> PluginRegistry<Manifest, Context>
where
    Manifest: PluginManifest,
    Manifest::PluginId: 'static,
    Context: 'static,
{
    pub fn remove(&mut self, id: Manifest::PluginId, context: &mut Context) -> bool {
        if let Some(state) = self.plugins.get_mut(&id) {
            if state.plugin.is_some() {
                let plugin = state.plugin.as_mut().unwrap();
                if state.enabled {
                    plugin.disable(context);
                    state.enabled = false;
                }
                plugin.unload(context);
                self.hooks.remove_plugin_hooks(id);
            }
            self.plugins.remove(&id);

            // Cleanup dependency graph, removing node if it has not incoming dependencies
            if self
                .dependency_graph
                .neighbors_directed(id, Incoming)
                .next()
                .is_none()
            {
                self.dependency_graph.remove_node(id);
            }

            true
        } else {
            false
        }
    }

    pub fn load(
        &mut self,
        id: Manifest::PluginId,
        context: &mut Context,
    ) -> Result<(), LoadPluginError<Manifest::PluginId>> {
        if self
            .plugins
            .get_mut(&id)
            .ok_or(LoadPluginError::NotFound(id))?
            .plugin
            .is_none()
        {
            self.load_dependencies(id, context)?;

            let state = &mut self.plugins.get_mut(&id).unwrap();
            state
                .plugin
                .insert(state.ctor.ok_or(LoadPluginError::MissingConstructor(id))?())
                .load(&mut self.hooks, context);
        }
        Ok(())
    }

    fn load_dependencies(
        &mut self,
        id: Manifest::PluginId,
        context: &mut Context,
    ) -> Result<(), LoadPluginError<<Manifest as PluginManifest>::PluginId>> {
        let mut dependencies = self
            .dependency_graph
            .edges(id)
            .map(|(_, d, &i)| (d, i))
            .collect::<Vec<_>>();
        dependencies.sort_unstable_by_key(|(_, i)| *i);
        for (dep, _) in dependencies {
            let [state, dep_state] = self.plugins.get_disjoint_mut([&id, &dep]);
            let dep_state = dep_state.ok_or(LoadPluginError::DependencyNotFound {
                plugin: id,
                dependency: dep,
            })?;

            // Ensure the dependency is loaded
            if dep_state.plugin.is_none() {
                state
                    .unwrap()
                    .manifest
                    .dependency_matches(&dep_state.manifest)
                    .map_err(|reason| LoadPluginError::DependencyMismatch {
                        plugin: id,
                        dependency: dep,
                        reason,
                    })?;

                self.load(dep, context)?;
            }
        }
        Ok(())
    }

    pub fn load_with<P>(
        &mut self,
        id: Manifest::PluginId,
        plugin: P,
        context: &mut Context,
    ) -> Result<(), LoadPluginError<Manifest::PluginId>>
    where
        P: Into<Box<dyn Plugin<Manifest::PluginId, Context>>>,
    {
        if self
            .plugins
            .get_mut(&id)
            .ok_or(LoadPluginError::NotFound(id))?
            .plugin
            .is_none()
        {
            self.load_dependencies(id, context)?;

            let state = &mut self.plugins.get_mut(&id).unwrap();
            state
                .plugin
                .insert(plugin.into())
                .load(&mut self.hooks, context);
        }
        Ok(())
    }

    pub fn unload(&mut self, id: Manifest::PluginId, context: &mut Context) -> bool {
        if let Some(state) = self.plugins.get_mut(&id) {
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

    pub fn enable(
        &mut self,
        id: Manifest::PluginId,
        context: &mut Context,
    ) -> Result<(), LoadPluginError<Manifest::PluginId>> {
        if !self
            .plugins
            .get_mut(&id)
            .ok_or(LoadPluginError::NotFound(id))?
            .enabled
        {
            // Ensure plugin already loaded
            self.load(id, context)?;

            // Ensure dependencies are all enabled
            let mut dependencies = self
                .dependency_graph
                .edges(id)
                .map(|(_, d, &i)| (d, i))
                .collect::<Vec<_>>();
            dependencies.sort_unstable_by_key(|(_, i)| *i);
            for (dep, _) in dependencies {
                self.enable(dep, context)?;
            }

            let state = &mut self.plugins.get_mut(&id).unwrap();
            state.plugin.as_mut().unwrap().enable(context);
            state.enabled = true;
        }
        Ok(())
    }

    pub fn disable(&mut self, id: Manifest::PluginId, context: &mut Context) -> bool {
        if let Some(state) = self.plugins.get_mut(&id) {
            if state.enabled {
                state.plugin.as_mut().unwrap().disable(context);
                state.enabled = false;
            }
            true
        } else {
            false
        }
    }
}

impl<Manifest, Context, S> Default for PluginRegistry<Manifest, Context, S>
where
    Manifest: PluginManifest,
    S: BuildHasher + Default,
{
    fn default() -> Self {
        Self {
            plugins: HashMap::default(),
            hooks: HookRegistry::default(),
            dependency_graph: GraphMap::default(),
        }
    }
}

impl<Manifest, Context, S> FromIterator<fn(&mut Self)> for PluginRegistry<Manifest, Context, S>
where
    Manifest: PluginManifest,
    S: BuildHasher + Default + Clone,
{
    fn from_iter<T: IntoIterator<Item = fn(&mut Self)>>(iter: T) -> Self {
        PluginRegistry::from_initializers_with_hasher(iter, S::default())
    }
}
