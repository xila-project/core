use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, rwlock::RwLock};

use super::*;

struct InternalUser {
    pub name: String,
    pub primary_group: GroupIdentifier,
}

struct InternalGroup {
    pub name: String,
    pub users: BTreeSet<UserIdentifier>,
}

struct InternalManager {
    pub users: BTreeMap<UserIdentifier, InternalUser>,
    pub groups: BTreeMap<GroupIdentifier, InternalGroup>,
}

pub struct Manager(RwLock<CriticalSectionRawMutex, InternalManager>);

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
}

impl Manager {
    pub fn new() -> Self {
        let mut groups = BTreeMap::new();
        groups.insert(
            GroupIdentifier::ROOT,
            InternalGroup {
                name: "root".to_string(),
                users: BTreeSet::new(),
            },
        );

        let mut users = BTreeMap::new();
        users.insert(
            UserIdentifier::ROOT,
            InternalUser {
                name: "root".to_string(),
                primary_group: GroupIdentifier::ROOT,
            },
        );

        Self(RwLock::new(InternalManager { users, groups }))
    }

    pub async fn get_new_group_identifier(&self) -> Result<GroupIdentifier> {
        let inner = self.0.read().await;

        let mut identifier = GroupIdentifier::MINIMUM;

        while inner.groups.contains_key(&identifier) {
            identifier += 1;

            if identifier == GroupIdentifier::MAXIMUM {
                return Err(Error::TooManyGroups);
            }
        }

        Ok(identifier)
    }

    pub async fn get_new_user_identifier(&self) -> Result<UserIdentifier> {
        let inner = self.0.read().await;

        let mut identifier = UserIdentifier::MINIMUM;

        while inner.users.contains_key(&identifier) {
            identifier += 1;

            if identifier == UserIdentifier::MAXIMUM {
                return Err(Error::TooManyUsers);
            }
        }

        Ok(identifier)
    }

    pub async fn add_user(
        &self,
        identifier: UserIdentifier,
        name: &str,
        primary_group: GroupIdentifier,
    ) -> Result<()> {
        let mut inner = self.0.write().await;

        // - Check if user identifier is unique
        if inner.users.contains_key(&identifier) {
            return Err(Error::DuplicateUserIdentifier);
        }

        // - Check if user name is unique
        if inner.users.values().any(|user| user.name == name) {
            return Err(Error::DuplicateUserName);
        }

        // - Add user to the users map
        let user = InternalUser {
            name: name.to_string(),
            primary_group,
        };

        if inner.users.insert(identifier, user).is_some() {
            return Err(Error::DuplicateUserIdentifier); // Shouldn't happen
        }

        // Saved groups are loaded before users, so primary membership may
        // already exist. Duplicate user identifiers/names were checked above.
        inner
            .groups
            .get_mut(&primary_group)
            .ok_or(Error::InvalidGroupIdentifier)?
            .users
            .insert(identifier);

        Ok(())
    }

    pub async fn add_group(
        &self,
        identifier: GroupIdentifier,
        name: &str,
        users: &[UserIdentifier],
    ) -> Result<()> {
        let mut inner = self.0.write().await;

        // - Check if group identifier is unique
        if inner.groups.contains_key(&identifier) {
            return Err(Error::DuplicateGroupIdentifier);
        }

        // - Check if group name is unique
        if inner.groups.values().any(|group| group.name == name) {
            return Err(Error::DuplicateGroupName);
        }

        let group = InternalGroup {
            name: name.to_string(),
            users: BTreeSet::from_iter(users.iter().cloned()),
        };

        if inner.groups.insert(identifier, group).is_some() {
            return Err(Error::DuplicateGroupIdentifier); // Shouldn't happen
        }

        Ok(())
    }

    pub fn is_root(identifier: UserIdentifier) -> bool {
        UserIdentifier::ROOT == identifier
    }

    pub async fn is_in_group(
        &self,
        user_identifier: UserIdentifier,
        group_identifier: GroupIdentifier,
    ) -> bool {
        let inner = self.0.read().await;

        if let Some(group) = inner.groups.get(&group_identifier)
            && group.users.contains(&user_identifier)
        {
            return true;
        }

        false
    }

    pub async fn get_user_groups(
        &self,
        identifier: UserIdentifier,
    ) -> Result<BTreeSet<GroupIdentifier>> {
        let inner = self.0.read().await;

        let mut user_groups: BTreeSet<GroupIdentifier> = BTreeSet::new();

        user_groups.extend(
            inner
                .groups
                .iter()
                .filter(|(_, group)| group.users.contains(&identifier))
                .map(|(identifier, _)| *identifier),
        );

        Ok(user_groups)
    }

