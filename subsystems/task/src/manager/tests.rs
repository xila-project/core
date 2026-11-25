mod allocation;

// Tests module - contains all Manager tests
extern crate std;

use super::*;
use crate::test;
use alloc::{collections::BTreeMap, format, vec, vec::Vec};
use core::time::Duration;
use users::{GroupIdentifier, UserIdentifier};

fn initialize() -> &'static Manager {
    crate::manager::test_manager()
}

fn get_instance() -> &'static Manager {
    initialize()
}

#[test(task_path = crate)]
async fn test_get_task_name() {
    let manager = initialize();

    let task_name = "Test Task";
    let task = manager.get_current_task_identifier().await;

    let spawner = manager.get_spawner(task).await.unwrap();

    let _ = manager
        .spawn(task, task_name, Some(spawner), async move |task| {
            assert_eq!(get_instance().get_name(task).await.unwrap(), task_name);
        })
        .await
        .unwrap()
        .0
        .join()
        .await;
}

#[test(task_path = crate)]
async fn test_set_get_owner() {
    let manager = initialize();

    let task = manager.get_current_task_identifier().await;

    // Set user and group to root
    manager.set_user(task, UserIdentifier::ROOT).await.unwrap();
    manager
        .set_group(task, GroupIdentifier::ROOT)
        .await
        .unwrap();

    assert_eq!(
        get_instance().get_user(task).await.unwrap(),
        UserIdentifier::ROOT
    );
    assert_eq!(
        get_instance().get_group(task).await.unwrap(),
        GroupIdentifier::ROOT
    );
}

#[test(task_path = crate)]
async fn test_get_current_task_identifier() {
    let manager = initialize();

    let task = manager.get_current_task_identifier().await;

    let spawner = manager.get_spawner(task).await.unwrap();

    manager
        .spawn(task, "Current Task", Some(spawner), async move |task| {
            assert_eq!(get_instance().get_current_task_identifier().await, task);
        })
        .await
        .unwrap()
        .0
        .join()
        .await;
}

#[test(task_path = crate)]
async fn test_task_owner_inheritance() {
    let manager = initialize();

    let task = manager.get_current_task_identifier().await;
    let user_identifier = UserIdentifier::new(123);
    let group_identifier = GroupIdentifier::new(456);

    manager.set_user(task, user_identifier).await.unwrap();
    manager.set_group(task, group_identifier).await.unwrap();

    // Get the spawner of the current task
    let spawner = manager.get_spawner(task).await.unwrap();

    // Spawn first task that verifies inheritance
    manager
        .spawn(task, "Task 1", Some(spawner), async move |task_1| {
            assert_eq!(
                get_instance().get_user(task_1).await.unwrap(),
                user_identifier
            );
            assert_eq!(
                get_instance().get_group(task_1).await.unwrap(),
                group_identifier
            );

            // Get the spawner of Task_1 to inherit to Task_2
            let task_1_spawner = get_instance().get_spawner(task_1).await.unwrap();

            // Spawn second task as a child of the first task
            let _ = manager
                .spawn(
                    task_1,
                    "Task 2",
                    Some(task_1_spawner),
                    async move |task_2| {
                        // Verify that the child task inherits the user and group
                        assert_eq!(
                            get_instance().get_user(task_2).await.unwrap(),
                            user_identifier
                        );
                        assert_eq!(
                            get_instance().get_group(task_2).await.unwrap(),
                            group_identifier
                        );

                        // This task has no nested calls to Spawn
                    },
                )
                .await
                .unwrap()
                .0
                .join()
                .await;
        })
        .await
        .unwrap()
        .0
        .join()
        .await;
}

#[test(task_path = crate)]
async fn test_environment_variables() {
    let manager = initialize();

    let task_identifier = manager.get_current_task_identifier().await;
    let name = "Key";
    let value = "Value";

    manager
        .set_environment_variable(task_identifier, name, value)
        .await
        .unwrap();
    assert_eq!(
        manager
            .get_environment_variable(task_identifier, name)
            .await
            .unwrap()
            .get_value(),
        value
    );
    manager
        .remove_environment_variable(task_identifier, name)
        .await
        .unwrap();
    assert!(
        manager
            .get_environment_variable(task_identifier, name)
            .await
            .is_err()
    );
}

