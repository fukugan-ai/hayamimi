//! The routes of the subtitle server: the event stream, the three pages and
//! the control routes. The control routes check a body (see [`crate::api`]),
//! update the shared view and hand the change to the recognition loop; a
//! route never touches the engine.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRef, Request, State};
use axum::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, CONTENT_TYPE, HOST, ORIGIN, VARY};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use hayamimi_asr::vad::Sensitivity;
use hayamimi_relay::origin::{Guard, Refusal, Verdict};
use serde::Serialize;
use serde_json::{Value, json};

use crate::api::{self, BadRequest, PairsOut};
use crate::control::Controls;
use crate::server::{self, Hub};
use crate::ui;

/// How long `POST /reset` waits for the loop before it answers 202.
const RESET_WAIT: Duration = Duration::from_secs(5);

/// What the handlers share.
#[derive(Clone)]
pub struct AppState {
    pub hub: Hub,
    pub controls: Arc<Controls>,
}

impl FromRef<AppState> for Hub {
    fn from_ref(state: &AppState) -> Hub {
        state.hub.clone()
    }
}

/// The routes of the server, for the pages of this machine.
pub fn router(state: AppState) -> Router {
    assemble(state, control_routes(), Guard::default())
}

/// The routes of the server for a listener on `serve_host`, for the pages of
/// this machine and the origins `guard` names. When `serve_host` is not a
/// loopback address, the control routes answer a peer on this machine only.
/// The routes must be served with connection info (see [`server::serve`]); a
/// request without a peer address is refused.
pub fn router_for(state: AppState, serve_host: IpAddr, guard: Guard) -> Router {
    if is_loopback_address(serve_host) {
        return assemble(state, control_routes(), guard);
    }
    let guarded = control_routes().layer(middleware::from_fn(loopback_peers_only));
    assemble(state, guarded, guard)
}

/// The pages, `/events` and the control routes, behind the guard: a request
/// of a page this machine does not serve, or to a host name that is not one of
/// this machine, is answered 403 before any route sees it.
fn assemble(state: AppState, control: Router<AppState>, guard: Guard) -> Router {
    Router::new()
        .route("/", get(ui::index))
        .route("/events", get(server::events))
        .route("/dashboard", get(ui::index))
        .route("/transcript", get(ui::index))
        .route("/ui/{*path}", get(ui::file))
        .merge(control)
        .layer(DefaultBodyLimit::max(api::MAX_BODY))
        .layer(middleware::from_fn_with_state(
            guard,
            pages_of_this_machine_only,
        ))
        .with_state(state)
}

/// The values of a header as text. A value that is not text is no browser's
/// and is counted against `unreadable`.
fn texts(headers: &HeaderMap, name: HeaderName, unreadable: Refusal) -> Result<Vec<&str>, Refusal> {
    headers
        .get_all(name)
        .iter()
        .map(HeaderValue::to_str)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| unreadable)
}

/// Lets a request through when its `Host` and `Origin` are those of a page of
/// this machine (see [`hayamimi_relay::origin`]), by the headers alone. A page
/// that is let in is told its own origin in the reply, never `*`, and a reply
/// of a route never carries another `access-control-allow-origin`.
async fn pages_of_this_machine_only(
    State(guard): State<Guard>,
    request: Request,
    next: Next,
) -> Response {
    let verdict = match (
        texts(request.headers(), HOST, Refusal::Host),
        texts(request.headers(), ORIGIN, Refusal::Origin),
    ) {
        (Err(refusal), _) | (_, Err(refusal)) => Verdict::Refuse(refusal),
        (Ok(hosts), Ok(origins)) => guard.judge(&hosts, &origins),
    };
    match verdict {
        Verdict::Refuse(refusal) => refuse_page(refusal),
        Verdict::Serve { echo } => tell_origin(next.run(request).await, echo.as_deref()),
    }
}

