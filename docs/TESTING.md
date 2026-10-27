# Tests et outillage

Les tests natifs ne demandent ni navigateur ni ngspice. La cible WASM, le build
web et la recette navigateur sont vérifiés séparément.

## 1. Commandes

| Commande | Effet |
| --- | --- |
| `cargo xtask check` | format, clippy strict, tests workspace |
| `cargo xtask fmt`, `clippy`, `test` | étapes séparées |
| `cargo xtask wasm-check` | bibliothèques, worker et app pour WASM |
| `cargo xtask web-build`, `web-serve` | distribution `dist/` et serveur Trunk |
| `cargo test --workspace` | tous les tests natifs |
| `cargo test -p sim-core` | unités, netlist, DC, transitoire, AC |
| `cargo test -p editor-core` | schéma, commandes, extraction, ERC, JSON |
| `cargo test -p circuitsim-app` | document, UI, instruments, FFT, Bode, export |
| `cargo test -p worker-protocol`, `-p worker` | protocole et ordonnancement |
| `cargo test --test editor_invariants` | propriétés proptest |
| `cargo run -p sim-core --example rc_response` | exemple DC/transitoire/AC |
| `cargo run --example custom_mosfet` | composant personnalisé |
| `cargo bench --workspace` | benchmarks Criterion |
| `cargo bench --workspace --no-run` | compilation des benchmarks |
| `cargo deny check licenses` | compatibilité des licences |

`cargo xtask check` est la référence avant livraison ; exécuter
`cargo xtask wasm-check` après toute modification du cœur, du worker ou de l'app.

## 2. Prérequis

Rust stable (MSRV 1.93), `rustfmt`, `clippy`, cible `wasm32-unknown-unknown`.
Trunk `0.21.14` est requis uniquement pour `web-build`/`web-serve` ; la CI
l'installe séparément.

## 3. Suite et couverture

Au 27 octobre 2026, `cargo test --workspace` exécute **261 tests** :

| Groupe | Tests | Points clés |
| --- | ---: | --- |
| `sim-core` unités | 17 | SI, parsing, formatage, netlist, erreurs |
| `sim-core` DC | 18 | diviseur, sources, R/C/L, switch, diodes, opamp, erreurs |
| `sim-core` transitoire | 19 | RC/RL, événements, diode, opamp, énergie, session incrémentale, mise à jour à chaud |
| `sim-core` AC | 15 | RC/RL/RLC, phase, diode, opamp, balayages, erreurs |
| `editor-core` | 47 | grille, symboles, schéma, historique, ERC, extraction, JSON, séries E, snap/routage |
| `circuitsim-app` | 119 | document, vue, navigation, mini-carte, UI, inspecteur, historique, presse-papiers, simulation continue, mise à jour à chaud, calques, palettes, survol, câblage direct, raccourcis, portées incrustées, FFT et curseurs A/B, curseurs de réglage, exemples, onboarding, accessibilité, sondes, instruments, worker, FFT, Bode, CSV |
| `worker-protocol` | 11 | trames, version, netlist, erreurs, transports |
| `worker` | 13 | coalescence, cadence, révisions, DC/transitoire/AC |
| `editor_invariants` | 2 | proptest undo/redo et extraction |

