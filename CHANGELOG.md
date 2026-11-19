# Changelog

## [3.0.0](https://github.com/patimweb-labs/sprechwerk/compare/v2.0.3...v3.0.0) (2026-11-19)


### ⚠ BREAKING CHANGES

* Das Paket `sprechwerk` enthält nicht mehr den Server; der liegt in `sprechwerk-server`. Das Serverarchiv enthält nicht mehr `sprechwerk-cli`, dafür `sprechwerk-server-fenster`. Die Server-App hat die Kennung de.weppelmann.Sprechwerk.Server und ein eigenes Flatpak.

### Neu

* Client und Server sind getrennte Anwendungen ([f7dca78](https://github.com/patimweb-labs/sprechwerk/commit/f7dca782a683bd3070947539e763ca1d78618458))

## [2.0.3](https://github.com/patimweb-labs/sprechwerk/compare/v2.0.2...v2.0.3) (2026-11-19)


### Bau und Pakete

* Container-Abbild nach Docker Hub ([51828d9](https://github.com/patimweb-labs/sprechwerk/commit/51828d95b7c0a5ef4036e158420a74e646cc597d))
* Container-Abbild nach Docker Hub statt ghcr.io ([51c235c](https://github.com/patimweb-labs/sprechwerk/commit/51c235c81000a5a7ad7def459ee0520ed1818b83))

## [2.0.2](https://github.com/patimweb-labs/sprechwerk/compare/v2.0.1...v2.0.2) (2026-11-19)


### Behoben

* **flatpak:** AppStream ohne rofiles-fuse ([fe204b6](https://github.com/patimweb-labs/sprechwerk/commit/fe204b6973a74df2831ad4a3ce07f031d381f5a3))
* **flatpak:** AppStream ohne rofiles-fuse, sonst bricht der Flatpak-Bau ab ([4fa4acb](https://github.com/patimweb-labs/sprechwerk/commit/4fa4acb45184229edf995508d0d9cb8b1ef323c1))
* **flatpak:** kein SVG im Flatpak, AppStream 1.0 kann es nicht lesen ([0959dbc](https://github.com/patimweb-labs/sprechwerk/commit/0959dbc078aefb5831dfd91ad79baa46b4d0c661))

## [2.0.1](https://github.com/patimweb-labs/sprechwerk/compare/v2.0.0...v2.0.1) (2026-11-18)


### Behoben

* **packaging:** deb/rpm und MSI bauen wieder ([e535d90](https://github.com/patimweb-labs/sprechwerk/commit/e535d9057c80fc9daec65fc19b0b0b8242ae7f80))
* **packaging:** deb/rpm und MSI bauen wieder ([c65fe40](https://github.com/patimweb-labs/sprechwerk/commit/c65fe404c0bfc9bce7e51c87157bc2c8f21b3722))

## [2.0.0](https://github.com/patimweb-labs/sprechwerk/compare/v1.3.0...v2.0.0) (2026-11-18)


### ⚠ BREAKING CHANGES

* Die Verwaltung im Browser gibt es nicht mehr (sprechwerk stand/rolle/protokoll oder sprechwerk-server-fenster); das Fensterprogramm heißt sprechwerk-fenster statt sprechwerk-desktop.

### Neu

* **fenster:** Bedienung wie TeamSpeak ([8f542e5](https://github.com/patimweb-labs/sprechwerk/commit/8f542e5d47a9fd7860012c5a340e800bf0aee81f))
* **fenster:** Fensteranwendung in reinem Rust (egui) ([c4978e2](https://github.com/patimweb-labs/sprechwerk/commit/c4978e25dc758138950dff91fac4102c8ce9e8b0))
* **fenster:** Sprechlicht in fester Spalte rechts neben dem Namen ([581c522](https://github.com/patimweb-labs/sprechwerk/commit/581c522ae6681ea551659ccef9fadbeed495743b))
* **fenster:** Sprechlicht vor dem Namen statt Pegelbalken im Kopf ([162a4a6](https://github.com/patimweb-labs/sprechwerk/commit/162a4a62954e4506531bc7b8312d040fdaa30802))
* Server-Fenster und Einstellungen einheitlich und schmal tauglich ([640f359](https://github.com/patimweb-labs/sprechwerk/commit/640f3591d26467abbc896134a82450ad816ebb14))
* **server:** Verwaltung auf der Konsole und als Fenster statt im Browser ([bab010f](https://github.com/patimweb-labs/sprechwerk/commit/bab010fe033992e9b681df53301377e707753e6d))


### Behoben

* **fenster:** Gerätenamen wie im System, Hand-Zeiger, nichts verschwindet beim Verkleinern ([18dacff](https://github.com/patimweb-labs/sprechwerk/commit/18dacff4d33ace8d6a944f92486e23cf3fe6f755))
* **fenster:** Icons zurück, Chat einklappbar, Kopfleiste ohne Überlappung; Effekte entfernt ([690698c](https://github.com/patimweb-labs/sprechwerk/commit/690698c75a23b854dcecac4d26639f21ce47d3a1))
* Scrollbalken am Fensterrand statt einen Rand weit innen ([58a43ab](https://github.com/patimweb-labs/sprechwerk/commit/58a43aba317de61ed8e92bec918fd4a57870d670))


### Umgebaut

* Bausatz crates/oberflaeche für beide Fenster ([0b9d577](https://github.com/patimweb-labs/sprechwerk/commit/0b9d5778a17a6e8f58c3e22b1c0659849a76f0f4))
* Client-Kern ohne Oberfläche in crates/client ([513dbc8](https://github.com/patimweb-labs/sprechwerk/commit/513dbc8f8a486e84680f761176a50c908392a050))


### Dokumentation

* ADR 0013 (alles in Rust), README, SPEC, AGENTS und Bilder nachgezogen ([8e0fa6f](https://github.com/patimweb-labs/sprechwerk/commit/8e0fa6f644168a0edc18a2aceca872691e95fcf2))


### Bau und Pakete

* Web-Oberfläche und Tauri entfernt, Pakete aus reinem Rust ([737c66d](https://github.com/patimweb-labs/sprechwerk/commit/737c66d2b655710cb0e335660f6ef8fc7358d0ad))

## [1.3.0](https://github.com/patimweb-labs/sprechwerk/compare/v1.2.0...v1.3.0) (2026-11-17)


### Neu

* Chat mit Reaktionen, Effekten und Link-Vorschau (ADR 0012 Stufe 2) ([87e566f](https://github.com/patimweb-labs/sprechwerk/commit/87e566f206cb913ddb4afd762481af8e3a51ae0a))
* **client:** Reaktionen, Effekte und Vorschau in Fenster und CLI ([1a10c43](https://github.com/patimweb-labs/sprechwerk/commit/1a10c435b4c5299e63461a0e8a629b8a05d92505))
* **ipc:** Inhalte prüfen und für die Oberfläche abbilden ([06c0671](https://github.com/patimweb-labs/sprechwerk/commit/06c067109300c63c09db8d8c9748661148c2f24f))
* Reaktionen, Effekte und Link-Vorschau im Inhalt ([70e8200](https://github.com/patimweb-labs/sprechwerk/commit/70e8200149887c1c42fb38e91b1591f10967ae58))
* **ui:** Chat mit Beiträgen, Reaktionen und Effektbühne ([54490e9](https://github.com/patimweb-labs/sprechwerk/commit/54490e9f6d3da64100145af4547a0e95b6663ac6))


### Dokumentation

* ADR 0012 Stufe 2, SPEC und neue Bilder ([30cc7c8](https://github.com/patimweb-labs/sprechwerk/commit/30cc7c80c26440492deb8ad9c3b3e832307d6e7c))
* README komplett überarbeitet ([d9538b1](https://github.com/patimweb-labs/sprechwerk/commit/d9538b145ce866607040c3e640161adbdb5ece67))


### Bau und Pakete

* Sperrdateien auf 1.2.0 nachziehen ([0e403d3](https://github.com/patimweb-labs/sprechwerk/commit/0e403d35cae9ca450270df0680ea77aa286535e8))

## [1.2.0](https://github.com/patimweb-labs/sprechwerk/compare/v1.1.0...v1.2.0) (2026-11-17)


### Neu

* **client:** Fenster und Referenzclient folgen der neuen Rollen- und Schlüssellage ([aac6823](https://github.com/patimweb-labs/sprechwerk/commit/aac6823442a9ad59a030be3df7c7cbce164463a5))
* **domain:** Rolle durch den Betreiber setzen, letzter Administrator bleibt ([b44092d](https://github.com/patimweb-labs/sprechwerk/commit/b44092d6b818e1251eb6f2d38955580f324e0a58))
* **server:** Verwaltung im Browser und Betrieb ohne Docker ([992ff06](https://github.com/patimweb-labs/sprechwerk/commit/992ff067945165315eef61dbac5a0ad89ec7a0dd))
* Text und Anhänge als Geheimtext im Protokoll ([71ad9c7](https://github.com/patimweb-labs/sprechwerk/commit/71ad9c70706035e86e3c5e15719af4dc03a30134))
* **ui:** Verwaltungsseite im Browser und Sprechtaste ([ddfedea](https://github.com/patimweb-labs/sprechwerk/commit/ddfedeaede80bada3a285dbb947f41ff15937cc2))
* Verwaltung im Browser, Ende-zu-Ende für Text und Anhänge ([9dfef08](https://github.com/patimweb-labs/sprechwerk/commit/9dfef083da96668b53e2977cf9792b4a00526870))


### Dokumentation

* ADRs 0011/0012, SPEC, README und Skripte nachgezogen ([acdd278](https://github.com/patimweb-labs/sprechwerk/commit/acdd278682e7a7de4680bf355a8ff5d33399a656))

## [1.1.0](https://github.com/patimweb-labs/sprechwerk/compare/v1.0.0...v1.1.0) (2026-11-16)


### Neu

* Ton ohne pactl — Opus und Geräte auf Windows und macOS ([cb86d15](https://github.com/patimweb-labs/sprechwerk/commit/cb86d158eaeab241eeb3cd2e5e262301129a055c))


### Dokumentation

* README, AUDIO und ADR 0007 für alle vier Paketwege ([783eba2](https://github.com/patimweb-labs/sprechwerk/commit/783eba2174078d7ba768395b3109963da99cd5ff))


### Bau und Pakete

* Pakete für Linux, Flatpak, Windows und macOS ([7f6d6c3](https://github.com/patimweb-labs/sprechwerk/commit/7f6d6c38cdfdef4e058b64eba33c23820fe0e3e4))

## 1.0.0 (2026-11-16)


### Neu

* Fenster zeigt den Fingerabdruck und heftet ihn an (TOFU) ([fcaea0f](https://github.com/patimweb-labs/sprechwerk/commit/fcaea0f1928ef06fd26155f88c57386ac2e3dd8d))
* quantensicherer Schlüsselaustausch, Fingerabdruck im Protokoll ([45bcfe3](https://github.com/patimweb-labs/sprechwerk/commit/45bcfe3fa7f7fcda53f6bbc25e3259c81a71a363))


### Umgebaut

* der Echopuffer gehört den Threads, nicht dem Pfad ([e16f8aa](https://github.com/patimweb-labs/sprechwerk/commit/e16f8aaa1cca25c231313ee775b2485068529bd6))


### Dokumentation

* README und ADR 0007 beschreiben die Releases ([438f023](https://github.com/patimweb-labs/sprechwerk/commit/438f0234accbd4b8f0ec3990147b5cddd390758c))


### Bau und Pakete

* Docker, CI und Prüfskript auf den neuen Stand ([c509941](https://github.com/patimweb-labs/sprechwerk/commit/c5099417ff70028b7ab57eddf6dafe525a4e6176))
* Release-Pipeline mit release-please, Paketen und Container ([b366788](https://github.com/patimweb-labs/sprechwerk/commit/b3667889f0c51ac809e3e6384f53085c99f76567))
