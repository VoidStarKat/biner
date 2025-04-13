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

/// An error occurred while registering a plugin. It is generic over the type of plugin id used by
/// the plugin system; see [`PluginRegistry`] for more details.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Error)]
pub enum RegisterPluginError<Id> {
    /// A plugin with the same id has already been registered.
    #[error("duplicate plugin `{0}` already registered")]
    Duplicate(Id),
    /// Registering a plugin would introduce a cyclic dependency which cannot be resolved.
    #[error("plugin `{0}` introduces a dependency cycle which cannot be resolved")]
    CyclicDependency(Id),
}

/// An Error occurred while loading a plugin. It is generic over the type of hte plugin id used by
/// the plugin system; see [`PluginRegistry`] for more details.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Error)]
pub enum LoadPluginError<Id> {
    /// No plugin with the given plugin id is currently registered.
    #[error("plugin `{0}` not found")]
    NotFound(Id),
    /// A plugin could not be loaded because it was not registered with a constructor.
    #[error("attempted to load plugin `{0}` that was not registered with a constructor")]
    MissingConstructor(Id),

    /// Plugin could not be loaded because one of its specified dependencies has not been
    /// registered.
    #[error("dependency `{dependency}` required by `{plugin}` not found")]
    DependencyNotFound {
        /// Plugin id of the plugin specifying the dependency.
        plugin: Id,
        /// Plugin id of the dependency that is not registered.
        dependency: Id,
    },
    /// The dependency specified by a plugin was rejected by the plugin's manifest dependency
    /// matching function, [`PluginManifest::dependency_matches`].
    #[error(
        "dependency `{dependency}` required by `{plugin}` does not match plugin requirements: {reason}"
    )]
    DependencyMismatch {
        /// Plugin id of the plugin specifying the dependency
        plugin: Id,
        /// Plugin id of the dependency that did not match the plugin's requirements.
        dependency: Id,
        /// Explanation provided by the plugin for why the plugin rejected the dependency.
        reason: String,
    },
}

/// Metadata about a plugin, including its id and required dependencies. The plugin host can provide
/// a custom manifest format for its plugins, including specifying the type of plugin ids and
/// additional custom metadata. Plugins must then provide instances of the host's manifest type
/// to specify plugin details.
///
/// If no custom metadata behavior is needed for the plugin host, the default
/// [`SimplePluginManifest`] can be used to provide all the necessary basic functionality.
pub trait PluginManifest {
    /// Specifies the type used for plugin ids, which are used throughout the plugin system. This
    /// allows a plugin host to provide appropriate ids for its own needs, such as interned strings,
    /// UUIDs, simple integers, etc.
    type PluginId: Copy + Ord + Hash;

    /// Get the id of the plugin this manifest represents. This id should never change for a plugin.
    fn id(&self) -> Self::PluginId;

    /// A set of required plugin dependencies that must be loaded/enabled prior to loading/enabling
    /// this plugin. It is a runtime error to specify dependencies that result in a cycle of
    /// dependency references.
    fn dependencies(&self) -> &[Self::PluginId] {
        &[]
    }

    /// Determines if the manifest of a plugin dependency specified by [`dependencies`] matches the
    /// dependency requirements for this plugin. This allows complex dependency requirements not
    /// enabled by default such as plugin version requirements or feature flags. If the dependency
    /// manifest does not match the plugin's requirements, a [`String`] [`Err`] detailing the reason
    /// indicates a failed match; an [`Ok`] result is a successful match.
    ///
    /// The default implementation always matches all dependencies without error.
    fn dependency_matches(&self, _dependency: &Self) -> Result<(), String> {
        Ok(())
    }
}

/// A default [`PluginManifest`] providing only the most basic required functionality of a manifest.
/// It is generic over plugin id to still allow easy plugin host choice over the id type.
/// It supports a basic plugin dependency list and a description of the plugin.
#[derive(Debug, Default)]
pub struct SimplePluginManifest<Id = &'static str> {
    id: Id,
    description: &'static str,
    dependencies: Vec<Id>,
}

impl<Id> SimplePluginManifest<Id> {
    /// Create a plugin manifest with a given plugin id and description and no plugin dependencies.
    pub fn new(id: Id, description: &'static str) -> Self {
        Self {
            id,
            description,
            dependencies: Vec::new(),
        }
    }

    /// Create a plugin manifest with a given plugin id and description and a list of plugin
    /// dependencies.
    pub fn with_dependencies(id: Id, description: &'static str, dependencies: Vec<Id>) -> Self {
        Self {
            id,
            description,
            dependencies,
        }
    }