#[test(task_path = crate)]
async fn test_environment_replacement() {
    let manager = initialize();

    let task = manager.get_current_task_identifier().await;
    manager
        .set_environment_variable(task, "Old", "1")
        .await
        .unwrap();
    let spawner = manager.get_spawner(task).await.unwrap();
    manager
        .spawn(task, "Replaced", Some(spawner), async move |child| {
            let manager = get_instance();
            // The child starts with a copy of its parent's environment.
            assert_eq!(
                manager
                    .get_environment_variable(child, "Old")
                    .await
                    .unwrap()
                    .get_value(),
                "1"
            );
            // An explicit list replaces it. Later entries win and the parent is unaffected.
            manager
                .replace_environment_variables(
                    child,
                    vec![
                        EnvironmentVariable::new("A", "1"),
                        EnvironmentVariable::new("B", "2"),
                        EnvironmentVariable::new("A", "3"),
                    ],
                )
                .await
                .unwrap();
            assert!(
                manager
                    .get_environment_variable(child, "Old")
                    .await
                    .is_err()
            );
            assert_eq!(
                manager
                    .get_environment_variable(child, "A")
                    .await
                    .unwrap()
                    .get_value(),
                "3"
            );
            assert_eq!(
                manager
                    .get_environment_variables(child)
                    .await
                    .unwrap()
                    .len(),
                2
            );
            manager
                .replace_environment_variables(child, Vec::new())
                .await
                .unwrap();
            assert!(
                manager
                    .get_environment_variables(child)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(
                manager
                    .replace_environment_variables(
                        TaskIdentifier::new(TaskIdentifier::MAXIMUM),
                        Vec::new()
                    )
                    .await
                    .is_err()
            );
        })
        .await
        .unwrap()
        .0
        .join()
        .await;
    assert_eq!(
        manager
            .get_environment_variable(task, "Old")
            .await
            .unwrap()
            .get_value(),
        "1"
    );
    manager
        .remove_environment_variable(task, "Old")
        .await
        .unwrap();
}

#[test(task_path = crate)]
async fn test_system_environment_variable() {
    let manager = initialize();
    let root = manager.get_current_task_identifier().await;
    assert_eq!(root, Manager::ROOT_TASK_IDENTIFIER);
    let value = async |task| {
        manager
            .get_environment_variable(task, "System")
            .await
            .ok()
            .map(|variable| alloc::string::String::from(variable.get_value()))
    };

    // Tasks without the variable follow the root task, which has none.
    let without = manager.register(root, "Without").await.unwrap();
    manager.set_system_environment_variable("System", "1").await;
    assert_eq!(value(root).await.as_deref(), Some("1"));
    assert_eq!(value(without).await.as_deref(), Some("1"));

    // A new task inherits the value; a task with its own value keeps it.
    let follower = manager.register(without, "Follower").await.unwrap();
    let own = manager.register(root, "Own").await.unwrap();
    manager
        .set_environment_variable(own, "System", "mine")
        .await
        .unwrap();
    let later = manager.register(root, "Later").await.unwrap();
    manager
        .remove_environment_variable(later, "System")
        .await
        .unwrap();
    manager.set_system_environment_variable("System", "2").await;
    assert_eq!(value(root).await.as_deref(), Some("2"));
    assert_eq!(value(without).await.as_deref(), Some("2"));
    assert_eq!(value(follower).await.as_deref(), Some("2"));
    assert_eq!(value(own).await.as_deref(), Some("mine"));
    // Once the root task has a value, a task without one is not updated.
    assert_eq!(value(later).await, None);
    // Other variables are unaffected.
    assert_eq!(
        manager
            .get_environment_variables(follower)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[test(task_path = crate)]
async fn test_environment_variable_inheritance() {
    let manager = initialize();

    let task = manager.get_current_task_identifier().await;

    get_instance()
        .set_environment_variable(task, "Key", "Value")
        .await
        .unwrap();

    // Get the spawner of the current task
    let spawner = manager.get_spawner(task).await.unwrap();

    // Then spawn the grandchild task with the returned task ID
    manager
        .spawn(task, "Grand child Task", Some(spawner), async move |task| {
            assert_eq!(
                get_instance()
                    .get_environment_variable(task, "Key")
                    .await
                    .unwrap()
                    .get_value(),
                "Value"
            );
        })
        .await
        .unwrap()
        .0
        .join()
        .await;
}

#[test(task_path = crate)]
async fn test_join_handle() {
    let manager = initialize();

    let task = manager.get_current_task_identifier().await;

    let spawner = manager.get_spawner(task).await.unwrap();
    let join_handle = manager
        .spawn(task, "Task with join handle", Some(spawner), async |_| 42)
        .await
        .unwrap();
    assert_eq!(join_handle.0.join().await, 42);
}

#[test(task_path = crate)]
async fn test_set_user() {
    let manager = initialize();

    let task = manager.get_current_task_identifier().await;

    let user = UserIdentifier::new(123); // Assuming UserIdentifier is i32 for example

    manager.set_user(task, user).await.unwrap();

    assert_eq!(manager.get_user(task).await.unwrap(), user);
}

#[test(task_path = crate)]
async fn test_set_group() {
    let manager = initialize();

    let task = manager.get_current_task_identifier().await;

    let group = GroupIdentifier::new(456); // Assuming GroupIdentifier is i32 for example

    manager.set_group(task, group).await.unwrap();

    assert_eq!(manager.get_group(task).await.unwrap(), group);
}

#[test(task_path = crate)]
async fn test_signal() {
    let manager = initialize();

    let task = manager.get_current_task_identifier().await;

    let spawner = manager.get_spawner(task).await.unwrap();

    let (child_handle, child_identifier) = manager
        .spawn(task, "Task with signal", Some(spawner), async |task| {
            Manager::sleep(Duration::from_millis(10)).await; // Allow the parent task to set signals

            assert_eq!(
                get_instance().peek_signal(task).await.unwrap(),
                Some(Signal::Hangup)
            );

            assert_eq!(
                get_instance().pop_signal(task).await.unwrap(),
                Some(Signal::Hangup)
            );

            assert_eq!(
                get_instance().peek_signal(task).await.unwrap(),
                Some(Signal::Kill)
            );

            assert_eq!(
                get_instance().pop_signal(task).await.unwrap(),
                Some(Signal::Kill)
            );
        })
        .await
        .unwrap();

    get_instance()
        .send_signal(child_identifier, Signal::Kill)
        .await
        .unwrap();

    get_instance()
        .send_signal(child_identifier, Signal::Hangup)
        .await
        .unwrap();

    // Wait for the task to finish
    child_handle.join().await;
}

#[test(task_path = crate)]
async fn test_find_first_available_identifier_empty_map() {
    let map: BTreeMap<u32, ()> = BTreeMap::new();
    let range = 0u32..10u32;

    let result = Manager::find_first_available_identifier(&map, range);
    assert_eq!(result, Some(0u32));
}

#[test(task_path = crate)]
async fn test_find_first_available_identifier_no_gaps() {
    let mut map: BTreeMap<u32, ()> = BTreeMap::new();
    map.insert(0, ());
    map.insert(1, ());
    map.insert(2, ());

    let range = 0u32..10u32;
    let result = Manager::find_first_available_identifier(&map, range);
    assert_eq!(result, Some(3u32));
}

#[test(task_path = crate)]
async fn test_find_first_available_identifier_with_gap() {
    let mut map: BTreeMap<u32, ()> = BTreeMap::new();
    map.insert(0, ());
    map.insert(2, ());
    map.insert(3, ());

    let range = 0u32..10u32;
    let result = Manager::find_first_available_identifier(&map, range);
    assert_eq!(result, Some(1u32));
}

#[test(task_path = crate)]
async fn test_find_first_available_identifier_range_exhausted() {
    let mut map: BTreeMap<u32, ()> = BTreeMap::new();
    map.insert(0, ());
    map.insert(1, ());

    let range = 0u32..2u32;
    let result = Manager::find_first_available_identifier(&map, range);
    assert_eq!(result, None);
}

#[test(task_path = crate)]
async fn test_find_first_available_identifier_single_element() {
    let mut map: BTreeMap<u32, ()> = BTreeMap::new();
    map.insert(1, ());

    let range = 0u32..5u32;
    let result = Manager::find_first_available_identifier(&map, range);
    assert_eq!(result, Some(0u32));
}

#[test(task_path = crate)]
async fn test_find_first_available_identifier_large_gap() {
    let mut map: BTreeMap<u32, ()> = BTreeMap::new();
    map.insert(0, ());
    map.insert(100, ());

    let range = 0u32..200u32;
    let result = Manager::find_first_available_identifier(&map, range);
    assert_eq!(result, Some(1u32));
}

#[test(task_path = crate)]
async fn test_find_first_available_identifier_step_range() {
    let mut map: BTreeMap<usize, ()> = BTreeMap::new();
    map.insert(0, ());
    map.insert(2, ());

    let range = (0..10).step_by(2);
    let result = Manager::find_first_available_identifier(&map, range);
    assert_eq!(result, Some(4));
}

#[test(task_path = crate)]
async fn test_find_first_available_identifier_unordered_keys() {
    let mut map: BTreeMap<u32, ()> = BTreeMap::new();
    map.insert(5, ());
    map.insert(1, ());
    map.insert(3, ());

    let range = 0u32..10u32;
    let result = Manager::find_first_available_identifier(&map, range);
    // BTreeMap orders keys, so iteration will be 1, 3, 5
    // Range starts at 0, so first mismatch is at position 0
    assert_eq!(result, Some(0u32));
}

#[test(task_path = crate)]
async fn test_find_first_available_identifier_exact_match_sequence() {
    let mut map: BTreeMap<u32, ()> = BTreeMap::new();
    for i in 5..10 {
        map.insert(i, ());
    }

    let range = 5u32..15u32;
    let result = Manager::find_first_available_identifier(&map, range);
    assert_eq!(result, Some(10u32));
}

#[test(task_path = crate)]
async fn test_find_first_available_identifier_empty_range() {
    let mut map: BTreeMap<u32, ()> = BTreeMap::new();
    map.insert(0, ());

    let range = 5u32..5u32; // Empty range
    let result = Manager::find_first_available_identifier(&map, range);
    assert_eq!(result, None);
}

#[test(task_path = crate)]
async fn test_spawn() {
    let manager = initialize();

    let task_name = "Child Task";
    let task = manager.get_current_task_identifier().await;

    let spawner = manager.get_spawner(task).await.unwrap();

    let _ = manager
        .spawn(task, task_name, Some(spawner), async |_| {})
        .await
        .unwrap()
        .0
        .join()
        .await;
}

#[test(task_path = crate)]
async fn test_get_parent() {
    let manager = initialize();

    let root_task = manager.get_current_task_identifier().await;
    let spawner = manager.get_spawner(root_task).await.unwrap();

    let (child_handle, _child_task) = manager
        .spawn(
            root_task,
            "Child Task",
            Some(spawner),
            async move |child_task| {
                // Test that child task's parent is the root task
                assert_eq!(
                    get_instance().get_parent(child_task).await.unwrap(),
                    root_task
                );
            },
        )
        .await
        .unwrap();

    child_handle.join().await;
}

#[test(task_path = crate)]
async fn test_get_children() {
    let manager = initialize();

    let root_task = manager.get_current_task_identifier().await;
    let spawner = manager.get_spawner(root_task).await.unwrap();

    // Spawn first child
    let (child1_handle, child1_task) = manager
        .spawn(root_task, "Child Task 1", Some(spawner), async move |_| {
            Manager::sleep(core::time::Duration::from_millis(50)).await;
        })
        .await
        .unwrap();

    // Spawn second child
    let (child2_handle, child2_task) = manager
        .spawn(root_task, "Child Task 2", Some(spawner), async move |_| {
            Manager::sleep(core::time::Duration::from_millis(50)).await;
        })
        .await
        .unwrap();

    // Check that root task contains both children
    let children = manager.get_children(root_task).await.unwrap();
    assert!(children.contains(&child1_task));
    assert!(children.contains(&child2_task));

    // Wait for children to complete
    child1_handle.join().await;
    child2_handle.join().await;

    // After children complete, they should no longer be in the children list
    let final_children = manager.get_children(root_task).await.unwrap();
    assert!(!final_children.contains(&child1_task));
    assert!(!final_children.contains(&child2_task));
}

#[ignore]
#[test(task_path = crate)]
async fn test_get_children_with_nested_tasks() {
    let manager = initialize();

    let root_task = manager.get_current_task_identifier().await;
    let spawner = manager.get_spawner(root_task).await.unwrap();

    let (parent_handle, _parent_task) = manager
        .spawn(
            root_task,
            "Parent Task",
            Some(spawner),
            async move |parent_task| {
                let parent_spawner = get_instance().get_spawner(parent_task).await.unwrap();

                // Parent task should initially have no children
                let initial_children = get_instance().get_children(parent_task).await.unwrap();
                assert_eq!(initial_children.len(), 0);

                // Spawn child from parent task
                let (child_handle, child_task) = get_instance()
                    .spawn(
                        parent_task,
                        "Nested Child",
                        Some(parent_spawner),
                        async move |child_task| {
                            // Verify parent-child relationship
                            assert_eq!(
                                get_instance().get_parent(child_task).await.unwrap(),
                                parent_task
                            );
                        },
                    )
                    .await
                    .unwrap();

                yield_now().await; // Yield to allow child task to start

                // Parent should now have one child
                let children = get_instance().get_children(parent_task).await.unwrap();
                assert_eq!(children.len(), 1);
                assert!(children.contains(&child_task));

                child_handle.join().await;
            },
        )
        .await
        .unwrap();

    parent_handle.join().await;
}

#[test(task_path = crate)]
async fn test_get_parent_invalid_task() {
    let manager = initialize();

    // Test with an invalid task identifier
    let invalid_task = TaskIdentifier::new(TaskIdentifier::MAXIMUM);
    let result = manager.get_parent(invalid_task).await;

    assert!(result.is_err());
}

#[test(task_path = crate)]
async fn test_get_children_invalid_task() {
    let manager = initialize();

    // Test with an invalid task identifier
    let invalid_task = TaskIdentifier::new(TaskIdentifier::MAXIMUM);
    let result = manager.get_children(invalid_task).await;

    // get_children should return an empty vector for invalid task
    // since it filters by parent, not checking if the task exists
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);
}

#[test(task_path = crate)]
async fn test_root_task_parent() {
    let manager = initialize();

    let root_task = manager.get_current_task_identifier().await;

    // Root task should be its own parent
    let parent = manager.get_parent(root_task).await.unwrap();
    assert_eq!(parent, Manager::ROOT_TASK_IDENTIFIER);
}

#[test(task_path = crate)]
async fn test_multiple_generation_relationships() {
    let manager = initialize();

    let root_task = manager.get_current_task_identifier().await;
    let spawner = manager.get_spawner(root_task).await.unwrap();

    let (level1_handle, level1_task) = manager
        .spawn(
            root_task,
            "Level 1 Task",
            Some(spawner),
            async move |level1_task| {
                // Verify Level 1 parent is root
                assert_eq!(
                    get_instance().get_parent(level1_task).await.unwrap(),
                    root_task
                );

                let level1_spawner = get_instance().get_spawner(level1_task).await.unwrap();

                let (level2_handle, level2_task) = get_instance()
                    .spawn(
                        level1_task,
                        "Level 2 Task",
                        Some(level1_spawner),
                        async move |level2_task| {
                            // Verify Level 2 parent is Level 1
                            assert_eq!(
                                get_instance().get_parent(level2_task).await.unwrap(),
                                level1_task
                            );

                            let level2_spawner =
                                get_instance().get_spawner(level2_task).await.unwrap();

                            let (level3_handle, level3_task) = get_instance()
                                .spawn(
                                    level2_task,
                                    "Level 3 Task",
                                    Some(level2_spawner),
                                    async move |level3_task| {
                                        // Verify Level 3 parent is Level 2
                                        assert_eq!(
                                            get_instance().get_parent(level3_task).await.unwrap(),
                                            level2_task
                                        );
                                        crate::sleep(Duration::from_millis(50)).await;
                                    },
                                )
                                .await
                                .unwrap();

                            // Level 2 should have Level 3 as child
                            let level2_children =
                                get_instance().get_children(level2_task).await.unwrap();
                            assert!(level2_children.contains(&level3_task));

                            level3_handle.join().await;
                        },
                    )
                    .await
                    .unwrap();

                // Level 1 should have Level 2 as child
                let level1_children = get_instance().get_children(level1_task).await.unwrap();
                assert!(level1_children.contains(&level2_task));

                level2_handle.join().await;
            },
        )
        .await
        .unwrap();

    // Root should have Level 1 as child
    let root_children = manager.get_children(root_task).await.unwrap();
    assert!(root_children.contains(&level1_task));

    level1_handle.join().await;
}

#[test(task_path = crate)]
async fn test_register_spawner() {
    let manager = initialize();

    // Get current task and its spawner
    let current_task = manager.get_current_task_identifier().await;
    let current_spawner_id = manager.get_spawner(current_task).await.unwrap();

    // We can't easily create new spawners in tests due to lifetime requirements,
    // but we can test the registration logic by verifying the current spawner exists
    // and testing error conditions

    // Verify the current spawner ID is valid
    assert!(current_spawner_id != usize::MAX);

    // Test that we can unregister and re-register spawners
    // Note: We can't actually unregister the current spawner as it would break the test
    // So we test the error path instead
    let invalid_spawner_id = 99999;
    let result = manager.unregister_spawner(invalid_spawner_id);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), Error::NoSpawnerAvailable));
}

