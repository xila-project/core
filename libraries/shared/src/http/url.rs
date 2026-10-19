use core::fmt::{self, Display};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url<'a> {
    pub scheme: &'a str,
    pub host: &'a str,
    pub path: &'a str,
}

impl<'a> Url<'a> {
    pub fn parse(url: &'a str) -> Option<Self> {
        let scheme_end = url.find("://")?;
        let scheme = &url[..scheme_end];

        let host_start = scheme_end + 3;
        let path_start = url[host_start..]
            .find('/')
            .map_or(url.len(), |i| host_start + i);
        let host = &url[host_start..path_start];

        let path = if path_start < url.len() {
            &url[path_start..]
        } else {
            "/"
        };

        Some(Url { scheme, host, path })
    }
}

impl Display for Url<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}://{}{}", self.scheme, self.host, self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_parsing() {
        let url_str = "https://example.com/path/to/resource";
        let url = Url::parse(url_str).unwrap();
        assert_eq!(url.scheme, "https");
        assert_eq!(url.host, "example.com");
        assert_eq!(url.path, "/path/to/resource");
    }
    #[test]
    fn test_url_parsing_no_path() {
        let url_str = "http://example.com";
        let url = Url::parse(url_str).unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "example.com");
        assert_eq!(url.path, "/");
    }
}
