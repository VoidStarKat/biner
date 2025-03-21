use std::{
    any::Any,
    borrow::Borrow,
    collections::{HashMap, hash_map},
    fmt::Debug,
    hash::Hash,
    iter::FusedIterator,
};

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

impl<S> PluginManifest<S> for SimplePluginManifest<S>
where
    S: AsRef<str>,
{
    fn id(&self) -> &S {
        &self.id
    }
}

pub trait Plugin<C>: Any + Send + Sync {
    fn load(&mut self, _: &mut C) {}
    fn unload(&mut self, _: &mut C) {}
    fn enable(&mut self, _: &mut C) {}
    fn disable(&mut self, _: &mut C) {}
}

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
    S: AsRef<str>,
{
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    pub fn plugin_ids(&self) -> PluginIdIter<M, C, S> {
        PluginIdIter {
            iter: self.plugins.keys(),
        }
    }
}

impl<M, C, S> PluginRegistry<M, C, S>
where
    M: PluginManifest<S>,
    S: AsRef<str> + Eq + Hash,
{
    pub fn get_manifest<Q>(&self, id: &Q) -> Option<&M>
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get(id).map(|s| &s.manifest)
    }

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

    pub fn load_plugin<Q>(&mut self, manifest: M, plugin: Q, context: &mut C) -> bool
    where
        Q: Into<Box<dyn Plugin<C>>>,
        S: Clone,
        C: 'static,
    {
        let id = manifest.id().clone();
        if self.plugins.contains_key(&id) {
            false
        } else {
            // TODO: Clean up these clones
            let id = manifest.id().clone();
            self.plugins
                .insert(id.clone(), PluginState::new(manifest, plugin.into()))
                .unwrap();
            let plugin = self.get_plugin_mut(&id).unwrap();
            plugin.load(context);

            true
        }
    }

    pub fn unload_plugin<Q>(&mut self, id: &Q, context: &mut C) -> bool
    where
        S: Borrow<Q>,
        Q: Eq + Hash,
        C: 'static,
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
        C: 'static,
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
        C: 'static,
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

impl<'_, M, C, S> FusedIterator for PluginIdIter<'_, M, C, S> where S: AsRef<str> {}

impl<'_, M, C, S> ExactSizeIterator for PluginIdIter<'_, M, C, S> where S: AsRef<str> {}
