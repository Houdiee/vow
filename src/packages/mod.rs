//! The package half: each file's `[packages]` lists, merged ([`resolve`]),
//! checked ([`validate`]) and compared with what is installed ([`plan`]).

mod origin;
mod plan;
mod resolve;
mod spec;
mod validate;
mod version;

#[cfg(test)]
pub use origin::Origin;
pub use plan::{Convergence, Plan, Repin, plan_all};
pub use resolve::{Resolved, is_managed, resolve, warnings};
pub use spec::PackageSpec;
pub use validate::{declared, is_pinned};
