//! Wire shape of the lookup for people a ticket can be assigned to (Jira and GitHub).

use super::*;

/// Response to `GET /api/jira/assignable?project=…&query=…` and
/// `GET /api/github/assignable?repo=…&query=…`: people the service would let you assign tickets
/// to.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AssignableResponse {
    /// `false` if the service is not set up or the key is missing (the list is then empty).
    pub active: bool,
    /// You yourself come as [`crate::Person::me`].
    #[serde(default)]
    pub people: Vec<crate::Person>,
}
