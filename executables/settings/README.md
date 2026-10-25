# Settings

The Appearance tab changes the shared Shadcn theme for all open windows in the
current session. Changes apply immediately and are saved to
`/users/<uid>/.appearance`.

The default is **Zinc dark**, with automatic primary and secondary colors from
the shared theme's zinc palette. Selecting light mode also switches automatic
colors to zinc light. Blue, green, orange, rose and violet presets keep their
colors when switching modes; the theme chooses contrasting text automatically.

The graphical shell restores each user's appearance after login, before opening
the desktop. Native and browser startup also restore root's appearance for the
login screen. Other platforms can call `graphics::appearance::restore_appearance`
after mounting the filesystem or after login; the caller must not hold the LVGL
lock.

Both shells create a private home at `/users/<uid>` before dropping login's
privileges and set `HOME` for the session. Numeric user IDs keep home paths stable
when an account is renamed. Home directories are owned by that user and their
primary group, with mode 0700; appearance records have mode 0600. Missing
preferences use the default. Invalid or unreadable preferences are logged and
leave the startup theme intact; Settings displays a warning and allows saving a
replacement.

Preferences use a bounded, versioned record with a checksum. Settings writes and
flushes a temporary file before replacing the saved file with a filesystem
rename. If saving fails, the selected appearance still applies for this session
and Settings displays an error. Reboot persistence requires a persistent backing
disk; an in-memory or newly formatted disk starts with the default appearance.