#[test(task_path = crate)]
async fn test_unregister_spawner() {
    let manager = initialize();

    // Test unregistering a non-existent spawner
    let invalid_spawner_id = 99999;
    let result = manager.unregister_spawner(invalid_spawner_id);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), Error::NoSpawnerAvailable));

    // Test unregistering the same spawner twice
    let second_result = manager.unregister_spawner(invalid_spawner_id);
    assert!(second_result.is_err());
    assert!(matches!(
        second_result.unwrap_err(),
        Error::NoSpawnerAvailable
    ));
}

#[test(task_path = crate)]
async fn test_unregister_nonexistent_spawner() {
    let manager = initialize();

    // Try to unregister a nonexistent spawner
    let invalid_id = 99999;
    let result = manager.unregister_spawner(invalid_id);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), Error::NoSpawnerAvailable));
}

#[test(task_path = crate)]
async fn test_register_multiple_spawners() {
    let manager = initialize();

    // Test the ID assignment logic by checking the current spawner
    let current_task = manager.get_current_task_identifier().await;
    let current_spawner_id = manager.get_spawner(current_task).await.unwrap();

    // The spawner ID should be valid
    assert!(current_spawner_id != usize::MAX);

    // Test that invalid spawner IDs are handled correctly
    let invalid_ids = [99999, usize::MAX, 12345];
    for invalid_id in invalid_ids {
        let result = manager.unregister_spawner(invalid_id);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::NoSpawnerAvailable));
    }
}

