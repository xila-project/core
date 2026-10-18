#[cfg(all(test, target_os = "linux"))]
mod tests {
    use xila::graphics::appearance::Appearance;
    use xila::graphics::appearance::restore_appearance;
    use xila::{
        file_system::Permissions,
        virtual_file_system::{File, user_home},
    };

    #[xila::task::test(task_path = xila::task)]
    async fn preferences_survive_reopening_and_invalid_files_are_rejected() {
        let standard = testing::initialize(false, false).await;
        let context = standard.context;
        let task = context.task_manager.get_current_task_identifier().await;
        let vfs = context.virtual_file_system;
        let path = user_home(xila::users::UserIdentifier::ROOT)
            .append(".appearance")
            .unwrap();
        let root_home = vfs
            .ensure_user_home(task, xila::users::UserIdentifier::ROOT)
            .await
            .unwrap();
        assert_eq!(root_home.as_str(), "/users/0");
        let stats = vfs.get_statistics(&root_home).await.unwrap();
        assert_eq!(stats.permissions, Permissions::USER_FULL);
        assert_eq!(
            Appearance::load(vfs, task).await.unwrap(),
            Appearance::default()
        );
        let appearance = Appearance {
            is_dark: false,
            primary: 3,
            secondary: 5,
        };
        appearance.save(vfs, task).await.unwrap();
        assert_eq!(
            vfs.get_statistics(&path).await.unwrap().permissions,
            Permissions::USER_READ_WRITE
        );
        assert_eq!(Appearance::load(vfs, task).await.unwrap(), appearance);
        let replacement = Appearance {
            primary: 2,
            ..appearance
        };
        replacement.save(vfs, task).await.unwrap();
        assert_eq!(Appearance::load(vfs, task).await.unwrap(), replacement);
        File::write_to_path(vfs, task, &path, b"broken")
            .await
            .unwrap();
        assert!(Appearance::load(vfs, task).await.is_err());
        replacement.save(vfs, task).await.unwrap();
        let mut oversized = alloc::vec::Vec::new();
        File::read_from_path(vfs, task, &path, &mut oversized)
            .await
            .unwrap();
        oversized.push(0);
        File::write_to_path(vfs, task, &path, &oversized)
            .await
            .unwrap();
        assert!(Appearance::load(vfs, task).await.is_err());

        let user = xila::users::UserIdentifier::new(42);
        let group = xila::users::GroupIdentifier::new(42);
        context
            .users_manager
            .add_group(group, "appearance-test", &[])
            .await
            .unwrap();
        context
            .users_manager
            .add_user(user, "appearance-test", group)
            .await
            .unwrap();
        let home = vfs.ensure_user_home(task, user).await.unwrap();
        assert_eq!(home.as_str(), "/users/42");
        let stats = vfs.get_statistics(&home).await.unwrap();
        assert_eq!(stats.user, user);
        assert_eq!(stats.group, group);
        assert_eq!(stats.permissions, Permissions::USER_FULL);
        context.task_manager.set_user(task, user).await.unwrap();
        context.task_manager.set_group(task, group).await.unwrap();
        // Every user starts with their own defaults and cannot read root's record.
        assert_eq!(
            Appearance::load(vfs, task).await.unwrap(),
            Appearance::default()
        );
        assert!(
            File::open(
                vfs,
                task,
                &path,
                xila::file_system::AccessFlags::Read.into()
            )
            .await
            .is_err()
        );
        replacement.save(vfs, task).await.unwrap();
        assert_eq!(Appearance::load(vfs, task).await.unwrap(), replacement);
        context
            .task_manager
            .set_user(task, xila::users::UserIdentifier::ROOT)
            .await
            .unwrap();
        context
            .task_manager
            .set_group(task, xila::users::GroupIdentifier::ROOT)
            .await
            .unwrap();

        assert!(Appearance::load(vfs, task).await.is_err());
        // Simulate boot with a fresh graphics manager, then exercise the real
        // controls and a second window without a native display server.
        replacement.save(vfs, task).await.unwrap();
        let graphics = alloc::boxed::Box::leak(alloc::boxed::Box::new(
            xila::graphics::initialize(
                context.time_manager,
                &HeadlessScreen,
                &HeadlessScreen,
                xila::graphics::InputKind::Pointer,
                1024,
                false,
            )
            .await,
        ));
        xila::graphics::set_ffi_manager(graphics);
        restore_appearance(graphics, vfs, task).await;
        crate::tabs::appearance::tests::exercise_ui(context, graphics).await;
    }

    struct HeadlessScreen;
    impl xila::file_system::MountOperations for HeadlessScreen {}
    impl xila::file_system::DirectCharacterDevice for HeadlessScreen {}
    impl xila::file_system::DirectBaseOperations for HeadlessScreen {
        fn read(&self, _: &mut [u8], _: u64) -> xila::file_system::Result<usize> {
            Ok(0)
        }
        fn write(&self, buffer: &[u8], _: u64) -> xila::file_system::Result<usize> {
            Ok(buffer.len())
        }
        fn control(
            &self,
            command: xila::file_system::ControlCommandIdentifier,
            _: &xila::shared::AnyByLayout,
            output: &mut xila::shared::AnyByLayout,
        ) -> xila::file_system::Result<()> {
            use xila::{file_system::ControlCommand, graphics::GET_RESOLUTION};
            if command == GET_RESOLUTION::IDENTIFIER {
                *GET_RESOLUTION::cast_output(output)? = xila::graphics::Point::new(800, 600);
                Ok(())
            } else {
                Err(xila::file_system::Error::UnsupportedOperation)
            }
        }
    }
}