/// The 403 for a page or a host name this machine does not serve. It carries
/// no CORS header, so the page that was turned away cannot read it.
fn refuse_page(refusal: Refusal) -> Response {
    let message = match refusal {
        Refusal::Host => "this host name is not one of this machine (--allow-origin names one)",
        Refusal::Origin => "this page is not one this machine serves (--allow-origin names one)",
    };
    (
        StatusCode::FORBIDDEN,
        [(CONTENT_TYPE, "application/json; charset=utf-8")],
        json!({"error": message}).to_string(),
    )
        .into_response()
}

/// `reply` with `Vary: Origin`, and with the origin of the page it is for as
/// its `access-control-allow-origin` when the request named one.
fn tell_origin(mut reply: Response, echo: Option<&str>) -> Response {
    let headers = reply.headers_mut();
    headers.remove(ACCESS_CONTROL_ALLOW_ORIGIN);
    headers.append(VARY, HeaderValue::from_static("Origin"));
    // An origin the guard let in is made of characters a header holds. Were it
    // not, the reply would carry no allow-origin header and the browser would
    // not let the page read it.
    if let Some(Ok(value)) = echo.map(HeaderValue::from_str) {
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    reply
}

/// The routes that read or change the settings of the run.
fn control_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/replacements",
            get(get_replacements).post(post_replacements),
        )
        .route("/itn_overrides", get(get_itn).post(post_itn))
        .route("/config", get(get_config).post(post_config))
        .route("/reset", post(reset))
}

/// Whether `ip` is a loopback address: `127.0.0.0/8` and `::1`, with an IPv4
/// address wrapped in an IPv6 one read as the IPv4 address it wraps.
pub fn is_loopback_address(ip: IpAddr) -> bool {
    ip.to_canonical().is_loopback()
}

/// Lets a request through when its peer is a loopback address, by the peer
/// address of the connection alone and not by any header. The body of a
/// refused request is not read.
async fn loopback_peers_only(request: Request, next: Next) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip());
    if peer.is_some_and(is_loopback_address) {
        next.run(request).await
    } else {
        refuse_peer()
    }
}

/// The 403 for a peer that is not on this machine. It carries no CORS header.
fn refuse_peer() -> Response {
    (
        StatusCode::FORBIDDEN,
        [(CONTENT_TYPE, "application/json; charset=utf-8")],
        json!({"error": "this route answers a peer on this machine only"}).to_string(),
    )
        .into_response()
}