//#[Test(task_path = crate)]
async fn _test_spawner_load_balancing() {
    let manager = initialize();

    // Test the load balancing behavior by checking how tasks are distributed
    // Since we can't easily create multiple spawners in tests, we test that
    // the Select_best_spawner function works with the available spawners

    let parent_task = manager.get_current_task_identifier().await;
    let current_spawner = manager.get_spawner(parent_task).await.unwrap();

    // Spawn multiple tasks and verify they all get valid spawner assignments
    let mut handles = Vec::new();

    for i in 0..4 {
        let task_name = format!("Load Balance Task {i}");
        let (handle, _) = manager
            .spawn(parent_task, &task_name, None, async move |task| {
                Manager::sleep(core::time::Duration::from_millis(10)).await;
                get_instance().get_spawner(task).await.unwrap()
            })
            .await
            .unwrap();

        handles.push(handle);
    }

    // Wait for all tasks to complete and collect their spawner IDs
    let mut used_spawners = Vec::new();
    for handle in handles {
        let spawner_used = handle.join().await;
        used_spawners.push(spawner_used);
    }

    // Verify that all tasks got valid spawner assignments
    for spawner_id in used_spawners {
        assert!(
            spawner_id != usize::MAX,
            "All tasks should get valid spawner IDs"
        );
        // Verify they're using a reasonable spawner (could be the current one or another valid one)
        assert!(
            spawner_id == current_spawner || spawner_id < 1000,
            "Spawner ID should be reasonable"
        );
    }
}

#[test(task_path = crate)]
async fn test_get_spawner_invalid_task() {
    let manager = initialize();

    // Test with an invalid task identifier
    let invalid_task = TaskIdentifier::new(TaskIdentifier::MAXIMUM);
    let result = manager.get_spawner(invalid_task).await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), Error::InvalidTaskIdentifier));
}

#[test(task_path = crate)]
async fn test_get_spawner_valid_task() {
    let manager = initialize();

    let task = manager.get_current_task_identifier().await;
    let spawner_result = manager.get_spawner(task).await;

    // Root task should have a spawner
    assert!(spawner_result.is_ok());
}

#[test(task_path = crate)]
async fn test_spawner_with_explicit_selection() {
    let manager = initialize();

    let parent_task = manager.get_current_task_identifier().await;
    let current_spawner_id = manager.get_spawner(parent_task).await.unwrap();

    // Spawn task with explicitly selected spawner
    let (handle, _) = manager
        .spawn(
            parent_task,
            "Explicit Spawner Task",
            Some(current_spawner_id),
            async move |task| get_instance().get_spawner(task).await.unwrap(),
        )
        .await
        .unwrap();

    let used_spawner = handle.join().await;
    assert_eq!(used_spawner, current_spawner_id);
}

