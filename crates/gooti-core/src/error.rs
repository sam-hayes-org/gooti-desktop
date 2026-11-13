//! Errors. The messages are shown to users, so they follow the wording rules in `CLAUDE.md`.
//! Code that needs to tell errors apart uses [`Error::code`], never the message.

use thiserror::Error;

/// Shortest vault password. Same rule as the extension (`minPasswordLength = 4`).
pub const MIN_PASSWORD_LENGTH: usize = 4;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Gooti is not set up yet.")]
    NoVault,
    #[error("Gooti is already set up.")]
    VaultExists,
    #[error("Gooti is locked.")]
    Locked,
    #[error("Invalid password")]
    InvalidPassword,
    #[error("Use at least {MIN_PASSWORD_LENGTH} characters.")]
    PasswordTooShort,
    #[error("Gooti's data on this desktop is damaged or was made by a newer version of Gooti.")]
    UnreadableVault,
    #[error("Enter a nick.")]
    NickRequired,
    #[error("This is not a private key. Paste an nsec, an ncryptsec, or a hex key.")]
    InvalidPrivateKey,
    #[error("Enter the password for this ncryptsec key.")]
    KeyPasswordRequired,
    #[error("Wrong password for this ncryptsec key.")]
    InvalidKeyPassword,
    #[error("An identity with this public key already exists: {0}")]
    DuplicateIdentity(String),
    #[error("Identity not found.")]
    IdentityNotFound,
    #[error("This identity has no private key on this desktop.")]
    NoPrivateKey,
    #[error("A color must look like #1a2b3c.")]
    InvalidColor,
    #[error("That passphrase is not valid. Check your 12 words.")]
    InvalidPassphrase,
    #[error("This passphrase cannot open the backup on the relay.")]
    WrongPassphrase,
    #[error("The backup on the relay is damaged.")]
    InvalidBackup,
    #[error("A backup cannot be larger than 1 MB.")]
    VaultTooLarge,
    #[error("Add a relay first.")]
    NoRelays,
    #[error("Relays must be wss:// URLs.")]
    InvalidRelay,
    #[error("Turn on backup first.")]
    BackupOff,
    #[error("Backup is already on.")]
    BackupOn,
    #[error("No backup found for this passphrase on these relays.")]
    NoRemoteVault,
    #[error("{message}")]
    Relays {
        message: String,
        failures: Vec<RelayFailure>,
    },
    #[error("This is not a nostrconnect:// link.")]
    InvalidConnectLink,
    #[error("Connection not found.")]
    ConnectionNotFound,
    #[error("This identity is a bunker in a browser. This desktop cannot sign for it.")]
    CannotSign,
    #[error("Add a wss:// relay. Browsers reach this desktop through it.")]
    NoBrowserRelay,
    #[error("This app chose its relays when it connected. Connect it again to change them.")]
    AppRelaysFixed,
    #[error(
        "This connection is how your browsers reach a Desktop only identity. Use the identity in browsers again to remove it."
    )]
    BrowsersConnection,
    #[error("The keychain is not available: {0}")]
    Keychain(String),
    #[error("Could not save: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    /// Stable identifier for the UI and tests.
    pub fn code(&self) -> &'static str {
        match self {
            Error::NoVault => "no_vault",
            Error::VaultExists => "vault_exists",
            Error::Locked => "locked",
            Error::InvalidPassword => "invalid_password",
            Error::PasswordTooShort => "password_too_short",
            Error::UnreadableVault => "unreadable_vault",
            Error::NickRequired => "nick_required",
            Error::InvalidPrivateKey => "invalid_private_key",
            Error::KeyPasswordRequired => "key_password_required",
            Error::InvalidKeyPassword => "invalid_key_password",
            Error::DuplicateIdentity(_) => "duplicate_identity",
            Error::IdentityNotFound => "identity_not_found",
            Error::NoPrivateKey => "no_private_key",
            Error::InvalidColor => "invalid_color",
            Error::InvalidPassphrase => "invalid_passphrase",
            Error::WrongPassphrase => "wrong_passphrase",
            Error::InvalidBackup => "invalid_backup",
            Error::VaultTooLarge => "vault_too_large",
            Error::NoRelays => "no_relays",
            Error::InvalidRelay => "invalid_relay",
            Error::BackupOff => "backup_off",
            Error::BackupOn => "backup_on",
            Error::NoRemoteVault => "no_remote_vault",
            Error::Relays { .. } => "relays_failed",
            Error::InvalidConnectLink => "invalid_connect_link",
            Error::ConnectionNotFound => "connection_not_found",
            Error::CannotSign => "cannot_sign",
            Error::NoBrowserRelay => "no_browser_relay",
            Error::AppRelaysFixed => "app_relays_fixed",
            Error::BrowsersConnection => "browsers_connection",
            Error::Keychain(_) => "keychain",
            Error::Io(_) => "io",
        }
    }
}

/// One relay's own reason, shown when every relay failed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RelayFailure {
    pub url: String,
    pub reason: String,
}

impl Error {
    /// Per-relay reasons, when every relay failed.
    pub fn relay_failures(&self) -> &[RelayFailure] {
        match self {
            Error::Relays { failures, .. } => failures,
            _ => &[],
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn check_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LENGTH {
        return Err(Error::PasswordTooShort);
    }
    Ok(())
}
