# Architecture

## 1. Objectif et contraintes

CircuitSim sépare le moteur électrique, l'éditeur et l'interface pour que les
calculs, la topologie et la présentation soient testables indépendamment.

- Rust stable, édition 2021, natif et `wasm32-unknown-unknown`.
- Aucun `unsafe`, aucune API navigateur ni horloge globale dans `sim-core` et
  `editor-core`.
- Validation explicite aux frontières, erreurs typées, pas de panique sur une
  entrée utilisateur.
- Résultats déterministes ; code et API en anglais, interface et documentation
  en français.

## 2. Crates

| Crate | Rôle | Dépendances principales |
| --- | --- | --- |
| `sim-core` | unités, netlist, MNA, DC, transitoire, AC | `serde`, `thiserror`, `nalgebra` |
| `editor-core` | schéma, commandes, historique nommé, undo, ERC, extraction, JSON, séries E, snap/routage | `sim-core`, `serde`, `serde_json` |
| `worker-protocol` | trames, messages, erreurs, transport | `sim-core`, `postcard`, `serde` |
| `worker` | file bornée, coalescence, analyses, pont WASM | `sim-core`, `worker-protocol`, `wasm-bindgen` |
| `circuitsim-app` | egui, instruments, FFT, Bode, export, client worker | `editor-core`, `worker`, `egui_plot`, `rustfft` |
| `circuitsim-harness` | tests d'intégration, exemples, proptest | `sim-core`, `editor-core` |
| `xtask` | format, lint, tests, WASM, build web | outillage natif |

Les dépendances vont dans un sens : `editor-core` → `sim-core`, worker →
protocole + `sim-core`, application → éditeur + worker. L'interface ne manipule
jamais les matrices ni les structures internes des solveurs.

## 3. Flux de données

```text
Schematic --Command/Undo--> extraction --> Netlist validée
                                             |
                                             v
                                   protocole binaire / worker
                                             |
                                             v
                         DC, transitoire, AC --> résultats typés
                                             |
                                             v
                         traces, mesures, FFT, Bode, CSV
```

1. Une commande est appliquée sur un clone, validée, puis remplace l'état.
2. `extract_netlist` construit la connectivité, l'ERC, les noms et la netlist.
3. `validate_for_simulation` exige une masse unique nommée `0`.
4. Le worker revalide la netlist et exécute l'analyse demandée.
5. L'application associe le résultat à la révision du document et aux tables de
   noms capturées avant l'envoi.
6. Les instruments calculent localement traces, mesures, spectre et Bode.
7. Un résultat antérieur au schéma reste consultable mais est signalé.
8. L'interface ne reçoit jamais de solution partielle ni de matrice.

Le service worker est appelé dans le fil de l'interface, y compris dans l'hôte
web. Le worker dédié du navigateur et `SharedArrayBuffer` restent à brancher ; le
protocole et la politique de transport sont déjà testés.

## 4. Frontières de validation

Trois niveaux se complètent :

- **Construction** : noms, grille, nombres finis, unités et paramètres validés
  par les constructeurs.
- **Structure** : `Schematic::validate` et `Netlist::validate` vérifient les
  identifiants, doublons, références et invariants.
- **Simulation** : `validate_for_simulation` exige exactement un nœud `Ground`
  nommé `0` ; les solveurs le revalident.

Une netlist structurellement valide peut être incomplète. Les données
désérialisées sont toujours revalidées ; aucun identifiant ou nom de fichier
n'est supposé fiable.

## 5. Déterminisme

Les collections qui influencent un résultat sont ordonnées (`BTreeMap`,
`BTreeSet`). Les identifiants sont monotones, l'undo ne rembobine pas `next_id`,
les noms de nets sont stables et les doublons reçoivent `#2`, `#3`, etc.

Les composants sont assemblés dans l'ordre de la netlist, les événements triés
par instant puis composant, les fréquences AC générées dans l'ordre croissant et
les résultats stockés dans des maps ordonnées. Aucun aléatoire, ordre de hachage
ou horloge n'intervient. Une même entrée produit les mêmes netlists, noms, JSON,
échantillons, phasors, audits et CSV.

## 6. Erreurs et atomicité

Chaque couche expose son erreur (`UnitParseError`, `NameError`, `NetlistError`,
`SchematicError`, `EditError`, `ExtractError`, `DocumentError`, `MnaError`,
`DcError`, `TransientError`, `AcError`, `ProtocolError`, `WorkerError`,
`ServiceError`, `SimulationError`, `ExportError`). Elles implémentent
`std::error::Error` via `thiserror` lorsque c'est utile.

Une commande ou un undo n'est validé qu'après application sur une copie ; un
échec laisse l'état, la révision et les identifiants inchangés. Un job échoué
produit `Failed`, jamais un résultat partiel.

## 7. Unités et extensibilité

Les nombres internes sont en SI et typés (`Volts`, `Amps`, `Ohms`, `Farads`,
`Henrys`, `Seconds`, `Hertz`, `Watts`, `Coulombs`, `Celsius`). Les paramètres
sont des `ParamValue` (`Number`, `Bool`, `Text`) rangés dans une `BTreeMap`.
Parsing, formatage et validation sont détaillés dans [NETLIST.md](NETLIST.md).

Un symbole personnalisé est un `SymbolKind::Custom` dans l'éditeur et un
`ComponentKind::Custom` dans la netlist. Le registre valide son nom et ses
broches ; les solveurs le refusent explicitement avec
`MnaError::UnsupportedComponent` tant qu'aucun modèle n'existe. L'exemple
`examples/custom_mosfet.rs` illustre ce contrat ; `OpAmp` fournit, lui, un VCVS à
gain fini et saturation.

## 8. Qualité et livraison

La suite compte 224 tests natifs, dont six propriétés proptest. La CI exécute
formatage, clippy strict, tests, `cargo deny check licenses`, WASM, benchmarks
rapides, exemples, build natif release et build web. Les budgets de performance
sont documentés mais non bloquants. Voir [LICENSING.md](LICENSING.md) et
[GIT_WORKFLOW.md](GIT_WORKFLOW.md).

Documents associés : [DECISIONS.md](DECISIONS.md) pour les arbitrages,
[NUMERICS.md](NUMERICS.md) pour les équations, [PROTOCOL.md](PROTOCOL.md) pour le
transport, [TESTING.md](TESTING.md) pour la validation, [DELIVERY.md](DELIVERY.md)
pour les artefacts et [PERFORMANCE.md](PERFORMANCE.md) pour les mesures.
