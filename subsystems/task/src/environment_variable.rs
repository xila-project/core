use alloc::{ffi::CString, format};
use core::fmt::Debug;
use synchronization::Arc;

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentVariable(Arc<CString>, usize);

impl Debug for EnvironmentVariable {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("EnvironmentVariable")
            .field("Name", &self.get_name())
            .field("Value", &self.get_value())
            .finish()
    }
}

impl EnvironmentVariable {
    /// Create a new environment variable.
    pub fn new(name: &str, value: &str) -> Self {
        let environment_variable = CString::new(format!("{name}={value}")).unwrap();
        Self(Arc::new(environment_variable), name.len())
    }

    /// Create a new environment variable from a raw string.
    ///
    /// # Example
    ///
    /// ```
    /// use task::EnvironmentVariable;
    ///
    /// let Environment_variable = EnvironmentVariable::new("Name", "Value");
    ///
    /// assert_eq!(Environment_variable.get_name(), "Name");
    /// ```
    pub fn get_name(&self) -> &str {
        self.0.to_str().unwrap()[..self.1].trim_end_matches('\0')
    }

    /// Get the value of the environment variable.
    ///
    /// # Example
    ///
    /// ```
    /// use task::EnvironmentVariable;
    ///
    /// let Environment_variable = EnvironmentVariable::new("Name", "Value");
    ///
    /// assert_eq!(Environment_variable.get_value(), "Value");
    /// ```
    pub fn get_value(&self) -> &str {
        self.0.to_str().unwrap()[self.1 + 1..].trim_end_matches('\0')
    }

    /// Get the inner raw CString.
    pub fn get_raw(&self) -> &CString {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_variable_creation() {
        let env_var = EnvironmentVariable::new("PATH", "/usr/bin:/bin");

        assert_eq!(env_var.get_name(), "PATH");
        assert_eq!(env_var.get_value(), "/usr/bin:/bin");
    }

    #[test]
    fn test_environment_variable_empty_name() {
        let env_var = EnvironmentVariable::new("", "some_value");

        assert_eq!(env_var.get_name(), "");
        assert_eq!(env_var.get_value(), "some_value");
    }

    #[test]
    fn test_environment_variable_empty_value() {
        let env_var = EnvironmentVariable::new("EMPTY_VAR", "");

        assert_eq!(env_var.get_name(), "EMPTY_VAR");
        assert_eq!(env_var.get_value(), "");
    }

    #[test]
    fn test_environment_variable_both_empty() {
        let env_var = EnvironmentVariable::new("", "");

        assert_eq!(env_var.get_name(), "");
        assert_eq!(env_var.get_value(), "");
    }

    #[test]
    fn test_environment_variable_special_characters() {
        let name = "TEST_VAR";
        let value = "value with spaces and !@#$%^&*()";
        let env_var = EnvironmentVariable::new(name, value);

        assert_eq!(env_var.get_name(), name);
        assert_eq!(env_var.get_value(), value);
    }

    #[test]
    fn test_environment_variable_equals_in_value() {
        let name = "CONFIG";
        let value = "key=value=another=part";
        let env_var = EnvironmentVariable::new(name, value);

        assert_eq!(env_var.get_name(), name);
        assert_eq!(env_var.get_value(), value);
    }

    #[test]
    fn test_environment_variable_unicode() {
        let name = "UNICODE_VAR";
        let value = "🦀 Rust is awesome! 中文测试";
        let env_var = EnvironmentVariable::new(name, value);

        assert_eq!(env_var.get_name(), name);
        assert_eq!(env_var.get_value(), value);
    }

    #[test]
    fn test_environment_variable_clone() {
        let env_var1 = EnvironmentVariable::new("HOME", "/home/user");
        let env_var2 = env_var1.clone();

        assert_eq!(env_var1.get_name(), env_var2.get_name());
        assert_eq!(env_var1.get_value(), env_var2.get_value());
        assert_eq!(env_var1, env_var2);
    }

    #[test]
    fn test_environment_variable_equality() {
        let env_var1 = EnvironmentVariable::new("USER", "alice");
        let env_var2 = EnvironmentVariable::new("USER", "alice");
        let env_var3 = EnvironmentVariable::new("USER", "bob");
        let env_var4 = EnvironmentVariable::new("HOME", "alice");

        assert_eq!(env_var1, env_var2);
        assert_ne!(env_var1, env_var3);
        assert_ne!(env_var1, env_var4);
    }

    #[test]
    fn test_environment_variable_hash() {
        use std::collections::HashMap;

        let env_var1 = EnvironmentVariable::new("PATH", "/usr/bin");
        let env_var2 = EnvironmentVariable::new("HOME", "/home/user");
        let env_var3 = EnvironmentVariable::new("PATH", "/usr/bin"); // Same as env_var1

        let mut map = HashMap::new();
        map.insert(env_var1.clone(), "first");
        map.insert(env_var2.clone(), "second");

        // Should not create a new entry since it's the same as env_var1
        map.insert(env_var3.clone(), "third");

        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&env_var1), Some(&"third")); // Should be overwritten
        assert_eq!(map.get(&env_var2), Some(&"second"));
        assert_eq!(map.get(&env_var3), Some(&"third"));
    }

    #[test]
    fn test_environment_variable_debug_format() {
        let env_var = EnvironmentVariable::new("DEBUG_VAR", "debug_value");
        let debug_string = format!("{env_var:?}");

        assert!(debug_string.contains("EnvironmentVariable"));
        assert!(debug_string.contains("DEBUG_VAR"));
        assert!(debug_string.contains("debug_value"));
    }

    #[test]
    fn test_environment_variable_get_raw() {
        let name = "RAW_TEST";
        let value = "raw_value";
        let env_var = EnvironmentVariable::new(name, value);
        let raw_cstring = env_var.get_raw();

        assert_eq!(raw_cstring.to_str().unwrap(), "RAW_TEST=raw_value");
    }

    #[test]
    fn test_environment_variable_long_name_and_value() {
        let long_name = "A".repeat(1000);
        let long_value = "B".repeat(2000);
        let env_var = EnvironmentVariable::new(&long_name, &long_value);

        assert_eq!(env_var.get_name(), long_name);
        assert_eq!(env_var.get_value(), long_value);
    }

    #[test]
    fn test_environment_variable_single_character() {
        let env_var = EnvironmentVariable::new("A", "B");

        assert_eq!(env_var.get_name(), "A");
        assert_eq!(env_var.get_value(), "B");
    }

    #[test]
    fn test_environment_variable_newlines_and_tabs() {
        let name = "MULTILINE";
        let value = "line1\nline2\tvalue";
        let env_var = EnvironmentVariable::new(name, value);

        assert_eq!(env_var.get_name(), name);
        assert_eq!(env_var.get_value(), value);
    }

    #[test]
    fn test_environment_variable_arc_sharing() {
        let env_var1 = EnvironmentVariable::new("SHARED", "value");
        let env_var2 = env_var1.clone();

        // Both should point to the same Arc
        assert_eq!(Arc::as_ptr(&env_var1.0), Arc::as_ptr(&env_var2.0));

        // Values should be identical
        assert_eq!(env_var1.get_name(), env_var2.get_name());
        assert_eq!(env_var1.get_value(), env_var2.get_value());
    }
}
