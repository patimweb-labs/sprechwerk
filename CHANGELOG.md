# Changelog

## [3.1.1](https://github.com/patimweb-labs/sprechwerk/compare/v3.1.0...v3.1.1) (2026-11-23)


### Behoben

* **audio:** Opus auch unter eigenem Homebrew-Präfix finden ([77629f7](https://github.com/patimweb-labs/sprechwerk/commit/77629f717ca5b860ee8fd51aadb08d5ce184bf5f))
* **audio:** PipeWire-Tests nur unter Linux bauen ([a80222b](https://github.com/patimweb-labs/sprechwerk/commit/a80222bc8db8e93bd1cb48236a6069eaac04aeb4))
* **audio:** Tests und Opus-Suche unter macOS ([3537cc1](https://github.com/patimweb-labs/sprechwerk/commit/3537cc1da5d4802afd2f78fe2246654e8371d14d))

## [3.1.0](https://github.com/patimweb-labs/sprechwerk/compare/v3.0.2...v3.1.0) (2026-11-21)


### Neu

* **fenster:** Zeichen und Wortmarke in der Kopfleiste ([478897d](https://github.com/patimweb-labs/sprechwerk/commit/478897d221d697c9e7f5b5b5097637e1862b9e01))
* **marke:** eigene Pixelraster für 16, 24 und 32 px ([e2d1659](https://github.com/patimweb-labs/sprechwerk/commit/e2d1659ca810f478c9d0a003a9237566626db2b6))
* **marke:** Logo und Wortmarke aus der Vorlage, SVGs als Quelle ([a66bdaf](https://github.com/patimweb-labs/sprechwerk/commit/a66bdaf8357f12c960ca0b1194df40883351e004))
* **marke:** neues Zeichen und Logo, erzeugt mit markenwerk ([f55c5f8](https://github.com/patimweb-labs/sprechwerk/commit/f55c5f8fc9b016acc9e4cb37258258d2b70f684f))
* **thema:** Türkis als Akzent, Orange als Sprechsignal ([6e5fa29](https://github.com/patimweb-labs/sprechwerk/commit/6e5fa29390ae257495ad2b65dc75ba6a7bfd2ab1))


### Behoben

* Abschied abwarten, Verlauf nachreichen, Passwortgrund nennen ([7cedb32](https://github.com/patimweb-labs/sprechwerk/commit/7cedb32f9744ceda8a4a7a8e0334d0a89b03c751))
* Anwesenheit ist flüchtig — Neustart zeigt keine Geister ([35f4264](https://github.com/patimweb-labs/sprechwerk/commit/35f42642c3596e42addf538f02c2b5720ddc60e1))
* Anwesenheit ist flüchtig, Abschied kommt an, Passwortgrund beim Client ([dce8305](https://github.com/patimweb-labs/sprechwerk/commit/dce8305291e0c6da64c5839126e9a3d44fcc4a1d))
* **marke:** Icons ohne Zeitstempel, damit zwei Läufe gleich sind ([d5e60b5](https://github.com/patimweb-labs/sprechwerk/commit/d5e60b5821d62e02ca44409e1471fa2ebe61f652))
* **marke:** Schwänze der Blasen laufen als Treppe nach außen-unten ([9b54986](https://github.com/patimweb-labs/sprechwerk/commit/9b549864cf29abe8ae5c76a2d91db79a2cdd921e))
* **marke:** Zeichen neu erzeugt, Entwurf als Referenz ([77e1976](https://github.com/patimweb-labs/sprechwerk/commit/77e1976b60d51242812a60691c792d1fd5143c40))
* **marke:** Zeichen und Wortmarke neu — Blasen mit Treppe, lesbares W ([21b1fbd](https://github.com/patimweb-labs/sprechwerk/commit/21b1fbd6512f849015a75ea40159b3a1499d009b))
* **server:** --help startet keinen Server; rundlauf.sh baut den Client mit ([a7aff5e](https://github.com/patimweb-labs/sprechwerk/commit/a7aff5ed9a9fd90fecd4c7f8a481c3ae9a82ad28))
* **thema:** Signal im hellen Theme heller, bleibt über 3 : 1 ([0eb6313](https://github.com/patimweb-labs/sprechwerk/commit/0eb63133c6ffd753906194defc6c90fb1757bce7))


### Dokumentation

* **adr:** 0015 — kleine Fassungen haben ein lesbares Gitter, kein Einmalwerkzeug ([07911d1](https://github.com/patimweb-labs/sprechwerk/commit/07911d127ea173aac3cc26d43dac0edd2ec6d99f))
* **adr:** 0015 Logo und Farben, 0014 als ersetzt markiert ([20cddb0](https://github.com/patimweb-labs/sprechwerk/commit/20cddb059042437fca284969e4dea624a2d3b8af))
* **adr:** Flaechenfarbe auf neutral.900 begruenden ([00c3d31](https://github.com/patimweb-labs/sprechwerk/commit/00c3d310d24129d725cade870126587f1bec9776))
* **adr:** Marke in zwei Farben fuer sprechwerk (ADR 0014) ([3199003](https://github.com/patimweb-labs/sprechwerk/commit/31990032ed0c203a12cb6e163df571236cbb69e3))
* CONTRIBUTING.md, Issue-Vorlagen und PR-Vorlage ([538ca4c](https://github.com/patimweb-labs/sprechwerk/commit/538ca4c32e93f9c13fcb3c0d00cd084f7db56216))
* fremden Zusatz aus der LICENSE entfernt ([7806765](https://github.com/patimweb-labs/sprechwerk/commit/780676504a1dde12c2c3c82e773b429eda321497))
* Repo-Angaben vollständig (LICENSE, Cargo, Community-Dateien) ([7e77813](https://github.com/patimweb-labs/sprechwerk/commit/7e77813fd372157a7b03b9894259a74e28857585))
* SECURITY.md — Lücken über Private vulnerability reporting ([427d6d7](https://github.com/patimweb-labs/sprechwerk/commit/427d6d791bc907cc580b31b0dce0d0ae3d21f84f))
* Vergleichstabelle richtiggestellt ([fd56937](https://github.com/patimweb-labs/sprechwerk/commit/fd56937790b541307df31d42243373146c9ddfca))


### Bau und Pakete

* Cargo-Angaben vollständig, an einer Stelle gepflegt ([6ac85d0](https://github.com/patimweb-labs/sprechwerk/commit/6ac85d00e3fa87e9dcbde1411974cbaadce76fbe))
* toten Tauri-Verweis im Arbeitsbereich entfernen ([53fc7cb](https://github.com/patimweb-labs/sprechwerk/commit/53fc7cb61a6765375a657e911fc758da0ea6de55))

## [3.0.2](https://github.com/patimweb-labs/sprechwerk/compare/v3.0.1...v3.0.2) (2026-11-19)


### Behoben

* **audio:** je System ein Weg — cpal überall, PipeWire nur Linux ([ba6cf5e](https://github.com/patimweb-labs/sprechwerk/commit/ba6cf5e70a3aa27a972c6cce86ce3b95958951cf))
* **audio:** je System ein Weg — cpal überall, PipeWire nur Linux ([7aa60f2](https://github.com/patimweb-labs/sprechwerk/commit/7aa60f2191eab620b898ad815be214bcbf78e736))
* **flatpak:** Zweig `stable`, Kategorien eindeutig, Doku ohne Handarbeit ([059cdbe](https://github.com/patimweb-labs/sprechwerk/commit/059cdbead2fb590c00b28f52b4b7e622ff7e5239))
* **flatpak:** Zweig stable, Kategorien eindeutig, Doku ohne Handarbeit ([4938e8b](https://github.com/patimweb-labs/sprechwerk/commit/4938e8b08806d250eadd2788f61dff73b648cc63))

## [3.0.1](https://github.com/patimweb-labs/sprechwerk/compare/v3.0.0...v3.0.1) (2026-11-19)


### Behoben

* **packaging:** Server-Tar nur aus einer Quelle, Windows doppelt raus ([57bfba6](https://github.com/patimweb-labs/sprechwerk/commit/57bfba65c8dcd0b58edea3a4ac55c896f27cb479))
* **packaging:** Server-Tar nur aus einer Quelle, Windows-Server nicht doppelt ([dd3c855](https://github.com/patimweb-labs/sprechwerk/commit/dd3c855cc2efbdb9c78c11be6a9d44c77676f8bb))

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
