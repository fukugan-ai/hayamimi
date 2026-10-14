//! R6 wiring: the real `hayamimi --wav tests/golden/ja/ja_014.wav --serve`
//! answers `/`, `/dashboard` and `/transcript` with the bytes of
//! `hayamimi-ui/web/index.html`, answers every file that index (and the
//! script it loads) reads under `/ui/` with 200, and answers a name that is
//! not in `/ui/` with 404.

mod support;

#[cfg(unix)]
mod wiring {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpStream};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    use crate::support::{
        BINARY, Failure, TestResult, clip, free_port, interrupt, models_dir, path_text,
        serve_driven,
    };

    /// The pages that must be the shell.
    const PAGES: [&str; 3] = ["/", "/dashboard", "/transcript"];

    /// The workspace directory (`native/`).
    fn workspace() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn index_path() -> PathBuf {
        workspace().join("hayamimi-ui/web/index.html")
    }

    /// The `ui` directory the server reads, next to the executable.
    fn bundle_dir() -> Result<PathBuf, Failure> {
        let exe = Path::new(BINARY);
        let parent = exe.parent().ok_or("the executable has no directory")?;
        Ok(parent.join("ui"))
    }

    fn run_tool(program: &str, args: &[&str], directory: &Path) -> Result<(), Failure> {
        let status = Command::new(program)
            .args(args)
            .current_dir(directory)
            .status()
            .map_err(|error| format!("cannot run {program}: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{program} {args:?} ended with {status}"))
        }
    }

    /// Builds the bundle into `out`, with the two commands of the guide.
    fn build_bundle(out: &Path) -> Result<(), Failure> {
        let native = workspace();
        run_tool(
            "cargo",
            &[
                "build",
                "--release",
                "--target",
                "wasm32-unknown-unknown",
                "-p",
                "hayamimi-ui",
                "--features",
                "web",
            ],
            &native,
        )?;
        let target = Path::new(BINARY)
            .parent()
            .and_then(Path::parent)
            .ok_or("the executable is not inside a target directory")?;
        let wasm = target.join("wasm32-unknown-unknown/release/hayamimi-ui.wasm");
        run_tool(
            "wasm-bindgen",
            &[
                "--target",
                "web",
                "--no-typescript",
                "--out-dir",
                &path_text(out)?,
                "--out-name",
                "hayamimi-ui",
                &path_text(&wasm)?,
            ],
            &native,
        )
    }

    /// One GET with `Connection: close`: the status and the body bytes.
    fn get(port: u16, path: &str) -> Result<(u16, Vec<u8>), Failure> {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .map_err(|error| format!("cannot connect to {port}: {error}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|error| error.to_string())?;
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .map_err(|error| format!("cannot send GET {path}: {error}"))?;
        let mut reply = Vec::new();
        stream
            .read_to_end(&mut reply)
            .map_err(|error| format!("cannot read the reply to GET {path}: {error}"))?;
        split_reply(&reply)
    }

    fn split_reply(reply: &[u8]) -> Result<(u16, Vec<u8>), Failure> {
        let end = reply
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or("the reply has no end of head")?;
        let head = String::from_utf8_lossy(&reply[..end]);
        let status = head
            .split_whitespace()
            .nth(1)
            .ok_or("the reply has no status")?
            .parse::<u16>()
            .map_err(|error| format!("the status is not a number: {error}"))?;
        Ok((status, reply[end + 4..].to_vec()))
    }

    /// Waits until the server answers on `port`.
    fn wait_for_server(port: u16) -> Result<(), Failure> {
        let deadline = Instant::now() + Duration::from_secs(120);
        while Instant::now() < deadline {
            if let Ok(stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) {
                // A connect to a port nobody listens on can complete against itself.
                if matches!((stream.local_addr(), stream.peer_addr()), (Ok(a), Ok(b)) if a != b) {
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(format!("nothing answered on port {port} in 120 s"))
    }

    /// The quoted names in `text` that start with `prefix`, in order.
    fn quoted_with<'a>(text: &'a str, quote: char, prefix: &str) -> Vec<&'a str> {
        text.split(quote)
            .filter(|part| part.starts_with(prefix))
            .collect()
    }

    /// Every `/ui/` URL the index reads, and those the scripts it loads import.
    fn bundle_urls(index: &str, bundle: &Path) -> Result<Vec<String>, Failure> {
        let mut urls: Vec<String> = quoted_with(index, '"', "/ui/")
            .into_iter()
            .map(str::to_owned)
            .collect();
        let scripts: Vec<String> = urls
            .iter()
            .filter(|url| url.ends_with(".js"))
            .cloned()
            .collect();
        for script in scripts {
            let name = script.trim_start_matches("/ui/");
            let text = std::fs::read_to_string(bundle.join(name))
                .map_err(|error| format!("cannot read {name} of the bundle: {error}"))?;
            for import in quoted_with(&text, '\'', "./snippets/") {
                urls.push(format!("/ui/{}", import.trim_start_matches("./")));
            }
        }
        urls.sort();
        urls.dedup();
        Ok(urls)
    }

    fn read_bytes(path: &Path) -> Result<Vec<u8>, Failure> {
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
    }

    fn expect_status(port: u16, path: &str, wanted: u16) -> Result<Vec<u8>, Failure> {
        let (status, body) = get(port, path)?;
        if status == wanted {
            Ok(body)
        } else {
            Err(format!("GET {path} gave {status}, wanted {wanted}"))
        }
    }

    fn check_pages(port: u16, index: &[u8]) -> Result<(), Failure> {
        for page in PAGES {
            if expect_status(port, page, 200)? != index {
                return Err(format!("GET {page} is not the bytes of web/index.html"));
            }
        }
        Ok(())
    }

    fn check_bundle(port: u16, index: &[u8], bundle: &Path) -> Result<(), Failure> {
        let text = String::from_utf8_lossy(index);
        let urls = bundle_urls(&text, bundle)?;
        // A walk that finds nothing would pass without reading anything.
        let count = |end: &str| urls.iter().filter(|url| url.ends_with(end)).count();
        if count(".wasm") == 0 || count(".js") < 2 {
            return Err(format!(
                "the index and its script name too few files: {urls:?}"
            ));
        }
        for url in &urls {
            let name = url.trim_start_matches("/ui/");
            if expect_status(port, url, 200)? != read_bytes(&bundle.join(name))? {
                return Err(format!("GET {url} is not the bytes of the bundle file"));
            }
        }
        Ok(())
    }

    /// A name the bundle does not have, a directory, and a way out of the
    /// directory to a file that exists (the control: it is there to be found).
    fn check_not_found(port: u16) -> Result<(), Failure> {
        if !Path::new(BINARY).is_file() {
            return Err(format!(
                "{BINARY} is not a file, so the way out proves nothing"
            ));
        }
        for path in [
            "/ui/no-such-file.js",
            "/ui/snippets",
            "/ui/../hayamimi",
            "/ui/",
        ] {
            expect_status(port, path, 404)?;
        }
        Ok(())
    }

    fn check_all(port: u16, bundle: &Path) -> Result<(), Failure> {
        wait_for_server(port)?;
        let index = read_bytes(&index_path())?;
        check_pages(port, &index)?;
        check_bundle(port, &index, bundle)?;
        check_not_found(port)
    }

    #[test]
    fn r6_the_real_server_serves_index_html_the_bundle_and_404_for_other_names() -> TestResult {
        let bundle = bundle_dir()?;
        build_bundle(&bundle)?;
        let models = path_text(&models_dir()?)?;
        let wav = path_text(&clip())?;
        let args = [
            "--wav",
            wav.as_str(),
            "--mode",
            "single",
            "--lang",
            "ja",
            "--models",
            models.as_str(),
        ];
        let port = free_port()?;
        serve_driven(
            BINARY,
            &args,
            (port, vec![port]),
            (Some("C.UTF-8"), &[]),
            |child| {
                check_all(port, &bundle)?;
                interrupt(child)
            },
        )?;
        Ok(())
    }

    #[test]
    fn a_reply_is_split_into_its_status_and_body() -> TestResult {
        let (status, body) = split_reply(b"HTTP/1.1 404 Not Found\r\nx: y\r\n\r\nab")?;
        assert_eq!((status, body.as_slice()), (404, b"ab".as_slice()));
        assert!(split_reply(b"GET / HTTP/1.1").is_err());
        Ok(())
    }

    #[test]
    fn the_names_a_script_imports_are_read_from_its_quotes() {
        let text = "import { a } from './snippets/x/inline0.js';\nimport b from \"./other.js\";";
        assert_eq!(
            quoted_with(text, '\'', "./snippets/"),
            ["./snippets/x/inline0.js"]
        );
        assert!(quoted_with(text, '\'', "./nothing/").is_empty());
    }
}
