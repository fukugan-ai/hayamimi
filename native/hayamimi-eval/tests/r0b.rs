//! R0b: the tests that read `testdata/` find it in a worktree, which does not
//! carry the directory, by asking where the main checkout is.

mod support;

use std::path::{Path, PathBuf};

use support::{TestResult, base, find_testdata, testdata};

#[test]
fn testdata_is_found_from_this_checkout() -> TestResult {
    let found = testdata()?;
    assert!(found.join("manifests").is_dir(), "{}", found.display());
    assert!(base()?.is_dir());
    Ok(())
}

#[test]
fn the_checkout_own_directory_comes_first() -> TestResult {
    let own = testdata()?;
    // The second place is never asked for when the first is a directory.
    let found = find_testdata(&own, || Err(String::from("must not be asked")))?;
    assert_eq!(found, own);
    Ok(())
}

#[test]
fn the_main_checkout_is_used_when_the_own_directory_is_missing() -> TestResult {
    let main = testdata()?;
    let found = find_testdata(Path::new("no-such-own-testdata"), || Ok(main.clone()))?;
    assert_eq!(found, main);
    Ok(())
}

#[test]
fn neither_place_is_an_error_that_names_both() {
    let said = find_testdata(Path::new("no-such-own-testdata"), || {
        Ok(PathBuf::from("no-such-main-testdata"))
    });
    let said = said.err();
    assert!(
        said.as_deref().is_some_and(|text| {
            text.contains("no-such-own-testdata") && text.contains("no-such-main-testdata")
        }),
        "{said:?}"
    );
}

#[test]
fn a_main_checkout_that_cannot_be_found_is_named_with_the_reason() {
    let said = find_testdata(Path::new("no-such-own-testdata"), || {
        Err(String::from("git rev-parse failed"))
    })
    .err();
    assert!(
        said.as_deref().is_some_and(|text| {
            text.contains("no-such-own-testdata") && text.contains("git rev-parse failed")
        }),
        "{said:?}"
    );
}
