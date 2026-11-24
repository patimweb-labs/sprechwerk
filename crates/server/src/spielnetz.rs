//! Der Anschluss des Spielnetzes an den Server (ADR 0016 §6).
//!
//! Das Modul (`sprechwerk_spielnetz`) kennt weder `Kern` noch `Anwendung`. Es
//! fragt über [`Mitgliedschaft`] nach einer **Momentaufnahme**: wer ist in der
//! Sitzung angemeldet, in welchem Raum, und darf er das Modul benutzen.
//!
//! Damit ist die Grenze eingehalten und trotzdem eindeutig: die Sitzung bleibt
//! die einzige Wahrheit über Räume und Rechte. Ein Client kann sich nicht in
//! eine fremde Freigabe hineinschreiben, weil der Raum nie im Rahmen steht.
//!
//! # Was hier **nicht** passiert
//!
//! Kein Umbau des Kerns. Diese Datei liest nur: `teilnehmer` (wer ist
//! verbunden), `state` (Kennung, Name, Raum, Rolle) und `darf`. Geschrieben
//! wird ausschließlich im Modul.

use std::sync::{Arc, Mutex};

use sprechwerk_domain::{Capability, RoomId};
use sprechwerk_spielnetz::{Mitgliedschaft, Stand};

use crate::kern::Kern;

/// Die Momentaufnahme für das Spielnetz.
pub struct Sitzung {
    kern: Arc<Mutex<Kern>>,
}

impl Sitzung {
    pub fn neu(kern: Arc<Mutex<Kern>>) -> Arc<Self> {
        Arc::new(Self { kern })
    }
}

impl Mitgliedschaft for Sitzung {
    fn stand(&self) -> Vec<Stand> {
        // Kurz sperren, kopieren, freigeben. Die Sperre wird nie über ein
        // `await` gehalten — `stand()` ist synchron und kehrt sofort zurück.
        let k = self.kern.lock().expect("Sperre");
        k.teilnehmer
            .keys()
            .filter_map(|id| {
                let mitglied = k.state.mitglied(*id)?;
                Some(Stand {
                    kennung: mitglied.schluessel.clone(),
                    name: mitglied.name.clone(),
                    raum: mitglied.raum.map(raum_text),
                    darf: sprechwerk_domain::role::darf(mitglied.rolle, Capability::Spielnetz),
                })
            })
            .collect()
    }
}

/// Ein Raum als Text — die Kennung, nicht der Name: Namen ändern sich, und das
/// Modul soll beim Umbenennen nicht alle Verbindungen neu bewerten.
fn raum_text(id: RoomId) -> String {
    id.to_string()
}
