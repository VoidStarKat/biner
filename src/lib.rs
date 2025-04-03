#[cfg(all(feature = "downcast-rs", feature = "downcast"))]
compile_error!(
    "Cargo features 'downcast-rs' and 'downcast' are mutually exclusive features and
 cannot both be enabled at the same time"
);

mod hook;
mod plugin;

pub use hook::*;
pub use linkme::distributed_slice as static_plugin_initializer;
pub use plugin::*;

#[macro_export]
macro_rules! declare_static_plugin_slot {
    ($pub:vis $name:ident $(<$($targ:ty),*>)?) => {
        #[$crate::static_plugin_initializer]
        $pub static $name: [fn(&mut $crate::PluginRegistry$(<$($targ),+>)?)];
    };
}

#[macro_export]
macro_rules! register_static_plugin {
    ($(#[$meta:meta])* $name:ident @ $slot:ident $(<$($targ:ty),+>)? : $manifest:expr => $init:expr ) => {
        $(#[$meta])*
        #[$crate::static_plugin_initializer($slot)]
        fn $name(registry: &mut $crate::PluginRegistry$(<$($targ),+>)?) {
            registry.register($manifest, Some($init));
        }
    };
}
