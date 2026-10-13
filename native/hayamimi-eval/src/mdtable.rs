//! Markdown tables, read and written the way the stage documents use them:
//! a heading, a table made of `|` rows, and nothing clever.

use crate::error::EvalError;

/// A table with its header row; the separator row is not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdTable {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// The text from the first line that starts with `heading` up to, not
/// including, the next heading of the same or a higher level.
pub fn section<'a>(text: &'a str, heading: &str) -> Result<&'a str, EvalError> {
    let level = heading.chars().take_while(|c| *c == '#').count();
    let mut start = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let is_heading = line.starts_with('#');
        let here = line.chars().take_while(|c| *c == '#').count();
        match start {
            None if line.starts_with(heading) => start = Some(offset),
            Some(from) if is_heading && here <= level => return Ok(&text[from..offset]),
            _ => {}
        }
        offset += line.len();
    }
    match start {
        Some(from) => Ok(&text[from..]),
        None => Err(EvalError::Invalid(format!("no section `{heading}`"))),
    }
}

fn cells(line: &str) -> Vec<String> {
    // A row is `| a | b |`: one pipe opens it and one closes it.
    let inner = line.trim().trim_start_matches('|');
    let inner = inner.strip_suffix('|').map_or(inner, str::trim_end);
    inner
        .split('|')
        .map(|cell| cell.trim().to_owned())
        .collect()
}

fn is_separator(line: &str) -> bool {
    let inner = line.trim().trim_matches('|');
    !inner.is_empty() && inner.chars().all(|c| matches!(c, '-' | ':' | ' ' | '|'))
}

/// The first table in `text`: the first run of lines that start with `|`.
pub fn first_table(text: &str) -> Result<MdTable, EvalError> {
    let mut lines = text
        .lines()
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'));
    let Some(header) = lines.next() else {
        return Err(EvalError::Invalid(String::from("no table")));
    };
    let rows = lines
        .filter(|line| !is_separator(line))
        .map(cells)
        .collect();
    Ok(MdTable {
        header: cells(header),
        rows,
    })
}

impl MdTable {
    /// The index of the column called `name`.
    pub fn column(&self, name: &str) -> Result<usize, EvalError> {
        self.header
            .iter()
            .position(|cell| cell == name)
            .ok_or_else(|| EvalError::Invalid(format!("no column `{name}`")))
    }

    /// The cell of `row` under the column called `name`; an absent or short row is an error.
    pub fn get<'a>(&self, row: &'a [String], name: &str) -> Result<&'a str, EvalError> {
        let at = self.column(name)?;
        row.get(at)
            .map(String::as_str)
            .ok_or_else(|| EvalError::Invalid(format!("a row has no cell under `{name}`")))
    }
}

/// A table rendered as Markdown lines.
pub fn render(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = format!("| {} |\n", header.join(" | "));
    out.push_str(&format!("|{}|\n", vec!["---"; header.len()].join("|")));
    for row in rows {
        out.push_str(&format!("| {} |\n", row.join(" | ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# top\n\n## a\n\n| x | y |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\n## b\n\n| z |\n|---|\n| 9 |\n";

    #[test]
    fn a_section_ends_at_the_next_heading_of_its_level() -> Result<(), EvalError> {
        let found = section(DOC, "## a")?;
        assert!(found.contains("| 3 | 4 |"));
        assert!(!found.contains("| 9 |"));
        Ok(())
    }

    #[test]
    fn a_missing_section_is_an_error() {
        assert!(matches!(section(DOC, "## c"), Err(EvalError::Invalid(_))));
    }

    #[test]
    fn the_last_section_runs_to_the_end() -> Result<(), EvalError> {
        assert!(section(DOC, "## b")?.contains("| 9 |"));
        Ok(())
    }

    #[test]
    fn a_table_keeps_its_header_and_drops_the_separator() -> Result<(), EvalError> {
        let table = first_table(section(DOC, "## a")?)?;
        assert_eq!(table.header, ["x", "y"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[1], ["3", "4"]);
        Ok(())
    }

    #[test]
    fn text_without_a_table_is_an_error() {
        assert!(matches!(first_table("none"), Err(EvalError::Invalid(_))));
    }

    #[test]
    fn a_column_is_found_by_name_and_a_missing_one_is_an_error() -> Result<(), EvalError> {
        let table = first_table(section(DOC, "## a")?)?;
        assert_eq!(table.get(&table.rows[0], "y")?, "2");
        assert!(matches!(table.column("q"), Err(EvalError::Invalid(_))));
        Ok(())
    }

    #[test]
    fn a_short_row_has_no_cell() -> Result<(), EvalError> {
        let table = first_table(section(DOC, "## a")?)?;
        let short = vec![String::from("1")];
        assert!(matches!(table.get(&short, "y"), Err(EvalError::Invalid(_))));
        Ok(())
    }

    #[test]
    fn rendering_then_reading_gives_the_same_rows() -> Result<(), EvalError> {
        let rows = vec![vec![String::from("a"), String::from("b")]];
        let text = render(&["h1", "h2"], &rows);
        let table = first_table(&text)?;
        assert_eq!(table.rows, rows);
        Ok(())
    }
}
