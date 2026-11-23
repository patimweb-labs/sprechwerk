//! Spielnetz: Spiele im Raum freigeben (ADR 0016).
//!
//! Drei Bausteine, alle ohne I/O:
//!
//! * [`protokoll`] — Rahmen zwischen Client und Server, Datagrammkopf.
//! * [`krypto`] — Ende-zu-Ende zwischen Gastgeber und Teilnehmer.
//! * [`vermittlung`] — Zustandsautomat des Servers: Ereignisse hinein,
//!   Aktionen heraus.
//!
//! # Was hier bewusst fehlt
//!
//! Kein Netz, kein `tokio`, kein `quinn`, keine Uhr außer der, die der Aufrufer
//! hereinreicht. Der Server führt die [`vermittlung::Aktion`]en aus, die
//! Fenster zeigen an, was das Modell sagt. Damit ist der ganze Ablauf prüfbar,
//! ohne einen Server zu starten und ohne Soundkarte (AGENTS.md §4).
//!
//! # Der Server liest nicht mit
//!
//! Was durch die Ströme und Datagramme geht, ist mit einem Schlüssel
//! verschlüsselt, den nur Gastgeber und Teilnehmer kennen (ADR 0012, ADR 0016
//! §5). Der Server sieht, **wer** etwas freigibt und **wie viel** fließt — nicht
//! **was**.

pub mod krypto;
pub mod protokoll;
pub mod vermittlung;

pub use protokoll::{
    Angebot, AngebotId, Art, ClientRahmen, Datagrammkopf, Fehler, Grenzen, Grund, Kanal,
    ServerRahmen, StromKopf, ALPN, MAX_UDP_NUTZLAST, VERSION,
};
pub use vermittlung::{Aktion, Ereignis, Vermittlung};
