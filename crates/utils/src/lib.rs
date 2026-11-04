/// An easy way to create HashMaps
#[macro_export]
macro_rules! map(
    { $($key:expr => $value:expr),+ $(,)? } => {
        {
            let mut m = ::std::collections::HashMap::new();
            $(
                m.insert($key.into(), $value);
            )+
            m
        }
     };
);

#[macro_export]
macro_rules! nbt_unwrap_val {
    // Accept qualified NBT variant constructors such as nbt::Value::Compound.
    ($e:expr, $p:path) => {
        match $e {
            Some($p(val)) => val,
            _ => anyhow::bail!("Expected {} but got {:?}", stringify!($p), $e),
        }
    };
}
