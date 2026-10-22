// Utilities module - helper functions and common operations

use super::*;

use alloc::collections::BTreeMap;
use core::fmt::Debug;

impl Manager {
    /// Find the first available identifier for any identifier type with into_inner() method
    pub(crate) fn find_first_available_identifier<RawIdentifier, Identifier, V>(
        map: &BTreeMap<Identifier, V>,
        mut range: impl Iterator<Item = RawIdentifier>,
    ) -> Option<Identifier>
    where
        Identifier: PartialEq<RawIdentifier> + From<RawIdentifier> + Debug,
        RawIdentifier: Debug,
    {
        for key in map.keys() {
            {
                let test_key = range.next()?;
                if *key != test_key {
                    return Some(test_key.into());
                }
            }
        }

        range.next().map(|key| key.into())
    }

    pub(crate) fn get_task(inner: &Inner, task_identifier: TaskIdentifier) -> Result<&Metadata> {
        inner
            .tasks
            .get(&task_identifier)
            .ok_or(Error::InvalidTaskIdentifier)
    }

    pub(crate) fn get_task_mutable(
        inner: &mut Inner,
        task_identifier: TaskIdentifier,
    ) -> Result<&mut Metadata> {
        inner
            .tasks
            .get_mut(&task_identifier)
            .ok_or(Error::InvalidTaskIdentifier)
    }
}