Points couverts : relations de segments (contact, T, X avec/sans jonction,
chevauchement), atomicité des commandes, noms déterministes, broches hors grille,
JSON v1 historique et futur/malformé, accroche et routage orthogonal, sauts
d’historique nommé, séries E12/E24/E96 ; conventions de signe, composants non
linéaires, saturation, singularités, non-convergence, événements alignés ou non,
continuité condensateur/inductance, énergie ; réponses analytiques RC/RL/RLC,
phases, conductance dynamique, balayages ; frames tronquées/direction/version,
révisions obsolètes, rafale bornée, horloge non monotone ; catalogue, échelon,
limites UI, mesures, pic FFT, trois CSV et export UTF-8 ; insertion en série,
déplacement de segment, alignement/distribution, sélection de net et rectangle,
presse-papiers, duplication, rotation/miroir de groupe, valeurs simulées,
recherche floue, placement sur canevas, zoom/pan, cadrage de sélection,
mini-carte, dispositions, courant animé, session transitoire incrémentale,
mise à jour à chaud d'une session, démarrage live, pause/reset/pas, mapping
tension→couleur, calques et palettes, lecture au survol, câblage direct depuis
une broche, coupe de fil, raccourcis sans collision, portées incrustées avec
FFT, curseurs A/B, fusion et réordonnancement, curseurs de réglage, exemples
originaux, sondes V/A/Ω, redimensionnement d’extrémités, déplacement de segment
sans embout, fils en erreur ERC, basculement d’interrupteur, propriétés par
double-clic, debounce du rafraîchissement automatique, curseurs A/B, mesures
d’intervalle, marqueur Bode, guide de démarrage et préférences d’affichage.

Deux propriétés proptest utilisent 64 cas dans les invariants : undo/redo
restaure exactement les contenus et l’historique final ; deux extractions du même
schéma sont égales, valides, indexées et nommées de façon unique. Une propriété
`editor-core` de 64 cas vérifie l’aller-retour des séries E. Trois propriétés app
de 32 à 64 cas vérifient l’insertion en série, la géométrie relative après
collage et l’aller-retour de la mini-carte.

## 4. Reproductibilité

Les collections influençant les résultats sont ordonnées ; les tests n'utilisent
pas de graine temporelle. Les tolérances analytiques sont explicites dans
[NUMERICS.md](NUMERICS.md). FFT et CSV ont tailles/ordre déterministes. L'export
natif utilise un fichier temporaire propre au processus. Une graine proptest et
son fichier de régression doivent être conservés en cas d'échec. Les benchmarks
sont déterministes dans leurs entrées, mais leurs durées dépendent de la machine.

## 5. WASM et navigateur

`cargo xtask wasm-check` compile les bibliothèques, le protocole, le worker et
l'application. Il vérifie aussi le pont `wasm-bindgen`, `WebRunner`, `egui_plot`,
`rustfft` et le téléchargement CSV.

L'hôte `index.html` et Trunk sont livrés. La recette Chromium couvre chargement,
schéma, transitoire, export et absence d'erreur console ; elle est décrite dans
[DELIVERY.md](DELIVERY.md). Le worker dédié et le repli `postMessage` effectif
restent à livrer.

## 6. CI

Le workflow `.github/workflows/ci.yml` s'exécute sur `push`, `pull_request` et
manuellement. Il installe Rust, les composants, la cible WASM et Trunk, puis :

```text
cargo xtask check
cargo deny check licenses
cargo xtask wasm-check
cargo bench -p sim-core --bench solvers -- --quick --noplot
cargo bench -p editor-core --bench extraction -- --quick --noplot
cargo bench -p circuitsim-app --bench instruments -- --quick --noplot
cargo run -q -p sim-core --example rc_response
cargo run -q --example custom_mosfet
cargo build --release -p circuitsim-app
cargo xtask web-build
```

Le cache Cargo est partagé, les exécutions obsolètes sont annulées et `dist/` est
publié comme artefact `circuitsim-web`. Ni ngspice ni navigateur ne sont requis.

## 7. Tests à venir

- Remplacement d'un job obsolète et avertissement de résultat antérieur de bout
  en bout.
- Pas adaptatif, erreur locale, formes d'onde continues, fenêtres FFT
  alternatives.
- Références ngspice versionnées, sans installation de ngspice en CI.
- Navigateur automatisé : chargement, instruments, téléchargement, puis worker
  dédié/SAB et repli `postMessage`.
- Export PNG/SVG, chargement de projet, baseline Criterion et budgets outillés.

Chaque correction de bug ajoute un test de régression sans affaiblir les tests
existants.
