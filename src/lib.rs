use std::{
    any::Any,
    borrow::Borrow,
    collections::{HashMap, hash_map},
    fmt::Debug,
    hash::Hash,
    iter::FusedIterator,
    ops::Deref,
};

pub trait PluginManifest<Id>
where
    Id: Deref,
{
    fn id(&self) -> &<Id as Deref>::Target;
}

#[derive(Debug, Default)]
pub struct SimplePluginManifest<Id = String> {
    id: Id,
    description: String,
}

impl<Id> SimplePluginManifest<Id> {
    pub fn new(id: Id, description: String) -> Self {
        Self { id, description }
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

impl<Id> PluginManifest<Id> for SimplePluginManifest<Id>
where
    Id: Deref,
{
    fn id(&self) -> &<Id as Deref>::Target {
        self.id.deref()
    }
}

pub trait Plugin<Context>: Any + Send + Sync {
    fn load(&mut self, _: &mut Context) {}
    fn unload(&mut self, _: &mut Context) {}
    fn enable(&mut self, _: &mut Context) {}
    fn disable(&mut self, _: &mut Context) {}
}

struct PluginState<Manifest, Context> {
    manifest: Manifest,
    plugin: Box<dyn Plugin<Context>>,
    enabled: bool,
}

impl<Manifest, Context> std::fmt::Debug for PluginState<Manifest, Context>
where
    Manifest: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginState")
            .field("manifest", &self.manifest)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub struct PluginRegistry<Manifest, Context, Id = String> {
    plugins: HashMap<Id, PluginState<Manifest, Context>>,
}

impl<Manifest, Context, Id> PluginRegistry<Manifest, Context, Id>
where
    Manifest: PluginManifest<Id>,
    Id: Deref,
{
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    pub fn plugin_ids(&self) -> PluginIdIter<Manifest, Context, Id> {
        PluginIdIter {
            iter: self.plugins.keys(),
        }
    }
}

impl<Manifest, Context, Id> PluginRegistry<Manifest, Context, Id>
where
    Manifest: PluginManifest<Id>,
    Id: Deref + Eq + Hash,
{
    pub fn get_manifest<Q>(&self, id: &Q) -> Option<&Manifest>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get(id).map(|s| &s.manifest)
    }

    pub fn is_enabled<Q>(&self, id: &Q) -> Option<bool>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get(id).map(|s| s.enabled)
    }

    pub fn get_plugin<Q>(&self, id: &Q) -> Option<&dyn Plugin<Context>>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get(id).map(|s| s.plugin.as_ref())
    }

    pub fn get_plugin_mut<Q>(&mut self, id: &Q) -> Option<&mut dyn Plugin<Context>>
    where
        Id: Borrow<Q>,
        Q: Eq + Hash,
    {
        self.plugins.get_mut(id).map(|s| s.plugin.as_mut())
    }
}

#[derive(Debug)]
pub struct PluginIdIter<'a, Manifest, Context, Id> {
    iter: hash_map::Keys<'a, Id, PluginState<Manifest, Context>>,
}

impl<'a, Manifest, Context, Id> Iterator for PluginIdIter<'a, Manifest, Context, Id>
where
    Id: Deref,
{
    type Item = &'a <Id as Deref>::Target;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|id| id.deref())
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

impl<'a, Manifest, Context, Id> FusedIterator for PluginIdIter<'a, Manifest, Context, Id> where
    Id: Deref
{
}

impl<'a, Manifest, Context, Id> ExactSizeIterator for PluginIdIter<'a, Manifest, Context, Id> where
    Id: Deref
{
}
