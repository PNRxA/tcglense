//! HTTP-level security tests.
//!
//! These drive the *real* application router (built by [`crate::build_router`],
//! including CORS, the JSON-body extractor, error mapping, and the auth stack)
//! in-process via `tower`'s `oneshot` — no TCP bind, no network. They assert the
//! security-relevant behaviour a unit test of any single module can't see on its
//! own: end-to-end refresh-token rotation + reuse detection, generic login
//! failures (no user enumeration), the hardened refresh cookie on the wire,
//! correct status codes for malformed bodies, the CORS contract, and that secret
//! material (password hashes) never leaks into a response.
//!
//! The shared HTTP harness lives in [`harness`]; each concern below is its own
//! module driving that harness.

mod harness;

mod api_keys;
mod caching;
mod captcha;
mod collection;
mod collection_import;
mod collection_products;
mod cors;
mod decks;
mod email_verification;
mod headers;
mod login;
mod mirror;
mod openapi;
mod pagination;
mod password_reset;
mod products;
mod public_collection;
mod rate_limit;
mod readiness;
mod refresh;
mod registration;
mod request_body;
mod request_params;
mod scan;
mod search;
mod sharing;
mod signup_toggle;
mod sitemap;
mod subtypes;
mod web_root;
mod wishlist;
mod wishlist_products;
