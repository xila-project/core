#[macro_export]
macro_rules! mount {

    ( $virtual_file_system:expr, $task:expr, &[ $( (
        $path:expr, $kind:expr, $device:expr) ),* $(,)? ] ) => {

    async || -> Result<(), $crate::exported_file_system::Error>
    {
        $(
            let __device = Box::leak(Box::new($device));
            $virtual_file_system.mount_static($task, $path, ItemStatic::$kind(__device)).await?;

        )*

        Ok(())
    }()
};

}

#[macro_export]
macro_rules! mount_static {
    ( $virtual_file_system:expr, $task:expr, &[ $( ( $path:expr, $kind:ident, $device:expr ) ),* $(,)? ] ) => {
    async || -> $crate::Result<()>
    {
        $(
            let _ = $virtual_file_system.remove($task, $path).await;
            $virtual_file_system.mount_static($task, $path, $crate::ItemStatic::$kind(&$device)).await?;

        )*

        Ok(())
    }()
};

}
