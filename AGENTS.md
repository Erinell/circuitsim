# AGENTS.md

## Mission

CircuitSim est un simulateur analogique natif et WASM. Le cœur, l'éditeur et
l'interface restent séparés. Chaque lot doit être compilable, testé, documenté,
versionné et accompagné d'un rapport.

## Règles non négociables

- Rust stable, édition 2021, `wasm32-unknown-unknown`.
- Aucun `unsafe`, `todo!()`, `unimplemented!()`, pseudo-code ou fonction vide.
- Commentaires limités à un *pourquoi* non évident, en une ligne.
- Code et API en anglais ; interface et documentation en français.
- Validation explicite, erreurs typées, pas de panique sur une entrée utilisateur.
- Résultats déterministes : collections ordonnées, tri stable, noms reproductibles.
- `sim-core` et `editor-core` sans egui, navigateur ni horloge globale.
- Aucune allocation ajoutée dans la boucle chaude du moteur.
- Résolution linéaire hors du thread principal.
- Tests natifs sans navigateur ni ngspice ; tout bug corrigé a un test de régression.

## Commandes

```text
cargo xtask check        # fmt check + clippy strict + tests
cargo xtask fmt
cargo xtask wasm-check
cargo xtask web-build
cargo bench --workspace --no-run
cargo deny check licenses
```

## Arborescence

- `crates/sim-core` : unités, netlist, MNA, DC, transitoire, AC.
- `crates/editor-core` : schéma, commandes, undo, ERC, extraction, JSON.
- `crates/protocol`, `crates/worker` : protocole binaire, ordonnancement, WASM.
- `crates/app` : egui, instruments, export ; hôte `index.html`.
- `crates/*/benches`, `examples/` : benchmarks Criterion et exemples.
- `tests/`, `src/lib.rs` : intégration et proptest.
- `.github/workflows/ci.yml`, `xtask/` : CI et automatisation.
- `docs/` : architecture, décisions, éditeur, UX, simulation, protocole, tests,
  performances, livraison, Git et licences.

## Contrats de données

- `NodeId` et `ComponentId` égalent l'indice dans leur vecteur validé.
- `Netlist::validate` accepte zéro ou plusieurs masses.
- `Netlist::validate_for_simulation` exige exactement une masse nommée `0`.
- `ComponentKind::Ground` pointe vers un nœud `NodeKind::Ground`.
- Nombres de paramètres finis et stockés en SI.
- Noms ≤ 128 caractères, sans caractère de contrôle.
- `NetName` coupe les espaces ; doublons renommés `#2`, `#3`, etc.
- `Schematic` utilise un compteur d'identifiants monotone partagé.
- L'undo restaure les objets complets mais pas `next_id`.
- Les champs `pub(crate)` du schéma restent dans `editor-core`.
- JSON versionné (`SCHEMA_VERSION = 1`) et revalidé au chargement.

## Contrats de simulation

- `solve_dc`, `simulate_transient` et `solve_ac` revalident la netlist.
- Courant de branche orienté `a -> b` ; source de courant `dc` de `a` vers `b`.
- Commutateur linéaire : `ron` si `closed`, sinon `roff`.
- Diode/LED Shockley ; Zener avec `bv`, `ibv`, `nbv`.
- `OpAmp` : VCVS à gain fini et saturation ; `Custom` refusé.
- Transitoire : Euler arrière à pas fixe ; événement après l'échantillon exact.
- AC petits signaux autour du DC ; `AcExcitation` porte amplitude et phase.
- `dc` n'est jamais une amplitude AC ; source absente = phasor nul.
- Fréquences finies, positives, ordonnées ; balayage explicite croissant.
- Audit d'énergie diagnostique en fin de pas.
- Résultats en `BTreeMap` ; jamais de solution partielle présentée comme valide.

## Contrats de connectivité

- Grille entière, bornes `-1_000_000..=1_000_000`.
- Fils orthogonaux, longueur strictement positive, extrémités normalisées.
- Contact colinéaire et T connectent ; X seulement avec `Junction`.
- Chevauchement colinéaire connecte avec ERC `WireOverlap`.
- Un label ne connecte que les conducteurs à sa position.
- Un groupe sans broche ne produit pas de net simulable.
- Noms : toutes les masses fusionnent en `0`, sinon plus petit label, sinon `N$001`, `N$002`, etc.
- L'ERC est un rapport ; seule une `ExtractError` bloque l'extraction.

