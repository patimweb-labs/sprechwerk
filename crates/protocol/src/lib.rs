//! Sprache auf der Leitung.
pub mod anhang;
pub mod frame;
pub mod geheimtext;
pub mod inhalt;
pub mod nachricht;
pub mod zertifikat;
pub use frame::{lese_rahmen, schreibe_rahmen, MAX_RAHMEN, PROTOKOLL_VERSION};

/// Schließcodes der QUIC-Verbindung (`CONNECTION_CLOSE`, Anwendungsfehler).
///
/// **Warum eigene Codes:** Ein abgelehnter Handschlag hinterlässt beim Client
/// nur „closed by peer: 0". Der Grund — falsches oder fehlendes Passwort —
/// muss aber beim Menschen ankommen, nicht im Transportfehler verschwinden.
/// Die Zahlen sind Teil des Vertrags: sie stehen im `CONNECTION_CLOSE`-Rahmen
/// und werden vom Client gelesen (`adapters::ablehnung_deuten`).
pub mod schliessen {
    /// Der Server verlangt ein Passwort, der Client hat keines geschickt.
    pub const PASSWORT_NOETIG: u32 = 5;
    /// Der Client hat ein Passwort geschickt, es stimmt nicht.
    pub const PASSWORT_FALSCH: u32 = 6;

    /// Der Text, den der Mensch dazu lesen soll.
    pub fn text(code: u32) -> Option<&'static str> {
        match code {
            PASSWORT_NOETIG => Some("Passwort nötig — der Server verlangt eines"),
            PASSWORT_FALSCH => Some("Passwort falsch"),
            _ => None,
        }
    }
}
pub use nachricht::{
    ClientFrame, Fehlercode, Mitgliedsansicht, Raumansicht, Schluesselstand, ServerFrame,
    Sprachkopf, CODEC_OPUS, CODEC_VERSCHLUESSELT, MAX_SPRACHRAHMEN, SPRACHKOPF_LEN,
    SPRACHKOPF_VERSION,
};
pub use zertifikat::{
    gleich, gruppiere, normalisiere, pruefe_fingerabdruck, zertifikat_fingerabdruck,
    FINGERABDRUCK_LEN,
};
