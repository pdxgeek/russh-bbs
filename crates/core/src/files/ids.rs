//! Ids minted by the store.

use std::fmt;

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(i64);

        impl $name {
            /// Wrap a stored id.
            #[must_use]
            pub const fn from_raw(raw: i64) -> Self {
                Self(raw)
            }

            /// The raw id, for a query.
            #[must_use]
            pub const fn as_raw(self) -> i64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id!(
    /// A file area.
    AreaId
);
id!(
    /// A file.
    FileId
);

impl AreaId {
    /// The area as an access resource.
    #[must_use]
    pub fn resource(self) -> Option<crate::access::ResourceRef> {
        crate::access::ResourceId::new(self.0).map(|id| crate::access::ResourceRef {
            kind: crate::access::ResourceKind::FileArea,
            id,
        })
    }
}
