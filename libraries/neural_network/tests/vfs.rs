#![cfg(all(feature = "cmsis-nn", target_os = "linux"))]
//! Exercise the existing VFS API with an owned heap model.
extern crate abi_definitions;
extern crate critical_section;
drivers_std::memory::instantiate_global_allocator!();
use file_system::{AccessFlags, Flags, MemoryDevice, Position};
use neural_network::{COMPUTE, GET_INPUT_INFO, NeuralNetworkDevice};
use virtual_file_system::File;

#[task::test]
async fn owned_model_runs_through_mounted_vfs_device() {
    let tasks = Box::leak(Box::new(task::Manager::new()));
    let users = Box::leak(Box::new(users::Manager::new()));
    let time = Box::leak(Box::new(
        time::Manager::new(&drivers_std::devices::TimeDevice).unwrap(),
    ));
    let task = tasks.get_current_task_identifier().await;
    let disk = MemoryDevice::<512>::new_static(512 * 1024);
    little_fs::FileSystem::format(disk, 256).unwrap();
    let fs = little_fs::FileSystem::new(disk, 256).unwrap();
    let vfs = virtual_file_system::initialize(tasks, users, time, fs).unwrap();
    let bytes = include_bytes!("fixtures/classifier.tflite");
    let mut model = nanoglia::core::Block::new_zeroed(bytes.len(), nanoglia::Global).unwrap();
    model.as_mut_slice().copy_from_slice(bytes);
    let device = NeuralNetworkDevice::from_owned_model(model, nanoglia::Target::Cpu).unwrap();
    vfs.mount_character_device(task, "/nn", device)
        .await
        .unwrap();
    let mut first = File::open(&vfs, task, "/nn", Flags::from(AccessFlags::READ_WRITE))
        .await
        .unwrap();
    let mut second = File::open(&vfs, task, "/nn", Flags::from(AccessFlags::READ_WRITE))
        .await
        .unwrap();
    let info = first.control(GET_INPUT_INFO, &0u32).await.unwrap();
    let input = vec![0; info.byte_len as usize];
    for file in [&mut first, &mut second] {
        assert_eq!(file.write(&input).await.unwrap(), input.len());
        file.control(COMPUTE, &()).await.unwrap();
        file.set_position(&Position::Start(0)).await.unwrap();
        let mut output = [0; 64];
        assert!(file.read(&mut output).await.unwrap() > 0);
        assert!(file.control(COMPUTE, &()).await.is_err());
    }
    drop(first);
    drop(second);
    vfs.unmount("/nn", false).await.unwrap();
}
