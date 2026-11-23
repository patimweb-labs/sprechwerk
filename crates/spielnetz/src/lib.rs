//! Spielnetz: Spiele im Raum freigeben (ADR 0016).
//!
//! # Drei Bausteine, alle ohne I/O
//!
//! * [`protokoll`] — Rahmen zwischen Client und Server, Datagrammkopf.
//! * [`krypto`] — Ende-zu-Ende zwischen Gastgeber und Teilnehmer.
//! * [`vermittlung`] — Zustandsautomat des Servers: Ereignisse hinein,
//!   Aktionen heraus.
//!
//! Dazu seit P2 zwei dünne Netzschichten, die nur die Aktionen ausführen und
//! die Verbindung führen:
//!
//! * [`server`] — nimmt die Spielnetz-Verbindungen an, prüft die Anmeldung und
//!   leitet Ströme und Datagramme weiter.
//! * [`client`] — baut die zweite QUIC-Verbindung auf und meldet sich an.
//!
//! # Was hier bewusst fehlt
//!
//! Weder `server` noch `client` kennen Räume, Rollen oder den Sitzungszustand
//! des Servers. Der Server beantwortet eine Momentaufnahme ([`server::Stand`])
//! und alles Weitere entscheidet die [`vermittlung::Vermittlung`]. Damit ist der
//! ganze Ablauf prüfbar, ohne einen Server zu starten und ohne Soundkarte
//! (AGENTS.md §4).
//!
//! # Der Server liest nicht mit
//!
//! Was durch die Ströme und Datagramme geht, ist mit einem Schlüssel
//! verschlüsselt, den nur Gastgeber und Teilnehmer kennen (ADR 0012, ADR 0016
//! §5). Der Server sieht, **wer** etwas freigibt und **wie viel** fließt — nicht
//! **was**.

pub mod client;
pub mod krypto;
pub mod protokoll;
pub mod server;
pub mod vermittlung;

pub use client::{anmelden, Anmeldefehler, Steuerung};
pub use protokoll::{
    Angebot, AngebotId, Art, ClientRahmen, Datagrammkopf, Fehler, Grenzen, Grund, Kanal,
    ServerRahmen, StromKopf, ALPN, MAX_UDP_NUTZLAST, VERSION,
};
pub use server::{Mitgliedschaft, Modul, Stand};
pub use vermittlung::{Aktion, Ereignis, Vermittlung};