    /// Get the description of the plugin.
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

/// Plugins can be loaded and unloaded by the plug in host. Plugins add functionally to the host
/// by registering hooks.
///
/// # Generic Arguments
///
/// `Id` is the plugin id type used by the host for plugins.
///
/// `Context` is the type of the optional function argument passed to plugin methods.
pub trait Plugin<Id = &'static str, Context = ()>: Any + Send + Sync {
    /// Called when the host requests a plugin be loaded. The plugin should register any hooks
    /// provided by the plugins when loaded and perform any other initialization of the plugin
    /// system.
    fn load(&mut self, _hooks: &mut HookRegistry<Id>, _context: &mut Context) {}

    /// Called when the host unloads this plugin. Hooks registered by this plugin will automatically
    /// be unregistered after unloading.
    fn unload(&mut self, _context: &mut Context) {}

    /// Called when the plugin host enables this plugin's hooks.
    fn enable(&mut self, _context: &mut Context) {}

    /// Called when the plugin host disables this plugin's hooks.
    fn disable(&mut self, _context: &mut Context) {}
}

impl<Id, Context> dyn Plugin<Id, Context> {
    /// Cast this dyn plugin object back into a reference to its concrete type.
    pub fn downcast_ref<T: Plugin<Id, Context>>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref()
    }

    /// Cast this dyn plugin object back into a mutable reference to its concrete type.
    pub fn downcast_mut<T: Plugin<Id, Context>>(&mut self) -> Option<&mut T> {
        (self as &mut dyn Any).downcast_mut()
    }
}

/// Function signature of constructor for a plugin object.
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