#[test(task_path = crate)]
async fn test_spawner_with_invalid_selection() {
    let manager = initialize();

    let parent_task = manager.get_current_task_identifier().await;
    let invalid_spawner = 99999;

    // Try to spawn task with invalid spawner ID
    let result = manager
        .spawn(
            parent_task,
            "Invalid Spawner Task",
            Some(invalid_spawner),
            async |_| {},
        )
        .await;

    assert!(result.is_err());
    if let Err(error) = result {
        assert!(matches!(error, Error::InvalidSpawnerIdentifier));
    }
}

#[test(task_path = crate)]
async fn test_spawner_reuse_after_unregister() {
    let manager = initialize();

    // Test the ID reuse behavior by testing with invalid spawner IDs
    // Since we can't easily create new spawners in tests, we verify the
    // error handling behavior of the spawner management system

    // Test unregistering non-existent spawners
    let invalid_spawner_id = 99999;
    let result1 = manager.unregister_spawner(invalid_spawner_id);
    assert!(result1.is_err());
    assert!(matches!(result1.unwrap_err(), Error::NoSpawnerAvailable));

    // Test that the same invalid ID consistently fails
    let result2 = manager.unregister_spawner(invalid_spawner_id);
    assert!(result2.is_err());
    assert!(matches!(result2.unwrap_err(), Error::NoSpawnerAvailable));

    // Verify that valid spawner operations still work
    let current_task = manager.get_current_task_identifier().await;
    let current_spawner = manager.get_spawner(current_task).await.unwrap();
    assert!(current_spawner != usize::MAX);
}

#[test(task_path = crate)]
async fn authority_tracks_user_and_rejects_reused_task_identifier() {
    let manager = Manager::new();
    let root = manager.get_current_task_identifier().await;
    let child = manager.register(root, "authority").await.unwrap();
    let authority = manager.authority(child).await.unwrap();
    assert_eq!(authority.user().await.unwrap(), UserIdentifier::ROOT);
    let user = UserIdentifier::new(123);
    manager.set_user(child, user).await.unwrap();
    assert_eq!(authority.user().await.unwrap(), user);
    manager.unregister(child).await.unwrap();
    assert!(authority.user().await.is_err());
    let replacement = manager.register(root, "replacement").await.unwrap();
    assert_eq!(replacement, child);
    assert_eq!(
        manager.get_user(replacement).await.unwrap(),
        UserIdentifier::ROOT
    );
    assert!(authority.user().await.is_err());
    assert_eq!(
        manager
            .authority(replacement)
            .await
            .unwrap()
            .user()
            .await
            .unwrap(),
        UserIdentifier::ROOT
    );
}

#[test(task_path = crate)]
async fn granted_identity_is_inherited_and_never_cleared() {
    let manager = Manager::new();
    let root = manager.get_current_task_identifier().await;
    let launcher = manager.register(root, "launcher").await.unwrap();
    let launched = manager.register(launcher, "launched").await.unwrap();
    let user = UserIdentifier::new(123);
    let group = GroupIdentifier::new(456);

    assert!(!manager.is_identity_granted(launched).await.unwrap());
    manager.grant_identity(launched, user, group).await.unwrap();
    assert_eq!(manager.get_user(launched).await.unwrap(), user);
    assert_eq!(manager.get_group(launched).await.unwrap(), group);
    assert!(manager.is_identity_granted(launched).await.unwrap());
    // Only the launched task: its parent and the root are not marked.
    assert!(!manager.is_identity_granted(launcher).await.unwrap());
    assert!(!manager.is_identity_granted(root).await.unwrap());

    // Every task it spawns afterwards inherits the mark, and keeps it once
    // its ancestors have exited and it has been reparented to the root task.
    let child = manager.register(launched, "child").await.unwrap();
    manager.unregister(launched).await.unwrap();
    let grandchild = manager.register(child, "grandchild").await.unwrap();
    manager.unregister(child).await.unwrap();
    assert_eq!(manager.get_parent(grandchild).await.unwrap(), root);
    assert!(manager.is_identity_granted(grandchild).await.unwrap());

    // Changing the identity again does not clear it.
    manager
        .set_user(grandchild, UserIdentifier::ROOT)
        .await
        .unwrap();
    manager
        .set_group(grandchild, GroupIdentifier::ROOT)
        .await
        .unwrap();
    assert!(manager.is_identity_granted(grandchild).await.unwrap());

    // A task spawned by an unmarked task is not marked, even when it reuses
    // the numeric identifier of a marked task that exited.
    let sibling = manager.register(launcher, "sibling").await.unwrap();
    assert!([launched, child].contains(&sibling));
    assert!(!manager.is_identity_granted(sibling).await.unwrap());
}

#[test(task_path = crate)]
async fn task_count_tracks_registration_and_completion() {
    let manager = initialize();
    let parent = manager.get_current_task_identifier().await;
    let before = manager.get_task_count().await;
    let release = synchronization::Arc::new(synchronization::signal::Signal::<
        synchronization::blocking_mutex::raw::CriticalSectionRawMutex,
        (),
    >::new());
    let child_release = release.clone();
    let (child, _) = manager
        .spawn(parent, "counted", None, move |_| async move {
            child_release.wait().await
        })
        .await
        .unwrap();
    assert_eq!(manager.get_task_count().await, before + 1);
    release.signal(());
    child.join().await;
    // Join completion is signalled before task unregistration finishes.
    for _ in 0..100 {
        if manager.get_task_count().await == before {
            return;
        }
        crate::sleep(Duration::from_millis(1)).await;
    }
    assert_eq!(manager.get_task_count().await, before);
}

#[test(task_path = crate)]
async fn utf8_names_and_failed_spawn_are_transactional() {
    let manager = initialize();
    let root = manager.get_current_task_identifier().await;
    let before = manager.get_task_count().await;
    assert!(
        manager
            .spawn(root, "invalid", Some(usize::MAX), async |_| {})
            .await
            .is_err()
    );
    assert_eq!(manager.get_task_count().await, before);
    let name = "abcdefghijklmnopqrstuvé終";
    let child = manager.register(root, name).await.unwrap();
    assert_eq!(manager.get_name(child).await.unwrap(), name);
    manager.unregister(child).await.unwrap();
}

