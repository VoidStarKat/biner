#[cfg(all(feature = "downcast-rs", feature = "downcast"))]
compile_error!(
    "Cargo features 'downcast-rs' and 'downcast' are mutually exclusive features and
 cannot both be enabled at the same time"
);

pub mod hook;
pub mod plugin;
