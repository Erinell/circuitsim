# Livraison et audit final

## 1. Artefacts

| Cible | Commande | Résultat |
| --- | --- | --- |
| Natif | `cargo build --release -p circuitsim-app` | `target/release/circuitsim[.exe]` |
| Web | `cargo xtask web-build` | `dist/` |
| Bibliothèques | `cargo build --release` | crates du workspace |

Sous Windows, le binaire de référence mesure environ 9,1 Mo. La distribution web
contient `index.html`, un module JS d'environ 78 ko et un wasm d'environ 5,6 Mo
après `wasm-opt`. `dist/` est ignoré par Git et publié par la CI sous l'artefact
`circuitsim-web`. Licence : `MIT OR Apache-2.0` à la racine.

## 2. Prérequis

Rust stable (MSRV 1.93), `rustfmt`, `clippy`, cible WASM, Trunk `0.21.14` pour le
Web et un navigateur récent avec WebGL/WebAssembly.

```text
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
```

## 3. Natif

```text
cargo build --release -p circuitsim-app
cargo run -p circuitsim-app
```

L'application ouvre le circuit RC de démonstration avec ses instruments. La
vérification release a contrôlé fenêtre, canevas, instruments et export.

## 4. Web

```text
cargo xtask web-build
cargo xtask web-serve
```

`web-build` appelle `trunk build --release` et produit `dist/` ; `web-serve`
écoute sur `http://127.0.0.1:8080`. Service statique équivalent :

```text
python -m http.server 8080 --directory dist
```

Le serveur doit fournir `index.html`, le JS et le wasm avec les types MIME
standards (`application/wasm` recommandé). Pour un sous-chemin :

```text
trunk build --release --public-url /circuitsim/
```

L'hôte fournit un canevas plein écran `circuitsim-canvas`, l'écran
`circuitsim-loading` retiré après démarrage, un repli sans JavaScript, une icône
SVG intégrée et un thème sombre sans requête externe.

## 5. Vérifications

| Contrôle | Résultat |
| --- | --- |
| `cargo xtask check` | format, clippy strict, 226 tests |
| `cargo deny check licenses` | licences compatibles |
| `cargo xtask wasm-check` | bibliothèques, worker, app compilés pour WASM |
| `cargo bench --workspace --no-run` | benchmarks compilés |
| Benchmarks rapides | solveurs, extraction, FFT, CSV mesurés |
| Exemples | `rc_response` et `custom_mosfet` conformes |
| Release natif | binaire construit et interface vérifiée |
| `cargo xtask web-build` | `dist/` produit |
| Chromium headless | chargement, transitoire, CSV, zéro erreur console |

Scénario navigateur : chargement de `index.html`, disparition du loading, schéma
RC et instruments, transitoire de 502 échantillons (501 pas), export et
téléchargement de `circuitsim-portee.csv`, en-tête `signal,unit,time_s,value` et
1 004 lignes de données. La recette n'est pas automatisée en CI.

## 6. Audit des contrats

- `sim-core` et `editor-core` n'utilisent ni egui ni navigateur.
- Aucun `unsafe`, `todo!()` ou `unimplemented!()` dans le workspace.
- Les solveurs revalident la netlist et ne renvoient pas de résultat partiel.
- Les événements s'appliquent après l'échantillon exact ; l'AC est petits
  signaux et l'excitation ne vient pas de `dc`.
- Résultats, traces, spectres et CSV sont ordonnés et reproductibles.
- FFT, Bode, mesures et export restent dans l'application.
- Le protocole est versionné, borné à un job et à révisions croissantes.
- Les identifiants sont monotones ; l'undo ne rembobine pas `next_id`.
- Limites UI : 100 000 pas, 10 000 points AC, 16 384 points FFT.
- `AGENTS.md` fait 150 lignes et les liens de documentation sont valides.

## 7. Risques résiduels

| Risque | Impact | Traitement |
| --- | --- | --- |
| Worker dans le fil de l'interface | analyse longue pouvant figer l'UI | protocole prêt, worker dédié/SAB à brancher |
| Recette navigateur manuelle | régression web non bloquante | scénario Chromium documenté |
| Budgets non bloquants | dérive perf non détectée | budgets documentés, revue manuelle |
| Transitoire sans pas adaptatif | erreur locale non contrôlée | Euler arrière assumé |
| Pas de persistance/PNG | reprise/partage limités | CSV livré, extension à planifier |
| Custom non simulé | extension incomplète | refus explicite, procédure documentée |

Ces points sont des limites de périmètre, pas des stubs.

## 8. Reproduction et conclusion

```text
cargo xtask check
cargo xtask wasm-check
cargo xtask web-build
cargo run -p sim-core --example rc_response
cargo run --example custom_mosfet
```

La CI exécute ces contrôles, les benchmarks rapides, les builds natif/web et
publie `dist/`. Les dix étapes sont livrées : cœur analogique, éditeur,
protocole worker, interface, instruments, documentation, exemples, CI,
benchmarks, audit et distribution native/web. Les contrôles locaux passent sans
avertissement ; les extensions restantes sont identifiées sans modification des
contrats publics.
