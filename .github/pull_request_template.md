## Was

Kurz, was dieser PR ändert — und warum.

## Prüfen

- [ ] Tests grün (Unit, Integration, Vertrag, Rundlauf)
- [ ] `cargo clippy --all-targets -- -D warnings` sauber (hier gibt es kein `vue-tsc`, ADR 0013)
- [ ] alle drei Themes geprüft (dark, light, mono)
- [ ] Tastaturbedienung geprüft, Fokus sichtbar, Rollen und Namen gesetzt
- [ ] Screenshot im PR (vorher/nachher bei UI-Änderungen)
- [ ] keine Tokens umgangen, keine Hex-Werte, keine neuen Abhängigkeiten ohne Begründung
- [ ] Dokumentation angepasst: `README.md`, `docs/`, `SPEC.md`
- [ ] Latenz nicht schlechter: `scripts/latenz.sh` vor und nach der Änderung
- [ ] Cheats dieses PRs als Ticket vermerkt

Der Befehl vor dem Commit:

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace
```

Regeln: [`AGENTS.md`](AGENTS.md) · Ablauf: [`CONTRIBUTING.md`](CONTRIBUTING.md) ·
Sicherheitslücken: [`SECURITY.md`](SECURITY.md).
