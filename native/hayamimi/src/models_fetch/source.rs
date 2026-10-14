//! Getting one group from one source into a stage: a release archive, a single
//! release file, or the files of a Hugging Face repository. Nothing here
//! touches the models directory; the caller installs the stage when this has
//! returned the names it wrote.

use std::io::Read;

use serde_json::Value;

use super::archive;
use super::error::FetchError;
use super::items::{Files, Item, Source};
use super::net::Net;
use super::options::Options;
use super::stage::Stage;

fn release_url(options: &Options, tag: &str, file: &str) -> String {
    format!("{}/{tag}/{file}", options.release_url)
}

fn hf_file_url(options: &Options, repo: &str, name: &str) -> String {
    format!("{}/{repo}/resolve/main/{name}", options.hf_url)
}

fn hf_listing_url(options: &Options, repo: &str) -> String {
    format!("{}/api/models/{repo}", options.hf_url)
}

/// Whether a name from a listing stays inside the group's directory and is made
/// only of characters that need no escaping in an address.
fn is_plain_name(name: &str) -> bool {
    let chars_ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'));
    chars_ok
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Writes one body to the stage; an empty body is a failure, not a file.
fn write_body(stage: &Stage, name: &str, url: &str, body: &mut dyn Read) -> Result<(), FetchError> {
    if stage.write(name, body)? == 0 {
        return Err(FetchError::Net {
            url: url.to_owned(),
            message: String::from("the body is empty"),
        });
    }
    Ok(())
}

/// The names of the files of a repository, from its listing.
fn listing_names(net: &dyn Net, options: &Options, repo: &str) -> Result<Vec<String>, FetchError> {
    let url = hf_listing_url(options, repo);
    let mut body = Vec::new();
    net.open(&url)?
        .read_to_end(&mut body)
        .map_err(|e| FetchError::Net {
            url: url.clone(),
            message: e.to_string(),
        })?;
    let bad = || FetchError::BadListing { url: url.clone() };
    let parsed: Value = serde_json::from_slice(&body).map_err(|_| bad())?;
    let siblings = parsed
        .get("siblings")
        .and_then(Value::as_array)
        .ok_or_else(bad)?;
    let mut names = Vec::with_capacity(siblings.len());
    for sibling in siblings {
        let name = sibling
            .get("rfilename")
            .and_then(Value::as_str)
            .ok_or_else(bad)?;
        if name == ".gitattributes" {
            continue;
        }
        if !is_plain_name(name) {
            return Err(FetchError::BadName {
                name: name.to_owned(),
            });
        }
        names.push(name.to_owned());
    }
    if names.is_empty() {
        return Err(bad());
    }
    Ok(names)
}

/// The names to fetch from a repository: the group's own, or the whole listing.
fn hf_names(
    net: &dyn Net,
    options: &Options,
    item: &Item,
    repo: &str,
) -> Result<Vec<String>, FetchError> {
    match item.files {
        Files::Named(names) => Ok(names.iter().map(|name| (*name).to_owned()).collect()),
        Files::Snapshot { .. } => listing_names(net, options, repo),
    }
}

fn from_hf(
    net: &dyn Net,
    options: &Options,
    item: &Item,
    repo: &str,
    stage: &Stage,
) -> Result<Vec<String>, FetchError> {
    let names = hf_names(net, options, item, repo)?;
    for name in &names {
        let url = hf_file_url(options, repo, name);
        let mut body = net.open(&url)?;
        write_body(stage, name, &url, &mut body)?;
    }
    Ok(names)
}

fn from_tar(
    net: &dyn Net,
    options: &Options,
    item: &Item,
    (tag, file): (&str, &str),
    stage: &Stage,
) -> Result<Vec<String>, FetchError> {
    let url = release_url(options, tag, file);
    let wanted = item.marker_names();
    archive::extract(net.open(&url)?, &url, wanted, stage)?;
    Ok(wanted.iter().map(|name| (*name).to_owned()).collect())
}

fn from_asset(
    net: &dyn Net,
    options: &Options,
    item: &Item,
    (tag, file): (&str, &str),
    stage: &Stage,
) -> Result<Vec<String>, FetchError> {
    let url = release_url(options, tag, file);
    let [name] = item.marker_names() else {
        return Err(FetchError::MissingMember {
            names: vec![file.to_owned()],
        });
    };
    let mut body = net.open(&url)?;
    write_body(stage, name, &url, &mut body)?;
    Ok(vec![(*name).to_owned()])
}

/// Writes the group's files from `source` into `stage`; returns their names.
pub fn gather(
    net: &dyn Net,
    options: &Options,
    item: &Item,
    source: Source,
    stage: &Stage,
) -> Result<Vec<String>, FetchError> {
    match source {
        Source::Tar { tag, file } => from_tar(net, options, item, (tag, file), stage),
        Source::Asset { tag, file } => from_asset(net, options, item, (tag, file), stage),
        Source::Hf { repo } => from_hf(net, options, item, repo, stage),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_stays_inside_the_directory() {
        for name in ["a.onnx", "sub/b-1_2.json", "x"] {
            assert!(is_plain_name(name), "{name}");
        }
        for name in [
            "",
            "/etc/passwd",
            "../x",
            "a/../b",
            "a//b",
            "a b",
            "a\\b",
            "./a",
            "é",
        ] {
            assert!(!is_plain_name(name), "{name}");
        }
    }

    #[test]
    fn the_addresses_follow_the_two_layouts() {
        let options = Options {
            release_url: String::from("http://r"),
            hf_url: String::from("http://h"),
            ..Options::new()
        };
        assert_eq!(
            release_url(&options, "tag", "f.tar.bz2"),
            "http://r/tag/f.tar.bz2"
        );
        assert_eq!(
            hf_file_url(&options, "o/n", "a.onnx"),
            "http://h/o/n/resolve/main/a.onnx"
        );
        assert_eq!(hf_listing_url(&options, "o/n"), "http://h/api/models/o/n");
    }
}