#[std::prelude::v1::test]
fn identity_outside_embassy_is_fallible() {
    assert_eq!(
        crate::block_on(Manager::get_current_internal_identifier()),
        Err(Error::ThreadNotRegistered)
    );
    assert_eq!(
        crate::block_on(Manager::new().try_get_current_task_identifier()),
        Err(Error::ThreadNotRegistered)
    );
    fn assert_traits<T: Send + Sync>() {}
    assert_traits::<Manager>();
    assert_traits::<JoinHandle<core::cell::Cell<u32>>>();
}

#[test(task_path = crate)]
async fn unknown_embassy_task_cannot_replace_root() {
    let manager = initialize();
    let root = manager.get_current_task_identifier().await;
    let authority = manager.authority(root).await.unwrap();
    manager
        .set_environment_variable(root, "root", "preserved")
        .await
        .unwrap();
    let (done, signal) = JoinHandle::new();
    let pool = alloc::boxed::Box::leak(alloc::boxed::Box::new(embassy_executor::raw::TaskPool::<
        _,
        1,
    >::new()));
    let token = pool
        .spawn(move || async move {
            assert_eq!(
                manager.try_get_current_task_identifier().await,
                Err(Error::ThreadNotRegistered)
            );
            signal.signal(());
        })
        .unwrap();
    // This test's waker has already been checked by the manager.
    unsafe { embassy_executor::Spawner::for_current_executor().await }.spawn(token);
    done.join().await;
    assert_eq!(authority.user().await.unwrap(), UserIdentifier::ROOT);
    assert_eq!(
        manager
            .get_environment_variable(root, "root")
            .await
            .unwrap()
            .get_value(),
        "preserved"
    );
    assert_eq!(manager.get_current_task_identifier().await, root);
}

#[test(task_path = crate)]
async fn storage_is_reused_and_future_captures_are_dropped() {
    // A private manager: tasks of concurrent tests on the shared manager
    // would occupy its slots and force new ones to be allocated.
    let manager: &'static Manager = alloc::boxed::Box::leak(alloc::boxed::Box::new(Manager::new()));
    let local = unsafe { embassy_executor::Spawner::for_current_executor().await };
    manager.register_spawner(local).unwrap();
    let root = manager.get_current_task_identifier().await;
    let capture = synchronization::Arc::new(());
    // Warm the slot vector and BTreeMap nodes before counting steady state.
    manager
        .spawn(root, "warm", None, async |_| {})
        .await
        .unwrap()
        .0
        .join()
        .await;
    let before = manager.0.read().await.slots.len();
    allocation::start();
    for _ in 0..1000 {
        let capture = capture.clone();
        manager
            .spawn(root, "reused", None, move |id| async move {
                assert_eq!(manager.get_current_task_identifier().await, id);
                drop(capture);
            })
            .await
            .unwrap()
            .0
            .join()
            .await;
    }
    let (allocations, retained) = allocation::finish();
    #[cfg(not(all(feature = "profiling", target_os = "linux")))]
    assert_eq!(retained, 0, "completed tasks retained heap bytes");
    // Profiling keeps a bounded history of finished tasks, not one per spawn.
    #[cfg(all(feature = "profiling", target_os = "linux"))]
    assert!(
        retained < 32 * 1024,
        "profiling retained {retained} bytes for 1000 completed tasks"
    );
    assert_eq!(synchronization::Arc::strong_count(&capture), 1);
    let slots = manager.0.read().await.slots.len();
    assert_eq!(slots, before);
    assert!(slots <= 4, "sequential spawning grew storage to {slots}");
    std::println!(
        "slot_size={} retained_slots={slots} sequential_spawns=1000 allocations={allocations} retained_bytes={retained}",
        core::mem::size_of::<embassy_executor::raw::TaskStorage<super::lifecycle::TaskFuture>>()
    );
}

#[test(task_path = crate)]
async fn configured_limit_and_parent_failure_roll_back() {
    let manager = alloc::boxed::Box::leak(alloc::boxed::Box::new(Manager::with_maximum_tasks(1)));
    let local = unsafe { embassy_executor::Spawner::for_current_executor().await };
    manager.register_spawner(local).unwrap();
    let root = manager.get_current_task_identifier().await;
    for parent in [root, TaskIdentifier::new(123)] {
        assert!(
            manager
                .spawn(parent, "limited", None, async |_| {})
                .await
                .is_err()
        );
    }
    assert_eq!(manager.get_task_count().await, 1);
    assert!(manager.0.read().await.slots.is_empty());
    manager
        .1
        .lock(|spawners| assert_eq!(spawners.borrow()[&0].tasks, 0));
}

#[test(task_path = crate)]
async fn foreign_executor_is_rejected_without_running_non_send_future() {
    let manager = initialize();
    let root = manager.get_current_task_identifier().await;
    let foreign = std::thread::spawn(move || {
        let executor = alloc::boxed::Box::leak(alloc::boxed::Box::new(
            embassy_executor::raw::Executor::new(core::ptr::null_mut()),
        ));
        manager.register_spawner(executor.spawner()).unwrap()
    })
    .join()
    .unwrap();
    let rc = alloc::rc::Rc::new(());
    let result = manager
        .spawn(root, "foreign", Some(foreign), move |_| async move {
            drop(rc);
        })
        .await;
    assert!(matches!(result, Err(Error::InvalidSpawnerIdentifier)));
    assert_eq!(manager.get_task_count().await, 1);
    manager.unregister_spawner(foreign).unwrap();
}

#[test(task_path = crate)]
async fn spawner_failure_and_metadata_contention_do_not_leak_or_spin() {
    let manager = alloc::boxed::Box::leak(alloc::boxed::Box::new(Manager::new()));
    assert!(matches!(
        manager
            .spawn(Manager::ROOT_TASK_IDENTIFIER, "missing", None, async |_| {})
            .await,
        Err(Error::NoSpawnerAvailable)
    ));
    assert_eq!(manager.get_task_count().await, 0);
    let local = unsafe { embassy_executor::Spawner::for_current_executor().await };
    let guard = manager.0.write().await;
    let id = manager.register_spawner(local).unwrap();
    manager.unregister_spawner(id).unwrap();
    drop(guard);
    manager.register_spawner(local).unwrap();
    let root = manager.get_current_task_identifier().await;
    assert!(matches!(
        manager
            .spawn(TaskIdentifier::new(123), "bad parent", None, async |_| {})
            .await,
        Err(Error::InvalidTaskIdentifier)
    ));
    assert_eq!(manager.get_task_count().await, 1);
    manager
        .1
        .lock(|spawners| assert_eq!(spawners.borrow()[&0].tasks, 0));
    manager.unregister(root).await.unwrap();
    assert_eq!(
        manager.try_get_current_task_identifier().await,
        Err(Error::ThreadNotRegistered)
    );
}

