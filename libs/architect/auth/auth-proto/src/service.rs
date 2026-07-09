//! The mountable RPC surface for architect-auth sessions.
//!
//! One `#[architect::rpc]` trait covering the session lifecycle:
//! sign-up, password sign-in, validate (`current_session`), refresh,
//! `whoami`, and sign-out. The trait is all-async, so `vox::service`
//! applies to it directly — the macro additionally emits the `serve` /
//! `layer` verbs, the deferred-bind `Service` token, and a glob-safe
//! [`prelude`] so the service mounts like any other architect service:
//!
//! ```ignore
//! use architect_auth::{AuthVoxService, auth_service_layer};
//!
//! let router = architect::LayerRouter::new()
//!     .merge(auth_service_layer(AuthVoxService::new(auth)));
//! ```
//!
//! Engine-only capabilities (TOTP, API keys, invitations, …) stay off
//! this surface on purpose: they remain reachable through
//! `ArchitectAuth` directly, and adding them later is purely additive —
//! new trait methods, no re-mount.

use crate::{AuthFlowError, AuthSessionBundle, AuthUser, SignInEmailPassword, SignUpEmailPassword};

/// Metadata key carrying the session token on vox calls.
///
/// Clients attach `Bearer <token>` under this key (see
/// `AuthClientMiddleware` / `auth-client`'s `TokenStoreMiddleware`);
/// `AuthServerMiddleware` parses it back out on the server side.
pub const AUTHORIZATION_METADATA_KEY: &str = "authorization";

// r[impl auth.transport.vox-schema]
#[architect::rpc]
pub trait AuthService {
    /// Create an email/password user and sign them in. Returns the
    /// freshly issued session bundle (the raw token is only returned
    /// here — only its hash is stored).
    async fn sign_up_email_password(
        &self,
        input: SignUpEmailPassword,
    ) -> Result<AuthSessionBundle, AuthFlowError>;

    /// Password sign-in for an existing user.
    async fn sign_in_email_password(
        &self,
        input: SignInEmailPassword,
    ) -> Result<AuthSessionBundle, AuthFlowError>;

    /// Validate a session token, returning the matching user + session.
    async fn current_session(&self, token: String) -> Result<AuthSessionBundle, AuthFlowError>;

    /// Rotate a valid session: issue a fresh token with a new expiry
    /// and deactivate the old one.
    async fn refresh_session(&self, token: String) -> Result<AuthSessionBundle, AuthFlowError>;

    /// Resolve a session token to its user — `current_session` minus
    /// the session details.
    async fn whoami(&self, token: String) -> Result<AuthUser, AuthFlowError>;

    /// Revoke the session behind a token. Idempotent: unknown or
    /// already-revoked tokens succeed without revealing existence.
    async fn sign_out(&self, token: String) -> Result<(), AuthFlowError>;
}
