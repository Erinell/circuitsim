# CircuitSim

CircuitSim est un simulateur de circuits analogiques en temps réel pour le
navigateur et le poste de travail. Le cœur de calcul, l'éditeur et l'interface
sont séparés, testables sans navigateur et compilables pour
`wasm32-unknown-unknown`.

## État

Version **0.3.0**. Les dix étapes du projet et les lots G1 à G8 sont livrés :
234 tests natifs, clippy strict, cible WASM, benchmarks, exemples, CI et
distribution web.

- **Cœur** : unités SI, netlist validée, MNA dense, DC, transitoire à pas fixe,
  AC petits signaux, Newton avec continuations, audit d'énergie.
- **Éditeur** : grille entière, symboles, fils orthogonaux, jonctions, labels,
  undo/redo nommé, sélection rectangle, presse-papiers, ERC, extraction
  déterministe, séries E, JSON versionné.
- **Interface** : egui native et WASM, fenêtre maximisée, palette, canevas,
  inspecteur avec valeurs simulées, courant animé, sondes V/A/Ω, transitoire
  temps réel avec pause et vitesse, curseurs A/B, marqueur Bode, menus
  contextuels, mini-carte, dispositions fiables, guide de démarrage, contraste
  élevé et taille réglable, portée, mesures, FFT de Hann, Bode, export CSV,
  échelon de source.
- **Transport** : protocole binaire `CSIM`, worker borné à un job, coalescence
  à 60 Hz, révisions croissantes, pont WASM.

L'analyse s'exécute encore dans le fil de l'interface ; le worker dédié du
navigateur et `SharedArrayBuffer` restent à brancher. Aucune fonctionnalité
absente n'est simulée par un bouton ou un résultat factice.

## Démarrage rapide

Prérequis : Rust stable (MSRV 1.93), `rustfmt`, `clippy`, cible WASM et, pour le
Web, Trunk `0.21.14`.

```text
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
cargo xtask check
cargo xtask wasm-check
cargo run -p circuitsim-app
```

Distribution web :

```text
cargo xtask web-build
cargo xtask web-serve
```

`web-build` produit `dist/` ; `web-serve` écoute sur `http://127.0.0.1:8080`.
Les artefacts et le déploiement sont détaillés dans
[DELIVERY.md](docs/DELIVERY.md).

## Commandes utiles

```text
cargo test --workspace
cargo test -p sim-core
cargo test -p editor-core
cargo test -p circuitsim-app
cargo test -p worker-protocol
cargo test -p worker
cargo run -p sim-core --example rc_response
cargo run --example custom_mosfet
cargo bench --workspace --no-run
cargo deny check licenses
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

## Structure

| Chemin | Rôle |
| --- | --- |
| `crates/sim-core` | unités, netlist, MNA, DC, transitoire, AC |
| `crates/editor-core` | schéma, commandes, undo/redo, ERC, extraction |
| `crates/protocol`, `crates/worker` | protocole binaire, ordonnancement, WASM |
| `crates/app` | interface egui, instruments, FFT, Bode, export |
| `crates/*/benches`, `examples/` | benchmarks Criterion et exemples exécutables |
| `index.html`, `Trunk.toml` | hôte web et distribution statique |
| `tests/`, `src/lib.rs` | tests d'intégration et propriétés proptest |
| `.github/workflows/ci.yml`, `xtask/` | CI et automatisation |

## Documentation

| Document | Contenu |
| --- | --- |
| [Architecture](docs/ARCHITECTURE.md) | crates, dépendances, flux, déterminisme |
| [Décisions](docs/DECISIONS.md) | arbitrages structurants et limites assumées |
| [Unités et netlist](docs/NETLIST.md) | SI, parsing, modèle, validation |
| [Éditeur](docs/EDITOR.md) | grille, connectivité, noms, undo, ERC, JSON |
| [Composants](docs/COMPONENTS.md) | paramètres, modèles DC/transitoire/AC |
| [Simulation](docs/SIMULATION.md) | API, options, résultats, erreurs |
| [Numérique](docs/NUMERICS.md) | équations MNA, Newton, LU, AC |
| [Protocole](docs/PROTOCOL.md) | trames, worker, WASM, transports |
| [Interface](docs/UI.md) | disposition, interactions, raccourcis |
| [Ergonomie](docs/UX.md) | principes, gestes, palettes et critères |
| [Instruments](docs/INSTRUMENTS.md) | portée, mesures, FFT, Bode, CSV |
| [Tests](docs/TESTING.md) | commandes, couverture, CI, reproductibilité |
| [Performances](docs/PERFORMANCE.md) | charges, mesures, budgets |
| [Exemples](docs/EXAMPLES.md) | RC, composant personnalisé, benchmarks |
| [Livraison](docs/DELIVERY.md) | artefacts, Web, audit, vérifications, risques |
| [Git](docs/GIT_WORKFLOW.md) | branches, commits, tags et réglages GitHub |
| [Licences](docs/LICENSING.md) | licence du projet et politique des dépendances |

## Principes et limites

- `sim-core` et `editor-core` n'utilisent ni egui, ni navigateur, ni horloge
  globale ; `egui_plot` et `rustfft` restent dans l'application.
- Une simulation exige une netlist validée avec une seule masse nommée `0` ; les
  résultats sont déterministes et ne contiennent jamais de solution partielle.
- Les événements transitoires s'appliquent après l'échantillon à leur instant
  exact ; l'AC est petits signaux autour du DC.
- Limites principales : pas adaptatif, AC sans capacité de jonction, opamp sans
  alimentation ni pôle, FFT sans fenêtres alternatives, pas de persistance/PNG.
- Worker dans le fil de l'interface, tests navigateur non automatisés et budgets
  de performance non bloquants.

## Licence

Distribué sous `MIT OR Apache-2.0`. Textes : [LICENSE-MIT](LICENSE-MIT) et
[LICENSE-APACHE](LICENSE-APACHE). Toute contribution est soumise aux mêmes
termes. Copyright © 2026 Cédric Debois. Vérification :
`cargo deny check licenses` ; politique : [docs/LICENSING.md](docs/LICENSING.md).
