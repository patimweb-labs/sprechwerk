# Mitentwickeln

Die verbindlichen Regeln stehen in [`AGENTS.md`](AGENTS.md) — für Menschen wie
für Werkzeuge. Hier das, was man zum ersten Pull Request braucht.

## Weg

1. `git switch -c fix/kurzbeschreibung origin/master`
2. ändern, prüfen, committen
3. `git push -u origin HEAD`, `gh pr create`
4. CI abwarten (dieselben Schritte wie unten), mergen

`master` ist geschützt: kein direkter Push, kein Force-Push, kein Löschen.
Reviews sind nicht erzwungen (Solo-Betrieb), der Pull Request ist es.

## Commits

Conventional Commits, ein Gedanke pro Commit, deutschsprachige Betreffzeile
erlaubt: `fix: Sperrliste hebt eine Adresssperre wieder auf`. Releases macht
`release-please` aus diesen Zeilen — `feat:`, `fix:`, `perf:`, `refactor:`,
`docs:`, `build:`, `ci:`, `test:`.

## Prüfen

Das ist der Befehl vor jedem Commit:

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace
./scripts/pruefen.sh   # dasselbe + Bilder + Sprachproben
```

`cargo test` **mit** `--workspace`: ohne das prüft Cargo nur das Wurzelpaket
und meldet „0 passed".

## Was zu einem PR gehört

Die Liste steht in [`AGENTS.md`](AGENTS.md) §5 und noch einmal in der
[Vorlage](.github/pull_request_template.md). Der kurze Kern: Test dabei
(ein Fix ohne Test ist ein Fix auf Probe), `clippy -D warnings` sauber, alle
drei Farbschemata geprüft, Tastaturbedienung und Fokus sichtbar, keine
Hex-Werte und keine Pixelmaße im Code — nur Tokens (AGENTS.md §2.6), keine neue
Abhängigkeit ohne Begründung, Dokumentation nachgezogen.

## Sicherheitslücken

Bitte **nicht** als Issue, sondern über GitHub „Private vulnerability
reporting" — siehe [`SECURITY.md`](SECURITY.md).
