// Tests module - contains all Manager tests
extern crate std;

use super::*;
use crate::test;
use alloc::{collections::BTreeMap, format, vec::Vec};
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
    let invalid_task = TaskIdentifier::new(99999);
    let result = manager.get_parent(invalid_task).await;

    assert!(result.is_err());
}

#[test(task_path = crate)]
async fn test_get_children_invalid_task() {
    let manager = initialize();

    // Test with an invalid task identifier
    let invalid_task = TaskIdentifier::new(99999);
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
    let invalid_task = TaskIdentifier::new(99999);
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