## Stack cible

eframe/egui `0.35.0`, egui_plot `0.36.0`, faer `0.24.4` sans rayon, nalgebra
`0.35.0`, wasm-bindgen `0.2.129`, web-sys/js-sys `0.3.106`, postcard,
rustfft `6.4.1`, proptest, criterion `0.8.2`, Trunk `0.21.14`, cargo-deny
`0.20.2`, cargo-about `0.9.2`. Toute nouvelle dépendance exige une revue de
licence et l'accord de l'utilisateur.

Le MOSFET reste un exemple d'extension (`examples/custom_mosfet.rs`) ; les types personnalisés sont acceptés.

## Règles Git et licence

- `main` toujours vert ; une branche par lot, fusion `--no-ff`.
- Conventional Commits `type(scope): description` ≤ 72 caractères.
- Un commit = un changement logique, testé et formaté.
- Pas de push, de remote, de force-push ni de réécriture de `main` sans accord.
- `CHANGELOG.md` mis à jour avec chaque `feat`/`fix`.
- Tags annotés SemVer à chaque jalon.
- Licence `MIT OR Apache-2.0`, titulaire Cédric Debois, 2026.
- Pas d'en-tête de licence dans les sources ; métadonnées SPDX uniquement.
- `cargo deny check licenses` avant toute dépendance et en CI.
- `THIRD_PARTY_LICENSES.md` régénéré à chaque release.

## Quand lire quoi

| Sujet | Documents |
| --- | --- |
| Architecture et dépendances | `ARCHITECTURE.md`, `DECISIONS.md` |
| Éditeur et interactions | `EDITOR.md`, `UX.md`, `UI.md` |
| Netlist, unités, composants | `NETLIST.md`, `COMPONENTS.md` |
| Simulation et numérique | `SIMULATION.md`, `NUMERICS.md` |
| Protocole worker | `PROTOCOL.md` |
| Interface et instruments | `UI.md`, `INSTRUMENTS.md` |
| Tests et performances | `TESTING.md`, `PERFORMANCE.md` |
| Git et historique | `GIT_WORKFLOW.md` |
| Licence et dépendances | `LICENSING.md`, `DECISIONS.md` |
| Livraison et exemples | `DELIVERY.md`, `EXAMPLES.md` |

## Workflow d'étape

1. Lire l'état courant et les documents concernés.
2. Implémenter sans stub, avec tests et validation.
3. Exécuter `fmt`, `check`, `wasm-check`, `deny` et le benchmark concerné.
4. Mettre à jour docs, README, CHANGELOG et le tableau ci-dessous.
5. Vérifier `AGENTS.md` (≤ 150 lignes) et produire un rapport court.

## Avancement

| Jalon | État |
| --- | --- |
| Étapes 1–10, tag `v0.1.0` | terminé |
| G1 Git, licence, gouvernance | terminé |
| G2 placement et câblage | terminé |
| G3 sélection et édition | terminé |
| G4 navigation et disposition | terminé |
| G5 simulation interactive | terminé |
| G6 ergonomie du scope | terminé |
| G7 onboarding et accessibilité | terminé |
| G8 finalisation `v0.2.0` | terminé |
| Refonte ergonomique v0.5 (U1–U7), surintensités, diagnostics et convergence transitoire | terminé |
| Ergonomie du câblage (extensions perpendiculaires, fusion colinéaire, reroutage de chaîne sans accumulation, glissement sur deux axes) | terminé |

## Limites finales

- Worker et session temps réel dans le fil de l'interface ; Web Worker/SAB à brancher.
- Pas de pas adaptatif global ; subdivision de secours d'un pas en échec, excitations continues par événements ; AC sans capacité de jonction, opamp sans alimentation/pôle.
- FFT plafonnée, sans fenêtres alternatives ; export CSV seul, sans persistance/PNG ; panneaux non dockables, préférences de session.
- Budgets de performance documentés, non bloquants.
