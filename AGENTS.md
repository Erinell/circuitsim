# AGENTS.md

## Mission

CircuitSim est un simulateur de circuits analogiques en temps réel, natif et
WASM. Le cœur de simulation, l'éditeur et l'interface restent séparés. La
livraison suit dix étapes ; chaque étape doit être compilable, testée,
documentée et accompagnée d'un rapport.

## Règles non négociables

- Rust stable, édition 2021, `wasm32-unknown-unknown`.
- Aucun `unsafe` (le workspace l'interdit).
- Aucun `todo!()`, `unimplemented!()`, pseudo-code, placeholder, code commenté
  ou fonction vide.
- Code et API en anglais ; interface et documentation en français.
- Commentaires de documentation publics courts (1 à 3 lignes). Les explications
  détaillées vont dans `docs/`.
- Validation explicite aux frontières ; erreurs typées ; pas de panique sur une
  entrée utilisateur.
- Résultats déterministes : collections ordonnées, tri stable, noms reproductibles.
- `sim-core` et `editor-core` ne dépendent ni d'egui, ni du navigateur, ni d'une
  horloge globale.
- Les tests natifs ne requièrent ni navigateur ni ngspice.
- Toute correction de bug ajoute un test de régression.

## Commandes

```text
cargo xtask check        # fmt check + clippy strict + tests
cargo xtask fmt
cargo xtask wasm-check   # cœurs, worker et app pour wasm32
cargo xtask web-build    # distribution web Trunk
cargo bench --workspace --no-run
```

Avant de déclarer une étape terminée : `cargo xtask check` et, si le cœur a
changé, `cargo xtask wasm-check`. Aucun avertissement n'est toléré.

## Arborescence

- `crates/sim-core` : unités SI, netlist, validation, MNA, DC, transitoire et AC.
- `crates/editor-core` : schéma, commandes, undo/redo, ERC, extraction, JSON.
- `crates/protocol` et `crates/worker` : protocole binaire, ordonnancement, WASM.
- `crates/app` : application egui/eframe, instruments et export ; hôte `index.html`.
- `crates/*/benches` et `examples/` : benchmarks Criterion et exemples exécutables.
- `crates/sim-core/tests/`, `src/lib.rs` et `tests/editor_invariants.rs` : tests natifs et proptest.
- `xtask` : automatisation ; `.github/workflows/ci.yml` : CI ; `docs/` : architecture,
  décisions, éditeur, netlist, simulation, numérique, composants, protocole,
  tests, UI, instruments, exemples et performances.

## Contrats de données

- `NodeId` et `ComponentId` égalent l'indice dans leur vecteur validé.
- `Netlist::validate` accepte zéro ou plusieurs masses.
- `Netlist::validate_for_simulation` exige exactement une masse nommée `0`.
- `ComponentKind::Ground` doit pointer vers un nœud `NodeKind::Ground`.
- Les nombres de paramètres sont finis et stockés en SI.
- Les noms font au plus 128 caractères, sans caractère de contrôle.
- `NetName` coupe les espaces extérieurs ; les labels dupliqués sont renommés
  avec `#2`, `#3`, etc.
- `Schematic` utilise un compteur d'identifiants monotone partagé.
- L'undo restaure les objets complets mais ne rembobine pas `next_id`.
- Les champs `pub(crate)` du schéma ne sont pas modifiés hors de `editor-core`.
- Le JSON de schéma est versionné (`SCHEMA_VERSION = 1`) et revalidé au chargement.

## Contrats de simulation

- `solve_dc`, `simulate_transient` et `solve_ac` exigent une netlist validée et
  revalident.
- Le courant de branche est orienté `a -> b` ; une source de courant `dc`
  circule de `a` vers `b` à travers la source.
- Le commutateur est linéaire : `ron` si `closed`, sinon `roff`.
- Diode et LED utilisent Shockley ; le Zener ajoute le claquage `bv`, `ibv`, `nbv`.
- `OpAmp` est un VCVS à gain fini et saturation ; `Custom` reste refusé.
- Le transitoire utilise Euler arrière à pas fixe. Un événement source ou
  commutateur s'applique après l'échantillon à son instant exact.
- L'AC est petits signaux autour du DC ; `AcExcitation` porte l'amplitude crête
  et la phase. Une source absente a un phasor nul ; le `dc` n'est pas une amplitude.
- Les fréquences AC sont finies, positives et ordonnées ; un balayage explicite
  est strictement croissant, les bornes générées sont exactes.
- L'audit d'énergie utilise la puissance de fin de pas ; il est diagnostique.
- Les résultats sont des `BTreeMap` ; une erreur ne produit pas de solution
  partielle présentée comme valide.

## Contrats de connectivité

- Grille entière, bornes `-1_000_000..=1_000_000`.
- Fils orthogonaux, longueur strictement positive, extrémités normalisées.
- Contact colinéaire : connexion.
- T (extrémité sur un autre fil) : connexion sans jonction.
- Croisement X intérieur : connexion uniquement avec `Junction`.
- Chevauchement colinéaire : connexion avec ERC `WireOverlap`.
- Un label ne connecte que les conducteurs présents à sa position.
- Un groupe sans broche ne produit pas de net simulable.
- Noms : masse `0`, sinon plus petit label, sinon `N$001`, `N$002`, etc.
- L'ERC est un rapport ; l'extraction ne s'arrête que sur une erreur
  structurelle (`ExtractError`).

## Stack cible

Versions approuvées : eframe/egui `0.35.0`, egui_plot `0.36.0`, faer `0.24.4`
sans rayon, nalgebra `0.35.0`, wasm-bindgen `0.2.129`, web-sys/js-sys `0.3.106`,
postcard, rustfft `6.4.1`, proptest, criterion `0.8.2`, Trunk `0.21.14`. Ne pas changer ces
versions sans mettre à jour la documentation et la CI.

Le MOSFET est livré comme exemple d'extension (`examples/custom_mosfet.rs`),
pas comme symbole intégré. Le registre accepte les types personnalisés.

## Workflow d'étape

1. Lire l'état courant, `AGENTS.md` et les documents concernés.
2. Implémenter sans stub, avec validation et tests.
3. Exécuter `cargo fmt --all`, `cargo xtask check`, puis `wasm-check` si utile.
4. Mettre à jour les documents concernés, le README et le tableau ci-dessous.
5. Vérifier `AGENTS.md` (150 lignes maximum) et produire un rapport court :
   fichiers, tests, risques, prochaine étape.

## État des étapes

| Étape | Contenu | État |
| --- | --- | --- |
| 1 | cadrage, architecture, outillage | terminé |
| 2 | fondations : unités, netlist, éditeur, ERC, tests | terminé |
| 3 | noyau numérique et simulation DC | terminé |
| 4 | transitoire, événements, Newton robuste, énergie | terminé |
| 5 | AC et analyses fréquentielles | terminé |
| 6 | protocole worker et WASM | terminé |
| 7 | interface egui : palette, canevas, propriétés | terminé |
| 8 | scopes, mesures, FFT, Bode, export | terminé |
| 9 | documentation complète, exemples, CI, benchmarks | terminé |
| 10 | audit final, performance et livraison | terminé |

## Documentation

Livrés : `README.md` et `docs/` (architecture, décisions, éditeur, netlist,
simulation, numérique, composants, protocole, tests, UI, instruments, exemples,
performances et livraison).

## Limites connues finales

- Worker appelé dans le fil de l'interface ; Web Worker dédié et
  `SharedArrayBuffer` effectif restent à brancher.
- Transitoire à pas fixe et Euler arrière ; pas de pas adaptatif ni d'estimation
  d'erreur locale. Les excitations continues sont représentées par événements.
- AC petits signaux sans capacité de jonction ; FFT sans fenêtres alternatives.
- `OpAmp` : alimentations ignorées, pas de limite de courant, de slew rate ni de pôle AC.
- Diode sans résistance série/température ; audit d'énergie approximatif ;
  extraction en O(P·W + W²), adaptée aux schémas pédagogiques.
- Export CSV seul, sans persistance, PNG ni curseurs ; budgets perf non bloquants.
