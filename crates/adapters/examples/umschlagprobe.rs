//! Was kostet der hybride Umschlag? (ADR 0010, Schritt 2)
//!
//! Aufruf:
//!
//! ```bash
//! cargo run --release --example umschlagprobe -p sprechwerk-adapters
//! ```
//!
//! Gemessen wird das, was beim Verteilen des Raumschlüssels tatsächlich
//! anfällt: je Mitglied ein Umschlag, plus das einmalige Erzeugen der eigenen
//! Sitzung. Die Zahlen gehören in `docs/adr/0010-quantensicherheit.md`, nicht in
//! ein Gefühl.

use sprechwerk_adapters::verschluesselt::{
    verpacken_hybrid, Austausch, FremderAustausch, Raumschluessel, AUSTAUSCH_LEN,
    HYBRIDER_UMSCHLAG_LEN,
};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    const MITGLIEDER: usize = 50;
    const LAEUFE: usize = 20;

    println!("Mitglieder je Verteilung: {MITGLIEDER}");
    println!(
        "veröffentlicht je Mitglied: {AUSTAUSCH_LEN} Byte ({} Zeichen hex)",
        AUSTAUSCH_LEN * 2
    );
    println!("Umschlag je Mitglied:      {HYBRIDER_UMSCHLAG_LEN} Byte");

    let start = Instant::now();
    for _ in 0..LAEUFE {
        let _ = Austausch::neu()?;
    }
    let sitzung = start.elapsed() / LAEUFE as u32;
    println!("eigene Sitzung erzeugen:   {sitzung:?}");

    let sitzungen: Vec<Austausch> = (0..MITGLIEDER)
        .map(|_| Austausch::neu())
        .collect::<anyhow::Result<_>>()?;
    let oeffentlich: Vec<FremderAustausch> = sitzungen
        .iter()
        .map(|a| FremderAustausch::aus_hex(&a.oeffentlich()))
        .collect::<anyhow::Result<_>>()?;

    let mut verpacken = Vec::new();
    let mut oeffnen = Vec::new();
    for epoche in 0..LAEUFE as u32 {
        let schluessel = Raumschluessel::neu(epoche);

        let start = Instant::now();
        let umschlaege: Vec<_> = oeffentlich
            .iter()
            .map(|f| verpacken_hybrid(f, "raum", &schluessel))
            .collect::<anyhow::Result<_>>()?;
        verpacken.push(start.elapsed());

        let start = Instant::now();
        for (sitzung, umschlag) in sitzungen.iter().zip(umschlaege.iter()) {
            sitzung.oeffnen(umschlag, "raum", epoche)?;
        }
        oeffnen.push(start.elapsed());
    }
    verpacken.sort();
    oeffnen.sort();
    println!(
        "verpacken für {MITGLIEDER}: p50 {:?}, p95 {:?} ({} Byte je Umschlag)",
        verpacken[verpacken.len() / 2],
        verpacken[((verpacken.len() - 1) as f64 * 0.95) as usize],
        HYBRIDER_UMSCHLAG_LEN
    );
    println!(
        "öffnen für {MITGLIEDER}:    p50 {:?}, p95 {:?}",
        oeffnen[oeffnen.len() / 2],
        oeffnen[((oeffnen.len() - 1) as f64 * 0.95) as usize]
    );
    Ok(())
}
