//! Account name and home rules shared by every account store.

/// Longest accepted user name, in bytes (the same limit as Linux `useradd`).
pub const MAXIMUM_USER_NAME_LENGTH: usize = 32;

/// Home of the built-in root account, which has no account record.
pub const ROOT_HOME: &str = "/users/root";

/// Name of the group whose members administer the device, for example by
/// running a command as another user. Seeded images create it with the
/// `administrator` account as its member.
pub const ADMINISTRATOR_GROUP_NAME: &str = "administrator";

/// Whether `name` may be given to a new or renamed account.
///
/// A user name becomes the final component of its account record path and of
/// its default home (`/users/<name>`), so the accepted set is deliberately
/// small: an ASCII letter followed by at most 31 ASCII letters, digits, `_` or
/// `-`. This rejects empty and overlong names, `/`, `.` and `..`, NUL and
/// control characters, whitespace and non-ASCII text. The leading letter also
/// keeps a name from colliding with the numeric `/users/<identifier>` homes of
/// accounts created before homes were named after users.
///
/// Names are case-sensitive. Existing accounts are not revalidated when they
/// are loaded, so older names keep working.
pub fn is_valid_user_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    name.len() <= MAXIMUM_USER_NAME_LENGTH
        && first.is_ascii_alphabetic()
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// Whether `home` is an acceptable recorded home: a single entry directly
/// below `/users`, such as `/users/alice` or the `/users/1000` of an older
/// account. The entry may not be `.` or `..`, nor contain `/`, NUL or control
/// characters, and is at most 255 bytes long.
pub fn is_valid_user_home(home: &str) -> bool {
    let Some(entry) = home.strip_prefix("/users/") else {
        return false;
    };
    !entry.is_empty()
        && entry.len() <= 255
        && !matches!(entry, "." | "..")
        && !entry
            .chars()
            .any(|character| character == '/' || character.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_documented_character_set() {
        for name in ["a", "administrator", "Alice", "build_bot-2", "z0"] {
            assert!(is_valid_user_name(name), "{name}");
        }
        assert!(is_valid_user_name(&"a".repeat(MAXIMUM_USER_NAME_LENGTH)));
    }

    #[test]
    fn rejects_names_that_could_escape_or_collide() {
        for name in [
            "",
            ".",
            "..",
            ".hidden",
            "a/b",
            "/",
            "../root",
            "a\0b",
            "a\nb",
            "tab\t",
            "with space",
            "1000",
            "0",
            "-option",
            "_private",
            "a.b",
            "é",
            "naïve",
        ] {
            assert!(!is_valid_user_name(name), "{name:?}");
        }
        assert!(!is_valid_user_name(
            &"a".repeat(MAXIMUM_USER_NAME_LENGTH + 1)
        ));
    }

    #[test]
    fn homes_are_single_entries_below_users() {
        for home in ["/users/alice", "/users/1000", ROOT_HOME] {
            assert!(is_valid_user_home(home), "{home}");
        }
        for home in [
            "",
            "/users",
            "/users/",
            "/users/.",
            "/users/..",
            "/users/a/b",
            "/users/a/",
            "/users/a\0",
            "/users/a\n",
            "/system/users/alice",
            "users/alice",
            "/usersalice",
        ] {
            assert!(!is_valid_user_home(home), "{home:?}");
        }
    }
}
