use std::collections::HashMap;
use std::ops::RangeInclusive;

const DEFAULT_RATING_RANGE: RangeInclusive<u64> = 2000..=2400;

#[derive(Clone, Debug, Default)]
pub struct ChannelState {
    pub(super) registered_users: HashMap<String, String>,
    pub(super) rating_range: Option<RangeInclusive<u64>>,
}

impl ChannelState {
    pub fn rating_range(&self) -> &RangeInclusive<u64> {
        self.rating_range.as_ref().unwrap_or(&DEFAULT_RATING_RANGE)
    }
}
