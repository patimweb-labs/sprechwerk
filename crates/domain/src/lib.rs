//! sprechwerk — Zustand ohne Netz.
//!
//! Dieses Crate kennt weder Sockets noch Datenbanken. Es beschreibt, **was
//! gilt**: Räume, Mitglieder, Rollen, Nachrichten, Prüfprotokoll — und welche
//! Übergänge erlaubt sind. Jede Übergangsfunktion prüft Rechte, schreibt ins
//! Prüfprotokoll und gibt entweder das Ergebnis oder einen Fehler zurück.
//!
//! Der Zeitpunkt wird **von außen** übergeben (`jetzt_ms`), damit Tests ohne
//! Uhr laufen und der Server eine einzige Zeitquelle hat.

pub mod ids;
pub mod role;
pub mod state;

pub use ids::{MemberId, MessageId, RoomId};
pub use role::{Capability, Role};
pub use state::{
    AuditEntry, Ban, Error, Member, Message, Room, State, DEFAULT_ROOM_NAME, FORMAT_VERSION,
    MAX_MESSAGE_LEN,
};
