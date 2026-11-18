use serde::{Deserialize, Serialize};

/// The count of confirmed receptacles visible in a single photo.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct PowerOutletCount {
    pub confirmed: u32,
    pub uncertain: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct PowerOutletComparison {
    pub previous: Option<PowerOutletCount>,
    pub current: Option<PowerOutletCount>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum PowerOutletResult {
    Photo(PowerOutletCount),
    Comparison(PowerOutletComparison),
}
