//! Which page an address is.

use crate::overlay::Show;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Overlay(Show),
    Dashboard,
    Transcript,
}

/// The page for a path and a query string. `/` and every path that is not one
/// of the other two is the overlay, the page a browser source loads.
pub fn page_for(path: &str, search: &str) -> Page {
    match path.trim_end_matches('/') {
        "/dashboard" => Page::Dashboard,
        "/transcript" => Page::Transcript,
        _ => Page::Overlay(Show::from_query(search)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_paths_are_the_three_pages() {
        assert_eq!(page_for("/dashboard", ""), Page::Dashboard);
        assert_eq!(page_for("/transcript", ""), Page::Transcript);
        assert_eq!(page_for("/", ""), Page::Overlay(Show::Both));
        assert_eq!(page_for("/", "?show=final"), Page::Overlay(Show::Final));
        assert_eq!(page_for("/", "?show=partial"), Page::Overlay(Show::Partial));
    }

    #[test]
    fn a_trailing_slash_does_not_change_the_page() {
        assert_eq!(page_for("/dashboard/", ""), Page::Dashboard);
        assert_eq!(page_for("/transcript/", "?show=final"), Page::Transcript);
    }

    #[test]
    fn any_other_path_is_the_overlay() {
        assert_eq!(page_for("", ""), Page::Overlay(Show::Both));
        assert_eq!(page_for("/index.html", ""), Page::Overlay(Show::Both));
        assert_eq!(page_for("/dashboard2", ""), Page::Overlay(Show::Both));
    }
}