/// Manages and discovers plugins for a plugin host.
///
/// # Generic Arguments
///
/// `Manifest` allows the plugin host to specify a custom [`PluginManifest`] for plugins.
/// `Context` is the type that can be passed to [`Plugin`] event methods.
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
    /// Construct an empty plugin registry with a custom hash builder for its internal indexes.
    pub fn with_hasher(hash_builder: S) -> Self {
        Self {
            plugins: HashMap::with_hasher(hash_builder.clone()),
            hooks: HookRegistry::with_hasher(hash_builder.clone()),
            dependency_graph: GraphMap::with_capacity_and_hasher(0, 0, hash_builder),
        }
    }

    /// Construct an empty plugin registry with a custom hash builder for its internal indexes and
    /// an initial plugin capacity count.
    pub fn with_capacity_and_hasher(count: usize, hash_builder: S) -> Self {
        Self {
            plugins: HashMap::with_capacity_and_hasher(count, hash_builder.clone()),
            hooks: HookRegistry::with_hasher(hash_builder.clone()),
            dependency_graph: GraphMap::with_capacity_and_hasher(count, 0, hash_builder),
        }
    }

    /// Construct a plugin registry with a custom hash builder for its internal indexes and register
    /// plugins from all static plugin initializers in the specified `callbacks` slot.
    pub fn from_initializers_with_hasher<'a>(
        callbacks: impl IntoIterator<Item = &'a fn(&mut Self)>,
        hash_builder: S,
    ) -> Self
    where
        Manifest: 'a,
        Context: 'a,
        S: 'a,
    {
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
    /// Construct a empty plugin registry.
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            hooks: HookRegistry::new(),
            dependency_graph: DiGraphMap::new(),
        }
    }

    /// Construct a empty plugin registry with
    pub fn with_capacity(count: usize) -> Self {
        Self {
            plugins: HashMap::with_capacity(count),
            hooks: HookRegistry::new(),
            dependency_graph: GraphMap::with_capacity(count, 0),
        }
    }

    /// Construct a plugin registry and register plugins from all static plugin initializers in the
    /// specified `callbacks` slot.
    pub fn from_initializers<'a>(callbacks: impl IntoIterator<Item = &'a fn(&mut Self)>) -> Self
    where
        Context: 'a,
        Manifest: 'a,
    {
        let iter = callbacks.into_iter();
        let mut this = Self::with_capacity(iter.size_hint().0);
        for f in iter {
            f(&mut this);
        }
        this
    }

    /// Determine whether a plugin with the given plugin id is registered.
    pub fn exists(&self, id: Manifest::PluginId) -> bool {
        self.plugins.contains_key(&id)
    }

    /// Determine whether a plugin with the given plugin id is currently loaded.
    pub fn is_loaded(&self, id: Manifest::PluginId) -> bool {
        self.plugins
            .get(&id)
            .is_some_and(|state| state.plugin.is_some())
    }

    /// Determine whether a plugin with the given plugin id is currently enabled.
    pub fn is_enabled(&self, id: Manifest::PluginId) -> bool {
        self.plugins.get(&id).is_some_and(|state| state.enabled)
    }

    /// Get a reference to the hook registry for managing plugin hooks.
    pub fn hooks(&self) -> &HookRegistry<Manifest::PluginId> {
        &self.hooks
    }

    /// Get a mutalbe reference to hook registry for managing plugin hooks.
    pub fn hooks_mut(&mut self) -> &mut HookRegistry<Manifest::PluginId> {
        &mut self.hooks
    }

    /// Get the number of registered plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Get the number of currently loaded plugins.
    pub fn loaded_plugin_count(&self) -> usize {
        self.plugins.values().filter(|p| p.plugin.is_some()).count()
    }

    /// Get the number of currently enabled plugins.
    pub fn enabled_plugin_count(&self) -> usize {
        self.plugins.values().filter(|p| p.enabled).count()
    }

    /// Get an iterator over all the registered plugin ids.
    pub fn plugin_ids(&self) -> impl FusedIterator<Item = Manifest::PluginId> {
        self.plugins.keys().copied()
    }

    /// Get an iterator over all the currently loaded plugin ids.
    pub fn loaded_plugin_ids(&self) -> impl FusedIterator<Item = Manifest::PluginId> {
        self.plugins
            .iter()
            .filter_map(|(k, p)| p.plugin.is_some().then_some(k))
            .copied()
    }

    /// Get an iterator over all the currently enabled plugin ids.
    pub fn enabled_plugin_ids(&self) -> impl FusedIterator<Item = Manifest::PluginId> {
        self.plugins
            .iter()
            .filter_map(|(k, p)| p.enabled.then_some(k))
            .copied()
    }

    /// Register a plugin if a plugin with the same id as specified in the `manifest` has not
    /// already been registered and return its id. An optional constructor function for the plugin
    /// can also be registered, but without a constructor the only way to load a plugin is with
    /// [`load_with`] providing an instance of the plugin manually.
    ///
    /// Dependency plugins specified in the plugin manifest will need to also be registered before
    /// loading the plugin, and those dependencies *must* have been registered with a constructor
    /// function.
    ///
    /// # Errors
    ///
    /// If a plugin with the same id as specified in the manifest has already been registered, will
    /// return [`RegisterPluginError::Duplicate`].
    ///
    /// If registering this plugin would result in a cycle of plugin dependencies, will return
    /// [`RegisterPluginError::CyclicDependency`].
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

    /// Get a reference to the plugin manifest by its id if that plugin has been registered.
    pub fn get_manifest(&self, id: Manifest::PluginId) -> Option<&Manifest> {
        self.plugins.get(&id).map(|s| &s.manifest)
    }

    /// Get a reference to the dyn plugin object of the plugin with the given id if it is currently
    /// loaded.
    pub fn get_loaded_plugin(
        &self,
        id: Manifest::PluginId,
    ) -> Option<&dyn Plugin<Manifest::PluginId, Context>> {
        self.plugins.get(&id)?.plugin.as_ref().map(AsRef::as_ref)
    }

    /// Get a mutable reference to the dyn plugin object of the plugin with the given id if it is
    /// currently loaded
    pub fn get_loaded_plugin_mut(
        &mut self,
        id: Manifest::PluginId,
    ) -> Option<&mut dyn Plugin<Manifest::PluginId, Context>> {
        self.plugins
            .get_mut(&id)?
            .plugin
            .as_mut()
            .map(AsMut::as_mut)
    }

    /// Get a reference to the plugin with the given id, if it is currently loaded and if the
    /// specified type `T` is the plugin's concrete type.
    pub fn get_loaded<T>(&self, id: Manifest::PluginId) -> Option<&T>
    where
        T: Plugin<Manifest::PluginId, Context>,
    {
        self.get_loaded_plugin(id)?.downcast_ref()
    }

    /// Get a mutable reference to the plugin with the given id, if it is currently loaded and if
    /// the specified type `T` is the plugin's concrete type.
    pub fn get_loaded_mut<T>(&mut self, id: Manifest::PluginId) -> Option<&mut T>
    where
        T: Plugin<Manifest::PluginId, Context>,
    {
        self.plugins.get_mut(&id)?.plugin.as_mut()?.downcast_mut()
    }

    /// Get a reference to the dyn plugin object of the plugin with the given id if it is currently
    /// enabled.
    pub fn get_enabled_plugin(
        &self,
        id: Manifest::PluginId,
    ) -> Option<&dyn Plugin<Manifest::PluginId, Context>> {
        let state = self.plugins.get(&id)?;
        if state.enabled {
            state.plugin.as_ref().map(AsRef::as_ref)
        } else {
            None
        }
    }

    /// Get a mutable reference to the dyn plugin object of the plugin with the given id if it is currently
    /// enabled.
    pub fn get_enabled_plugin_mut(
        &mut self,
        id: Manifest::PluginId,
    ) -> Option<&mut dyn Plugin<Manifest::PluginId, Context>> {
        let state = self.plugins.get_mut(&id)?;
        if state.enabled {
            state.plugin.as_mut().map(AsMut::as_mut)
        } else {
            None
        }
    }

    /// Get a reference to the plugin with the given id, if it is currently enabled and if the
    /// specified type `T` is the plugin's concrete type.
    pub fn get_enabled<T>(&self, id: Manifest::PluginId) -> Option<&T>
    where
        T: Plugin<Manifest::PluginId, Context>,
    {
        self.get_enabled_plugin(id)?.downcast_ref()
    }

    /// Get a mutable reference to the plugin with the given id, if it is currently enabled and if
    /// the specified type `T` is the plugin's concrete type.
    pub fn get_enabled_mut<T>(&mut self, id: Manifest::PluginId) -> Option<&mut T>
    where
        T: Plugin<Manifest::PluginId, Context>,
    {
        self.get_enabled_plugin_mut(id)?.downcast_mut()
    }
}

