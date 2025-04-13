//! A modular plugin framework with trait hooks.

#![warn(missing_docs)]

mod hook;
mod plugin;

pub use hook::*;
pub use linkme::distributed_slice as static_plugin_initializer;
pub use plugin::*;

/// Declares a slot for hosting static plugin initializers that can be registered in client plugins
/// by [`register_static_plugin`]. The plugin host can then use
/// [`PluginRegistry::from_initializers`] to automatically discover and register all staticly
/// declared plugins.
///
/// This static initialization occurs without any life-before-main and is implemented purely in
/// linker. See [`static_plugin_initializer`] for details.
///
/// # Examples
///
/// ```ignore
/// use biner::{static_plugin_slot, PluginRegistry};
///
/// // Plugin host declares the slot
/// static_plugin_slot!(pub MY_PLUGINS);
/// fn init_plugin_host() {
///     let plugins = PluginRegistry::from_initializers(MY_PLUGINS);
///     // ...
/// }
/// # init_plugin_host()
/// ```
#[macro_export]
macro_rules! static_plugin_slot {
    ($(#[$meta:meta])* $pub:vis $name:ident $(<$($targ:ty),*>)?) => {
        $(#[$meta])*
        #[$crate::static_plugin_initializer]
        $pub static $name: [fn(&mut $crate::PluginRegistry$(<$($targ),+>)?)];
    };
}

/// Registers a plugin to a static plugin slot to be later discovered by the plugin host. The
/// plugin slot must already have been declared using [`static_plugin_slot`] by the plugin host.
///
/// The provided plugin must implement [`Plugin`] and an expression constructing a manifest for
/// the plugin and an expression constructing the plugin must be provided to this macro.
///
/// This static initialization occurs without any life-before-main and is implemented purely in
/// linker. See [`static_plugin_initializer`] for details.
///
/// # Examples
///
/// ```ignore
/// use biner::{static_plugin_slot, register_static_plugin, Plugin, SimplePluginManifest};
///
/// // Plugin host declares the slot
/// static_plugin_slot!(pub MY_PLUGINS);
///
/// struct MyPlugin;
///
/// impl Plugin for MyPlugin {}
///
/// impl MyPlugin {
///     fn new() -> MyPlugin {
///         MyPlugin
///     }
/// }
///
/// register_static_plugin!{
///     MY_PLUGINS:
///     MyPlugin
///     SimplePluginManifest::new("my_plugin", "My plugin example");
///     MyPlugin::new()
/// }
/// ```
#[macro_export]
macro_rules! register_static_plugin {
    ($(#[$meta:meta])* $slot:ident $(<$($targ:ty),+>)? : $name:ident $manifest:expr ; $init:expr ) => {
        $(#[$meta])*
        #[$crate::static_plugin_initializer($slot)]
        fn $name(registry: &mut $crate::PluginRegistry$(<$($targ),+>)?) {
            registry.register($manifest, ::std::option::Option::Some($init));
        }
    };
}

/// Declares a hook slot for plugins to register hooks. A hook slot is simply a zero-sized type
/// that is used to access the slot in methods and has the trait required for the hook as a dyn
/// associated type. Plugins and host can then use methods on [`HookRegistry`] to register or access
/// hooks attached to that slot.
///
/// # Examples
///
/// ```
/// use biner::hook_slot;
///
/// pub trait MyHookTrait: Send + Sync {
///     // ...
/// }
///
/// hook_slot!(pub MyHookSlot: dyn MyHookTrait);
/// ```
#[macro_export]
macro_rules! hook_slot {
    ($(#[$meta:meta])* $pub:vis $name:ident : $traitobj:ty) => {
        $(#[$meta])*
        #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        $pub struct $name;

        impl $crate::HookSlot for $name {
            type TraitObject = $traitobj;
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::PluginRegistry;

    static_plugin_slot!(pub TEST_PLUGIN_SLOT);

    #[test]
    fn static_plugin_slot() {
        let _ = PluginRegistry::from_initializers(TEST_PLUGIN_SLOT);
    }
}