    pub async fn exists_group(&self, identifier: GroupIdentifier) -> Result<bool> {
        Ok(self.0.read().await.groups.contains_key(&identifier))
    }

    pub async fn exists_user(&self, identifier: UserIdentifier) -> Result<bool> {
        Ok(self.0.read().await.users.contains_key(&identifier))
    }

    pub async fn add_to_group(
        &self,
        user_identifier: UserIdentifier,
        group_identifier: GroupIdentifier,
    ) -> Result<()> {
        if !self.exists_group(group_identifier).await? {
            return Err(Error::InvalidGroupIdentifier);
        }
        let mut inner = self.0.write().await;
        if !inner
            .groups
            .get_mut(&group_identifier)
            .ok_or(Error::InvalidGroupIdentifier)?
            .users
            .insert(user_identifier)
        {
            return Err(Error::DuplicateGroupIdentifier);
        }
        Ok(())
    }

    pub async fn get_group_name(&self, identifier: GroupIdentifier) -> Result<String> {
        Ok(self
            .0
            .read()
            .await
            .groups
            .get(&identifier)
            .ok_or(Error::InvalidGroupIdentifier)?
            .name
            .clone())
    }

    pub async fn get_user_identifier(&self, name: &str) -> Result<UserIdentifier> {
        Ok(*self
            .0
            .read()
            .await
            .users
            .iter()
            .find(|(_, user)| user.name == name)
            .ok_or(Error::InvalidUserIdentifier)?
            .0)
    }

    pub async fn get_group_users(
        &self,
        identifier: GroupIdentifier,
    ) -> Result<Vec<UserIdentifier>> {
        Ok(self
            .0
            .read()
            .await
            .groups
            .get(&identifier)
            .ok_or(Error::InvalidGroupIdentifier)?
            .users
            .clone()
            .into_iter()
            .collect())
    }

    pub async fn get_user_name(&self, identifier: UserIdentifier) -> Result<String> {
        Ok(self
            .0
            .read()
            .await
            .users
            .get(&identifier)
            .ok_or(Error::InvalidUserIdentifier)?
            .name
            .clone())
    }

    pub async fn get_user_primary_group(
        &self,
        identifier: UserIdentifier,
    ) -> Result<GroupIdentifier> {
        Ok(self
            .0
            .read()
            .await
            .users
            .get(&identifier)
            .ok_or(Error::InvalidUserIdentifier)?
            .primary_group)
    }