#[test(task_path = crate)]
async fn storage_grows_only_when_busy_and_respects_peak_limit() {
    let manager = alloc::boxed::Box::leak(alloc::boxed::Box::new(Manager::with_maximum_tasks(7)));
    let local = unsafe { embassy_executor::Spawner::for_current_executor().await };
    let spawner = manager.register_spawner(local).unwrap();
    let root = manager.get_current_task_identifier().await;
    let mut running = Vec::new();
    for index in 0..6 {
        let (release, wait) = JoinHandle::new();
        let (handle, id) = manager
            .spawn(
                root,
                "held",
                None,
                move |_| async move { wait.join().await },
            )
            .await
            .unwrap();
        let inner = manager.0.read().await;
        assert_eq!(inner.slots.len(), (index / 2 + 1) * 2);
        // The child has not run yet, but both sides of the identity map exist.
        assert_eq!(inner.identifiers[&inner.tasks[&id].internal_identifier], id);
        drop(inner);
        running.push((release, handle));
    }
    assert!(matches!(
        manager.spawn(root, "overflow", None, async |_| {}).await,
        Err(Error::TooManyTasks)
    ));
    assert_eq!(manager.unregister_spawner(spawner), Err(Error::AlreadySet));
    for (release, handle) in running {
        release.signal(());
        handle.join().await;
    }
    assert_eq!(manager.get_task_count().await, 1);
    assert_eq!(manager.0.read().await.slots.len(), 6);
    manager.unregister_spawner(spawner).unwrap();
}

#[test(task_path = crate)]
async fn send_spawn_balances_executors_and_keeps_non_send_state_remote() {
    let manager = initialize();
    let root = manager.get_current_task_identifier().await;
    let local = manager.get_spawner(root).await.unwrap();
    let remote = crate::test_executor::new_thread_executor(manager).await;
    let caller = std::thread::current().id();
    // The idle remote registration is least loaded; the root occupies `local`.
    let (release, wait) = JoinHandle::<()>::new();
    let (held, held_id) = manager
        .spawn_send(root, "held", None, move |id| async move {
            // Rc makes this future !Send: it must be created and dropped remotely.
            let identity = alloc::rc::Rc::new(id);
            wait.join().await;
            assert_eq!(manager.get_current_task_identifier().await, *identity);
            (std::thread::current().id(), *identity)
        })
        .await
        .unwrap();
    assert_eq!(manager.get_spawner(held_id).await, Ok(remote));
    // Equal loads choose the lowest identifier, here the caller's executor.
    let (balanced, balanced_id) = manager
        .spawn_send(root, "balanced", None, |_| async {
            std::thread::current().id()
        })
        .await
        .unwrap();
    assert_eq!(manager.get_spawner(balanced_id).await, Ok(local));
    assert_eq!(balanced.join().await, caller);
    release.signal(());
    let (thread, observed) = held.join().await;
    assert_ne!(thread, caller);
    assert_eq!(observed, held_id);
    // Local and Send tasks share the same bounded storage.
    let before = manager.0.read().await.slots.len().max(4);
    for _ in 0..100 {
        let remote_task = manager
            .spawn_send(root, "remote", Some(remote), |_| async {})
            .await
            .unwrap();
        remote_task.0.join().await;
        manager
            .spawn(root, "local", None, async |_| {})
            .await
            .unwrap()
            .0
            .join()
            .await;
    }
    let slots = manager.0.read().await.slots.len();
    assert!(
        slots <= before,
        "alternating executors grew storage to {slots}"
    );
    assert!(matches!(
        manager
            .spawn_send(root, "missing", Some(usize::MAX), |_| async {})
            .await,
        Err(Error::InvalidSpawnerIdentifier)
    ));
    assert_eq!(manager.get_task_count().await, 1);
    manager.1.lock(|spawners| {
        let spawners = spawners.borrow();
        assert_eq!(spawners[&local].tasks, 1);
        assert_eq!(spawners[&remote].tasks, 0);
    });
}

#[cfg(not(feature = "metrics"))]
#[test(task_path = crate)]
async fn metrics_are_unavailable_without_the_feature() {
    const { assert!(!crate::metrics::ENABLED) };
    assert!(initialize().get_metrics().await.is_none());
}

/// Busy-wait so the poll takes measurable time on the test clock.
#[cfg(feature = "metrics")]
fn spin(duration: Duration) {
    let start = std::time::Instant::now();
    while start.elapsed() < duration {
        core::hint::spin_loop();
    }
}

#[cfg(feature = "metrics")]
#[test(task_path = crate)]
async fn metrics_count_task_polls_and_executor_load() {
    let manager = initialize();
    let root = manager.get_current_task_identifier().await;
    let first = manager.get_metrics().await.unwrap();
    let release: &'static synchronization::signal::Signal<
        synchronization::blocking_mutex::raw::CriticalSectionRawMutex,
        (),
    > = alloc::boxed::Box::leak(alloc::boxed::Box::new(
        synchronization::signal::Signal::new(),
    ));
    let (worker, worker_identifier) = manager
        .spawn(root, "metrics worker", None, async move |_| {
            for _ in 0..3 {
                spin(Duration::from_millis(2));
                crate::yield_now().await;
            }
            release.wait().await;
        })
        .await
        .unwrap();
    // Let the worker run its three busy polls and park on the signal.
    for _ in 0..10 {
        crate::yield_now().await;
    }
    // A sleeping executor accumulates idle time through the test executor's hook.
    crate::sleep(Duration::from_millis(5)).await;
    let second = manager.get_metrics().await.unwrap();
    let task = second
        .tasks
        .iter()
        .find(|task| task.identifier == worker_identifier)
        .unwrap();
    assert_eq!(task.name, "metrics worker");
    assert!(task.polls >= 4, "{task:?}");
    // Measured durations may fall one clock tick short of the real ones.
    assert!(task.busy_nanoseconds >= 5_900_000, "{task:?}");
    assert!(task.longest_poll_nanoseconds >= 1_900_000, "{task:?}");
    assert!(task.longest_poll_nanoseconds <= task.busy_nanoseconds);
    let spawner = manager.get_spawner(root).await.unwrap();
    assert_eq!(task.spawner, spawner);
    let executor = unsafe { embassy_executor::Spawner::for_current_executor().await }.executor_id();
    assert_eq!(task.executor, Some(executor));

    let before = first
        .executors
        .iter()
        .find(|executor| executor.spawner == spawner)
        .unwrap();
    let after = second
        .executors
        .iter()
        .find(|executor| executor.spawner == spawner)
        .unwrap();
    assert_eq!(before.executor, after.executor);
    assert!(second.timestamp_nanoseconds > first.timestamp_nanoseconds);
    assert!(after.polls >= before.polls + 4);
    let busy = after.busy_nanoseconds - before.busy_nanoseconds;
    let idle = after.idle_nanoseconds.unwrap() - before.idle_nanoseconds.unwrap();
    let elapsed = after.timestamp_nanoseconds - before.timestamp_nanoseconds;
    assert!(after.timestamp_nanoseconds <= second.timestamp_nanoseconds);
    assert!(busy >= 5_900_000, "busy {busy}");
    // A runnable but descheduled thread records no idle time, so under load
    // the sleep can measure far less than 5 ms of idle time.
    assert!(idle > 0, "idle {idle}");
    // Same thread, nanosecond clock: no rounding or cross-core excess.
    assert!(busy + idle <= elapsed, "{busy} + {idle} > {elapsed}");

    release.signal(());
    worker.join().await;
    let third = manager.get_metrics().await.unwrap();
    assert!(
        !third
            .tasks
            .iter()
            .any(|task| task.identifier == worker_identifier)
    );
    let last = third
        .executors
        .iter()
        .find(|executor| executor.spawner == spawner)
        .unwrap();
    // Executor counters keep the time of finished tasks.
    assert!(last.busy_nanoseconds >= after.busy_nanoseconds);
    assert!(last.polls > after.polls);
}