impl<Manifest, Context> PluginRegistry<Manifest, Context>
where
    Manifest: PluginManifest,
    Manifest::PluginId: 'static,
    Context: 'static,
{
    /// Remove the plugin with the given plugin id and return true if it was registered. If the
    /// plugin was enabled and/or loaded, it will be disabled and unloaded before removal, including
    /// disabling and/or unloading all plugins that list it as a dependency, and returning both an
    /// iterator over all the plugin ids unloaded and an iterator over all the plugin ids disabled.
    pub fn remove(
        &mut self,
        id: Manifest::PluginId,
        context: &mut Context,
    ) -> (
        bool,
        impl IntoIterator<Item = Manifest::PluginId>,
        impl IntoIterator<Item = Manifest::PluginId>,
    ) {
        let mut result = false;
        let mut unloaded = Vec::new();
        let mut disabled = Vec::new();
        if self.plugins.get_mut(&id).is_some() {
            // Ensure unloaded first
            let (dep_unloaded, dep_disabled) = self.unload(id, context);
            unloaded.extend(dep_unloaded);
            disabled.extend(dep_disabled);

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

            result = true;
        }
        (result, unloaded, disabled)
    }

    /// Load the plugin registered with the given plugin id if it is not currently loaded, passing
    /// `context` to the plugin's [`Plugin::load`] method. If this plugin lists any dependencies
    /// in its manifest, attempts to load all of its dependencies before loading the specified
    /// plugin. The plugin will be created using the construction function registered with
    /// the plugin. See [`register`] for more details.
    ///
    /// Use [`load_with`] to bypass the plugin constructor and use a provided plugin instance
    /// instead.
    ///
    /// # Errors
    ///
    /// Returns [`LoadPluginError::NotFound`] if no plugin has been registered with the specified
    /// id.
    ///
    /// If no constructor function was specified when a plugin was registered, returns
    /// [`LoadPluginError::MissingConstructor`].
    ///
    /// Returns [`LoadPluginError::DependencyNotFound`] if any dependencies have not been
    /// registered.
    ///
    /// If the plugin's manifest determines a dependency does not match using
    /// [`PluginManifest::dependency_matches`], returns [`LoadPluginError::DependencyMismatch`].
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

    /// Load the plugin registered with the given plugin id if it is not currently loaded, passing
    /// `context` to the plugin's [`Plugin::load`] method. If this plugin lists any dependencies
    /// in its manifest, attempts to load all of its dependencies before loading the specified
    /// plugin.
    ///
    /// Unlike [`load`], does not use the plugin's registered constructor, if any, and instead uses
    /// the provided `plugin` instance.
    ///
    /// # Errors
    ///
    /// Returns [`LoadPluginError::NotFound`] if no plugin has been registered with the specified
    /// id.
    ///
    /// If no constructor function was specified when a plugin was registered, returns
    /// [`LoadPluginError::MissingConstructor`].
    ///
    /// Returns [`LoadPluginError::DependencyNotFound`] if any dependencies have not been
    /// registered.
    ///
    /// If the plugin's manifest determines a dependency does not match using
    /// [`PluginManifest::dependency_matches`], returns [`LoadPluginError::DependencyMismatch`].
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

    /// Unload the plugin with the given plugin id. If the plugin was enabled, it will be disabled
    /// before unloading, including disabling all plugins that list it as a dependency. All plugins
    /// that list this plugin as a dependency will be unloaded before unloading this plugin.
    /// Returns both an iterator over all the plugin ids unloaded and an iterator over all the
    /// plugin ids disabled.
    pub fn unload(
        &mut self,
        id: Manifest::PluginId,
        context: &mut Context,
    ) -> (
        impl IntoIterator<Item = Manifest::PluginId>,
        impl IntoIterator<Item = Manifest::PluginId>,
    ) {
        let mut unloaded = Vec::new();
        let mut disabled = Vec::new();
        if self
            .plugins
            .get_mut(&id)
            .is_some_and(|state| state.plugin.is_some())
        {
            // Disable first
            disabled.extend(self.disable(id, context));

            // Unload downstream dependents first
            let mut dependents = self
                .dependency_graph
                .edges(id)
                .map(|(_, d, &i)| (d, i))
                .collect::<Vec<_>>();
            dependents.sort_unstable_by_key(|(_, i)| *i);
            for (dep, _) in dependents.into_iter().rev() {
                let (dep_unloaded, dep_disabled) = self.unload(dep, context);
                unloaded.extend(dep_unloaded);
                disabled.extend(dep_disabled);
            }

            let state = &mut self.plugins.get_mut(&id).unwrap();
            state.plugin.as_mut().unwrap().unload(context);
            self.hooks.remove_plugin_hooks(id);
            unloaded.push(id);
        }
        (unloaded, disabled)
    }

    /// Enable the plugin loaded with the given plugin id if it is not currently enabled, passing
    /// `context` to the plugin's [`Plugin::enable`] method. If this plugin lists any dependencies
    /// in its manifest, attempts to enable all of its dependencies before enabling the specified
    /// plugin. If the plugin has not been loaded yet, will [`load`] the plugin first.
    ///
    /// # Errors
    ///
    /// Returns [`LoadPluginError::NotFound`] if no plugin has been registered with the specified
    /// id.
    ///
    /// If no constructor function was specified when a plugin was registered, returns
    /// [`LoadPluginError::MissingConstructor`].
    ///
    /// Returns [`LoadPluginError::DependencyNotFound`] if any dependencies have not been
    /// registered.
    ///
    /// If the plugin's manifest determines a dependency does not match using
    /// [`PluginManifest::dependency_matches`], returns [`LoadPluginError::DependencyMismatch`].
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

    /// Disable the plugin with the given plugin id. All plugins that list this plugin as a
    /// dependency will be disabled before unloading this plugin. Returns an iterator over all the
    /// plugin ids disabled.
    pub fn disable(
        &mut self,
        id: Manifest::PluginId,
        context: &mut Context,
    ) -> impl IntoIterator<Item = Manifest::PluginId> {
        let mut disabled = Vec::new();
        if self.plugins.get_mut(&id).is_some_and(|state| state.enabled) {
            // Ensure downstream dependents are all disabled first
            let mut dependents = self
                .dependency_graph
                .edges_directed(id, Incoming)
                .map(|(_, d, &i)| (d, i))
                .collect::<Vec<_>>();
            dependents.sort_unstable_by_key(|(_, i)| *i);
            for (dep, _) in dependents.into_iter().rev() {
                disabled.extend(self.disable(dep, context));
            }

            let state = &mut self.plugins.get_mut(&id).unwrap();
            state.plugin.as_mut().unwrap().disable(context);
            state.enabled = false;
            disabled.push(id);
        }
        disabled
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

impl<'a, Manifest, Context, S> FromIterator<&'a fn(&mut Self)>
    for PluginRegistry<Manifest, Context, S>
where
    Manifest: PluginManifest + 'a,
    Context: 'a,
    S: BuildHasher + Default + Clone + 'a,
{
    fn from_iter<T: IntoIterator<Item = &'a fn(&mut Self)>>(iter: T) -> Self {
        PluginRegistry::from_initializers_with_hasher(iter, S::default())
    }
}
