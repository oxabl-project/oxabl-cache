macro_rules! secret_key {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
        pub struct $name([u8; 32]);

        impl $name {
            /// Wraps 32 bytes of key material.
            pub fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

            pub(crate) fn bytes(&self) -> &[u8; 32] {
                &self.0
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

pub(crate) use secret_key;