#[cfg(feature = "metrics")]
#[test(task_path = crate)]
async fn reused_slots_restart_task_counters() {
    let manager = initialize();
    let root = manager.get_current_task_identifier().await;
    let mut generations = Vec::new();
    for _ in 0..2 {
        let (gate, identifier) = manager
            .spawn(root, "reused metrics", None, async |_| {
                crate::yield_now().await;
            })
            .await
            .unwrap();
        let snapshot = manager.get_metrics().await.unwrap();
        let task = snapshot
            .tasks
            .iter()
            .find(|task| task.identifier == identifier)
            .unwrap();
        // Not polled yet: the previous task's counters were reset on claim.
        assert_eq!(task.polls, 0);
        assert_eq!(task.busy_nanoseconds, 0);
        generations.push(task.generation);
        gate.join().await;
    }
    assert_ne!(generations[0], generations[1]);
}

#[cfg(feature = "metrics")]
#[test(task_path = crate)]
async fn executors_on_other_threads_are_reported_separately() {
    let manager = initialize();
    let root = manager.get_current_task_identifier().await;
    let local = manager.get_spawner(root).await.unwrap();
    let remote = crate::test_executor::new_thread_executor(manager).await;
    let (handle, identifier) = manager
        .spawn_send(root, "remote metrics", Some(remote), |_| async {
            spin(Duration::from_millis(3));
        })
        .await
        .unwrap();
    handle.join().await;
    let snapshot = manager.get_metrics().await.unwrap();
    assert!(
        !snapshot
            .tasks
            .iter()
            .any(|task| task.identifier == identifier)
    );
    let local = snapshot
        .executors
        .iter()
        .find(|executor| executor.spawner == local)
        .unwrap();
    let remote = snapshot
        .executors
        .iter()
        .find(|executor| executor.spawner == remote)
        .unwrap();
    assert_ne!(local.executor, remote.executor);
    assert!(remote.busy_nanoseconds >= 2_900_000, "{remote:?}");
    // The remote executor created its idle hook before registering.
    assert!(remote.idle_nanoseconds.is_some(), "{remote:?}");
}

#[cfg(feature = "metrics")]
#[test(task_path = crate)]
async fn executors_sampled_while_another_thread_writes_never_decrease() {
    use portable_atomic::{AtomicBool, Ordering};
    let manager = initialize();
    let root = manager.get_current_task_identifier().await;
    let remote = crate::test_executor::new_thread_executor(manager).await;
    let stop: &'static AtomicBool =
        alloc::boxed::Box::leak(alloc::boxed::Box::new(AtomicBool::new(false)));
    // The remote executor alternates short busy polls and short waits.
    let (handle, _) = manager
        .spawn_send(root, "metrics load", Some(remote), move |_| async move {
            while !stop.load(Ordering::Relaxed) {
                spin(Duration::from_micros(50));
                crate::sleep(Duration::from_micros(50)).await;
            }
        })
        .await
        .unwrap();
    let executor_of = |snapshot: &crate::metrics::MetricsSnapshot| {
        *snapshot
            .executors
            .iter()
            .find(|executor| executor.spawner == remote)
            .unwrap()
    };
    let mut previous = executor_of(&manager.get_metrics().await.unwrap());
    // Keep sampling until the remote executor alternated polls and waits
    // many times, even when both threads share one processor.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let mut samples = 0;
    while samples < 2_000 || previous.polls < 50 {
        assert!(std::time::Instant::now() < deadline, "{previous:?}");
        samples += 1;
        let current = executor_of(&manager.get_metrics().await.unwrap());
        assert_eq!(current.executor, previous.executor);
        assert!(current.timestamp_nanoseconds >= previous.timestamp_nanoseconds);
        assert!(current.polls >= previous.polls);
        assert!(
            current.busy_nanoseconds >= previous.busy_nanoseconds,
            "{previous:?} {current:?}"
        );
        let (previous_idle, current_idle) = (
            previous.idle_nanoseconds.unwrap_or(0),
            current.idle_nanoseconds.unwrap_or(0),
        );
        assert!(current_idle >= previous_idle, "{previous:?} {current:?}");
        // Per-executor timestamps bound the accounted time exactly on x86
        // with a nanosecond clock. Weakly ordered hosts can extrapolate a
        // period in progress by up to the writer's store latency.
        let tolerance = if cfg!(target_arch = "x86_64") {
            0
        } else {
            10_000
        };
        let accounted =
            (current.busy_nanoseconds - previous.busy_nanoseconds) + (current_idle - previous_idle);
        assert!(
            accounted <= current.timestamp_nanoseconds - previous.timestamp_nanoseconds + tolerance,
            "{previous:?} {current:?}"
        );
        previous = current;
    }
    stop.store(true, Ordering::Relaxed);
    handle.join().await;
    assert!(previous.busy_nanoseconds > 0, "{previous:?}");
    assert!(previous.idle_nanoseconds.unwrap_or(0) > 0, "{previous:?}");
}