/// A JSON reply with the headers the existing server sent.
fn json_reply(status: StatusCode, body: String) -> Response {
    (
        status,
        [(CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response()
}

fn reply_value(status: StatusCode, value: &Value) -> Response {
    json_reply(status, value.to_string())
}

fn reply_serialized<T: Serialize>(value: &T) -> Response {
    match serde_json::to_string(value) {
        Ok(body) => json_reply(StatusCode::OK, body),
        Err(_) => reply_value(
            StatusCode::INTERNAL_SERVER_ERROR,
            &json!({"error": "could not write the reply"}),
        ),
    }
}

fn bad_request(error: &BadRequest) -> Response {
    reply_value(StatusCode::BAD_REQUEST, &json!({"error": error.0}))
}

/// The answer to a change: done, or the loop has ended and nothing can apply it.
fn applied(sent: bool) -> Response {
    if sent {
        reply_value(StatusCode::OK, &json!({"ok": true}))
    } else {
        reply_value(
            StatusCode::SERVICE_UNAVAILABLE,
            &json!({"error": "the session has ended"}),
        )
    }
}

async fn get_replacements(State(state): State<AppState>) -> Response {
    let pairs = state.controls.live().replacements();
    reply_serialized(&PairsOut(&pairs))
}

async fn post_replacements(State(state): State<AppState>, body: Bytes) -> Response {
    match api::parse_replacements(&body) {
        Ok(pairs) => applied(state.controls.set_replacements(pairs)),
        Err(error) => bad_request(&error),
    }
}

#[derive(Serialize)]
struct ItnOut<'a> {
    exclude: &'a [String],
    force: PairsOut<'a>,
}

async fn get_itn(State(state): State<AppState>) -> Response {
    let (exclude, force) = state.controls.live().itn();
    reply_serialized(&ItnOut {
        exclude: &exclude,
        force: PairsOut(&force),
    })
}

async fn post_itn(State(state): State<AppState>, body: Bytes) -> Response {
    match api::parse_itn(&body) {
        Ok(parsed) => applied(state.controls.set_itn(parsed)),
        Err(error) => bad_request(&error),
    }
}

/// A detector setting as a short decimal: 0.3 and not 0.30000001192092896.
fn short(value: f32) -> f64 {
    (f64::from(value) * 1e6).round() / 1e6
}

/// The body of `GET /config`: the members of the existing server, with
/// those this build does not have as null.
pub fn config_value(punctuate: bool, vad: Sensitivity) -> Value {
    json!({
        "lang": "ja",
        "dual_confirm": null,
        "punctuate": punctuate,
        "lid_switch_confirm": null,
        "min_switch_s": null,
        "translate": [],
        "vad": {
            "threshold": short(vad.threshold),
            "min_silence": short(vad.min_silence),
            "max_speech": short(vad.max_speech),
        },
    })
}

async fn get_config(State(state): State<AppState>) -> Response {
    let (punctuate, vad) = state.controls.live().config();
    reply_value(StatusCode::OK, &config_value(punctuate, vad))
}

async fn post_config(State(state): State<AppState>, body: Bytes) -> Response {
    match api::parse_config(&body) {
        Ok(change) => applied(state.controls.apply_config(change)),
        Err(error) => bad_request(&error),
    }
}

async fn reset(State(state): State<AppState>) -> Response {
    let Some(done) = state.controls.reset() else {
        return applied(false);
    };
    match tokio::time::timeout(RESET_WAIT, done).await {
        Ok(Ok(())) => applied(true),
        Ok(Err(_)) => applied(false),
        // The loop is busy: the reset is queued and will run, as the existing server answered.
        Err(_) => reply_value(StatusCode::ACCEPTED, &json!({"ok": false, "pending": true})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vad() -> Sensitivity {
        Sensitivity {
            threshold: 0.5,
            min_silence: 0.3,
            max_speech: 12.0,
        }
    }

    #[test]
    fn detector_settings_print_as_short_decimals() {
        assert_eq!(short(0.3), 0.3);
        assert_eq!(short(0.35), 0.35);
        assert_eq!(short(12.0), 12.0);
        assert_eq!(config_value(true, vad())["vad"]["min_silence"], 0.3);
    }

    #[test]
    fn the_config_has_the_seven_members_of_the_existing_server() {
        let config = config_value(true, vad());
        let object = config.as_object();
        for key in [
            "dual_confirm",
            "lang",
            "lid_switch_confirm",
            "min_switch_s",
            "punctuate",
            "translate",
            "vad",
        ] {
            assert!(object.is_some_and(|o| o.contains_key(key)), "{key}");
        }
        assert_eq!(object.map(|o| o.len()), Some(7));
        assert_eq!(config["lang"], "ja");
        assert_eq!(config["translate"], json!([]));
    }

    #[test]
    fn an_unapplied_change_is_a_503_and_an_applied_one_a_200() {
        assert_eq!(applied(true).status(), StatusCode::OK);
        assert_eq!(applied(false).status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn a_refused_body_is_a_400_carrying_the_reason() {
        let reply = bad_request(&BadRequest(String::from("nope")));
        assert_eq!(reply.status(), StatusCode::BAD_REQUEST);
    }

    /// T-BR-35-004: both ends of `127.0.0.0/8`, `::1`, the wrapped form, and what lies outside.
    #[test]
    fn the_loopback_judgement_covers_the_whole_127_range_and_the_wrapped_form()
    -> Result<(), std::net::AddrParseError> {
        for text in [
            "127.0.0.1",
            "127.0.0.0",
            "127.255.255.255",
            "::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(is_loopback_address(text.parse()?), "{text} is loopback");
        }
        for text in [
            "126.255.255.255",
            "128.0.0.0",
            "192.168.1.5",
            "::ffff:192.168.1.5",
            "::",
            "fe80::1",
            "0.0.0.0",
        ] {
            assert!(
                !is_loopback_address(text.parse()?),
                "{text} is not loopback"
            );
        }
        Ok(())
    }
}