    pub fn check_credentials(&self, _user_name: &str, _password: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    use task::test;

    #[test]
    async fn create_user() {
        let manager = Manager::new();
        let user_name = "Alice";
        let identifier = UserIdentifier::new(1000);
        manager
            .add_user(identifier, user_name, GroupIdentifier::ROOT)
            .await
            .unwrap();
        assert!(manager.exists_user(identifier).await.unwrap());
    }

    #[test]
    async fn create_user_with_restored_primary_group_membership() {
        let manager = Manager::new();
        let user = UserIdentifier::new(42);
        let group = GroupIdentifier::new(1000);
        manager.add_group(group, "saved", &[user]).await.unwrap();
        assert_eq!(manager.add_user(user, "bob", group).await, Ok(()));
        assert_eq!(manager.get_user_name(user).await.unwrap(), "bob");
        assert_eq!(manager.get_group_users(group).await.unwrap(), [user]);
        assert_eq!(
            manager.add_user(user, "bob", group).await,
            Err(Error::DuplicateUserIdentifier)
        );
    }

    #[test]
    async fn create_user_duplicate() {
        let user_name_1 = "Alice";
        let user_name_2 = "Bob";

        let identifier_1 = UserIdentifier::new(1000);
        let identifier_2 = UserIdentifier::new(1001);

        // Same identifier
        let manager = Manager::new();

        manager
            .add_user(identifier_1, user_name_1, GroupIdentifier::ROOT)
            .await
            .unwrap();

        let result = manager
            .add_user(identifier_1, user_name_2, GroupIdentifier::ROOT)
            .await;
        assert_eq!(result, Err(Error::DuplicateUserIdentifier));

        // Same name
        let manager = Manager::new();

        manager
            .add_user(identifier_1, user_name_1, GroupIdentifier::ROOT)
            .await
            .unwrap();

        let result = manager
            .add_user(identifier_2, user_name_1, GroupIdentifier::ROOT)
            .await;
        assert_eq!(result, Err(Error::DuplicateUserName));

        // Same name and identifier
        let manager = Manager::new();

        manager
            .add_user(identifier_1, user_name_1, GroupIdentifier::ROOT)
            .await
            .unwrap();

        manager
            .add_user(identifier_1, user_name_1, GroupIdentifier::ROOT)
            .await
            .unwrap_err();
    }

    #[test]
    async fn create_group() {
        let manager = Manager::new();
        let group_name = "Developers";
        let group_id = GroupIdentifier::new(1000);
        let result = manager.add_group(group_id, group_name, &[]).await;
        assert!(result.is_ok());
        assert!(manager.exists_group(group_id).await.unwrap());
    }

    #[test]
    async fn create_group_duplicate() {
        let group_name_1 = "Developers";
        let group_name_2 = "Testers";

        let group_id_1 = GroupIdentifier::new(1000);
        let group_id_2 = GroupIdentifier::new(1001);

        // Same identifier
        let manager = Manager::new();

        manager
            .add_group(group_id_1, group_name_1, &[])
            .await
            .unwrap();

        let result = manager.add_group(group_id_1, group_name_2, &[]).await;
        assert_eq!(result, Err(Error::DuplicateGroupIdentifier));

        // Same name
        let manager = Manager::new();

        manager
            .add_group(group_id_1, group_name_1, &[])
            .await
            .unwrap();

        let result = manager.add_group(group_id_2, group_name_1, &[]).await;
        assert_eq!(result, Err(Error::DuplicateGroupName));

        // Same name and identifier
        let manager = Manager::new();

        manager
            .add_group(group_id_1, group_name_1, &[])
            .await
            .unwrap();

        manager
            .add_group(group_id_1, group_name_1, &[])
            .await
            .unwrap_err();
    }

    #[test]
    async fn is_root() {
        let root_id = UserIdentifier::ROOT;
        assert!(Manager::is_root(root_id));
    }

    #[test]
    async fn is_in_group() {
        let manager = Manager::new();
        let user_name = "Bob";
        let identifier = UserIdentifier::new(1000);
        manager
            .add_user(identifier, user_name, GroupIdentifier::ROOT)
            .await
            .unwrap();
        let group_name = "Admins";
        let group_id = GroupIdentifier::new(1000);

        manager.add_group(group_id, group_name, &[]).await.unwrap();
        manager.add_to_group(identifier, group_id).await.unwrap();
        assert!(manager.is_in_group(identifier, group_id).await);
    }

    #[test]
    async fn get_user_groups() {
        let manager = Manager::new();

        let user_name = "Charlie";
        let identifier = UserIdentifier::new(1000);
        manager
            .add_user(identifier, user_name, GroupIdentifier::ROOT)
            .await
            .unwrap();
        let group_name1 = "TeamA";
        let group_id1 = GroupIdentifier::new(1000);

        manager
            .add_group(group_id1, group_name1, &[])
            .await
            .unwrap();
        let group_name2 = "TeamB";
        let group_id2 = GroupIdentifier::new(1001);

        manager
            .add_group(group_id2, group_name2, &[])
            .await
            .unwrap();
        manager.add_to_group(identifier, group_id1).await.unwrap();
        manager.add_to_group(identifier, group_id2).await.unwrap();
        let groups = manager.get_user_groups(identifier).await.unwrap();

        assert_eq!(groups.len(), 3);
        assert!(
            groups.contains(&group_id1)
                && groups.contains(&group_id2)
                && groups.contains(&GroupIdentifier::ROOT)
        );
    }

    #[test]
    async fn get_group_name() {
        let manager = Manager::new();
        let group_name = "QA";
        let group_id = GroupIdentifier::new(1000);
        manager.add_group(group_id, group_name, &[]).await.unwrap();
        let retrieved_name = manager.get_group_name(group_id).await.unwrap();
        assert_eq!(group_name, retrieved_name);
    }

    #[test]
    async fn get_group_users() {
        let manager = Manager::new();
        let user_name = "Dave";
        let identifier = UserIdentifier::new(1000);
        manager
            .add_user(identifier, user_name, GroupIdentifier::ROOT)
            .await
            .unwrap();
        let group_name = "Engineers";
        let group_id = GroupIdentifier::new(1000);
        manager.add_group(group_id, group_name, &[]).await.unwrap();
        manager.add_to_group(identifier, group_id).await.unwrap();
        let users = manager.get_group_users(group_id).await.unwrap();
        assert_eq!(users.len(), 1);
        assert!(users.contains(&identifier));
    }

    #[test]
    async fn get_user_name() {
        let manager = Manager::new();
        let user_name = "Eve";
        let identifier = UserIdentifier::new(1000);
        manager
            .add_user(identifier, user_name, GroupIdentifier::ROOT)
            .await
            .unwrap();
        let retrieved_name = manager.get_user_name(identifier).await.unwrap();
        assert_eq!(user_name, retrieved_name);
    }

    #[test]
    async fn check_credentials() {
        let manager = Manager::new();
        let user_name = "Frank";
        let password = "password123";
        assert!(manager.check_credentials(user_name, password));
    }
}
