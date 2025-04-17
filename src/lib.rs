//! A modular plugin framework with trait hooks.

#![warn(missing_docs)]

mod hook;
mod plugin;

pub use hook::*;
pub use linkme::distributed_slice as static_plugin_initializer;
pub use plugin::*;

/// Declares a slot for hosting static plugin initializers that can be registered in client plugins
/// by [`register_static_plugin`]. The plugin host can then use
/// [`PluginRegistry::from_initializers`] to automatically discover and register all statically
/// declared plugins.
///
/// This static initialization occurs without any life-before-main and is implemented purely in
/// linker. See [`static_plugin_initializer`] for details.
///
/// # Examples
///
/// ```standalone_crate
/// use biner::{static_plugin_slot, PluginRegistry};
///
/// // Plugin host declares the slot
/// static_plugin_slot!(pub MY_PLUGINS);
///
/// fn init_plugin_host() {
///     let plugins = PluginRegistry::from_initializers(MY_PLUGINS);
///     // ...
/// }
/// # fn main() {
/// #   #[cfg(not(miri))]
/// #   init_plugin_host();
/// # }
/// ```
///
/// If the plugin host wishes to use custom plugin manifests or a plugin context, add the declare
/// the types in the slot.
///
/// ```standalone_crate
/// use biner::{static_plugin_slot, PluginRegistry, SimplePluginManifest};
/// // Plugin host declares the using `i32` as plugin id and a `String` as a plugin context
///
/// static_plugin_slot!(pub MY_PLUGINS<SimplePluginManifest<i32>, String>);
///
/// fn init_plugin_host() {
///     let plugins = PluginRegistry::from_initializers(MY_PLUGINS);
///     // ...
/// }
/// # fn main() {
/// #   #[cfg(not(miri))]
/// #   init_plugin_host();
/// # }
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
/// The plugin host will then be able to consume all registered plugins by using
/// [`PluginRegistry::from_initializers`].
///
/// The provided plugin must implement [`Plugin`] and an expression constructing a manifest for
/// the plugin and an expression constructing the plugin must be provided to this macro.
///
/// This static initialization occurs without any life-before-main and is implemented purely in
/// linker. See [`static_plugin_initializer`] for details.
///
/// # Examples
///
/// ```standalone_crate
/// use biner::{static_plugin_slot, register_static_plugin, Plugin, SimplePluginManifest};
///
/// // Plugin host declares the slot
/// static_plugin_slot!(pub MY_PLUGINS);
///
/// // ...
///
/// // Plugin can be defined in another module or even crate
/// struct MyPlugin;
///
/// impl Plugin for MyPlugin {
///     // ...
/// }
///
/// impl MyPlugin {
///     // Requires some sort of constructor function with this signature
///     fn new_boxed_plugin() -> Box<dyn Plugin> {
///         Box::new(MyPlugin)
///     }
///  }
///
/// // Will add plugin to MY_PLUGINS slot to be accessible from `PluginRegistry::from_initializers`
/// register_static_plugin!{
///     MY_PLUGINS: // The declared plugin slot
///     init_my_plugin // Name of the initializer function that will be added to the slot
///     // Provided plugin manifest, which can be any expression returning a manifest
///     SimplePluginManifest::new("my_plugin", "My plugin example");
///     MyPlugin::new_boxed_plugin // Path to plugin constructor function
///  }
///
/// # fn main() {} // Just needs to compile
/// ```
#[macro_export]
macro_rules! register_static_plugin {
    ($(#[$meta:meta])* $slot:ident $(<$($targ:ty),+>)? : $pub:vis $name:ident $manifest:expr ; $init:expr ) => {
        $(#[$meta])*
        #[$crate::static_plugin_initializer($slot)]
        $pub fn $name(registry: &mut $crate::PluginRegistry$(<$($targ),+>)?) {
            registry.register($manifest, ::std::option::Option::Some($init)).unwrap();
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
///
/// # fn main() {}
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
    use crate::{Plugin, PluginRegistry, SimplePluginManifest};

    static_plugin_slot!(pub TEST_PLUGIN_SLOT);

    struct TestPlugin;

    impl Plugin for TestPlugin {}

    fn new_test_plugin() -> Box<dyn Plugin> {
        Box::new(TestPlugin)
    }

    register_static_plugin! {
        TEST_PLUGIN_SLOT:
        init_test_plugin
        SimplePluginManifest::new("test", "test description");
        new_test_plugin
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn static_plugin_slot() {
        let mut plugins = PluginRegistry::from_initializers(TEST_PLUGIN_SLOT);

        assert_eq!(plugins.plugin_count(), 1);
        assert!(plugins.exists("test"));
        assert_eq!(
            plugins.get_manifest("test").unwrap(),
            &SimplePluginManifest::new("test", "test description")
        );
        assert_eq!(plugins.plugin_ids().collect::<Vec<_>>(), vec!["test"]);

        assert!(!plugins.is_loaded("test"));
        assert!(!plugins.is_enabled("test"));
        assert!(plugins.get_loaded::<TestPlugin>("test").is_none());
        assert!(plugins.get_loaded_plugin("test").is_none());
        assert!(plugins.get_enabled::<TestPlugin>("test").is_none());
        assert!(plugins.get_enabled_plugin("test").is_none());

        let mut context = ();

        plugins.load("test", &mut context).unwrap();
        plugins.enable("test", &mut context).unwrap();

        assert_eq!(plugins.disable("test", &mut ()).into_iter().count(), 1);
        let (unloaded, disabled) = plugins.unload("test", &mut context);
        assert_eq!(unloaded.into_iter().count(), 1);
        assert_eq!(disabled.into_iter().count(), 0);

        plugins.enable("test", &mut context).unwrap();
        let (unloaded, disabled) = plugins.unload("test", &mut context);
        assert_eq!(unloaded.into_iter().count(), 1);
        assert_eq!(disabled.into_iter().count(), 1);

        assert!(plugins.remove("test", &mut context).0);
    }
}
