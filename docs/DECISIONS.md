# Décisions d'architecture

Chaque décision est révisable, mais une révision doit mettre à jour le code, les
tests et les documents liés. Les détails d'implémentation restent dans les
documents spécialisés.

## Structure

1. **Séparer cœur, éditeur et interface.** `sim-core`, `editor-core`, le worker
   et `circuitsim-app` forment des couches à dépendances descendantes. Les
   solveurs et l'éditeur restent testables sans fenêtre ni navigateur ;
   `egui_plot` et `rustfft` restent dans l'application. Voir
   [ARCHITECTURE.md](ARCHITECTURE.md).
2. **Stocker en SI et valider aux frontières.** Les calculs utilisent des types
   SI et des paramètres finis ; le parsing et le formatage d'ingénierie sont des
   opérations de présentation. Une valeur invalide produit une erreur typée sans
   modifier l'état. Voir [NETLIST.md](NETLIST.md).
3. **Identifiants indexés et monotones.** `NodeId` et `ComponentId` égalent
   l'indice du vecteur validé ; le schéma utilise un compteur partagé que
   l'undo ne rembobine pas. Les résultats et les noms restent stables et
   reproductibles. Voir [EDITOR.md](EDITOR.md).

## Modèle et simulation

4. **Connectivité géométrique explicite.** Fils orthogonaux, contacts et T
   connectent ; un croisement X ne connecte qu'avec une jonction ; un label ne
   connecte que les conducteurs à sa position. L'ERC explique les ambiguïtés.
   Voir [EDITOR.md](EDITOR.md).
5. **MNA dense et LU déterministe.** `nalgebra` suffit aux schémas
   pédagogiques, reste portable en WASM et donne des résultats reproductibles.
   La complexité cubique limite la taille ; `faer` reste une option future.
   Voir [NUMERICS.md](NUMERICS.md).
6. **Transitoire à pas fixe, Euler arrière.** Le schéma est simple à tester et
   stable pour les circuits usuels. Les événements sont appliqués après
   l'échantillon exact ; pas adaptatif, erreur locale et formes d'onde continues
   restent hors périmètre. Voir [SIMULATION.md](SIMULATION.md).
7. **AC petits signaux autour du DC.** Diode, LED, Zener et opamp sont
   linéarisés au point de fonctionnement ; l'excitation vient d'`AcExcitation`,
   jamais du paramètre `dc`. Pas de capacité de jonction ni de modèle grand
   signal AC. Voir [COMPONENTS.md](COMPONENTS.md).

## Interface et transport

8. **Protocole binaire versionné.** Trames `CSIM` + postcard, un job en attente,
   coalescence à 60 Hz, révisions croissantes et résultats complets ou erreurs
   typées. L'interface ne voit jamais les matrices. Le service tourne encore
   dans le fil de l'interface ; worker dédié et `SharedArrayBuffer` restent à
   brancher. Voir [PROTOCOL.md](PROTOCOL.md).
9. **Instruments dans l'application.** Le catalogue, les traces, les mesures, la
   FFT de Hann, le Bode et le CSV sont des traitements de présentation dans
   `circuitsim-app`. Les graphiques sont décimés à 4 096 points ; mesures et
   exports gardent toutes les valeurs. Voir [INSTRUMENTS.md](INSTRUMENTS.md).
10. **Formats versionnés et interface bornée.** `SCHEMA_VERSION = 1`,
    `PROTOCOL_VERSION = 2`, 100 000 pas transitoires, 10 000 points AC et
    16 384 points FFT. Un format incompatible est refusé, jamais interprété
    approximativement. Voir [PROTOCOL.md](PROTOCOL.md) et
    [INSTRUMENTS.md](INSTRUMENTS.md).
11. **Presse-papiers interne et historique nommé.** Le copier/coller reste dans
    `circuitsim-app` : aucune dépendance au presse-papiers système n’est
    ajoutée, le contenu est déterministe et les identifiants sont régénérés au
    collage. L’historique conserve un libellé par commande et permet un saut
    bidirectionnel sans réécrire le schéma. Voir [EDITOR.md](EDITOR.md).
12. **Dispositions fiables plutôt qu’un dockage libre.** Les panneaux restent
    des zones fixes, redimensionnables et masquables, avec quatre presets
    testables. Aucune bibliothèque de dockage n’est ajoutée : le gain ne
    justifie pas le risque d’états incohérents, et le canevas garde toujours la
    priorité. La mini-carte est une projection pure testée dans `view.rs`. Voir
    [UI.md](UI.md).
13. **Simulation interactive par transitoire complet.** Le bouton **Simuler** et
    l’option **Auto** réutilisent le transitoire existant, avec un délai de
    0,35 s et une seule tentative par révision. Aucun solveur n’est déplacé dans
    le fil de l’interface et aucun résultat partiel n’est affiché. Le dernier
    échantillon alimente le canevas et l’inspecteur. Voir
    [INSTRUMENTS.md](INSTRUMENTS.md).
14. **Accessibilité sans dépendance supplémentaire.** Le contraste, la taille et
    la réduction des animations utilisent les styles egui et `set_zoom_factor`.
    Les préférences restent session-locales pour éviter d’activer la persistance
    d’eframe et ses dépendances ; `Tab`/`Maj+Tab` restent la navigation clavier
    native d’egui. Le thème **Clair** et les palettes de tension sont des jetons
    internes au canevas, sans nouvelle bibliothèque. Voir [UI.md](UI.md).
15. **Lisibilité par calques indépendants.** La tension, le courant, la
    puissance, les valeurs, les références et les labels sont des couches
    désactivables. La couleur de tension est normalisée sur la valeur absolue
    maximale du circuit, avec une palette daltonienne par défaut et une variante
    haute visibilité. L’erreur ERC et la sélection restent prioritaires. Aucun
    texte n’est l’unique porteur d’une information. Voir [UI.md](UI.md).

## Tests et périmètre

16. **Composants personnalisés représentés, pas simulés.** Le registre,
    l'extraction, le JSON et la netlist acceptent `Custom`; les solveurs le
    refusent avec `MnaError::UnsupportedComponent` jusqu'à l'ajout d'un modèle
    dans `sim-core`. Voir [COMPONENTS.md](COMPONENTS.md).
17. **Tests natifs, benchmarks et CI.** Les tests ne demandent ni navigateur ni
    ngspice ; les propriétés proptest affichent leur graine. La CI vérifie
    format, clippy, tests, WASM, benchmarks rapides, exemples et builds
    natif/web. Les budgets Criterion sont des seuils de revue, non bloquants sur
    un runner partagé. Voir [TESTING.md](TESTING.md) et
    [PERFORMANCE.md](PERFORMANCE.md).

## Licence et dépendances

Le projet est sous **MIT OR Apache-2.0**. Ce choix est compatible avec le
standard Rust, l’écosystème de dépendances et la distribution WASM. Les textes
sont à la racine ; les métadonnées SPDX sont héritées par chaque crate.

Les exceptions permissives suivantes sont autorisées dans `deny.toml` et
documentées ici :

| Licence | Dépendances concernées | Décision |
| --- | --- | --- |
| `BSL-1.0` | `clipboard-win`, `error-code` | permissive, compatible |
| `CC0-1.0` | `hexf-parse` | domaine public, compatible |
| `0BSD` | `adler2` | permissive, compatible |
| `Unlicense` | `aho-corasick`, `memchr`, `walkdir`, `byteorder-lite` | alternative MIT retenue quand disponible |
| `Apache-2.0 WITH LLVM-exception` | `rustix`, `linux-raw-sys`, `wasip2`, `wit-bindgen` | compatible avec Apache-2.0 |
| `OFL-1.1`, `Ubuntu-font-1.0` | `epaint_default_fonts` | polices embarquées compatibles |

Les alternatives `GPL-2.0-only` de `self_cell` et `LGPL-2.1-or-later` de `r-efi`
ne sont pas utilisées ; les branches MIT/Apache sont retenues. Toute nouvelle
licence hors liste exige une décision explicite avant ajout.

## Décisions différées

- Web Worker dédié et transport `SharedArrayBuffer`.
- Pas adaptatif et estimation d'erreur locale.
- Excitations continues paramétrées et sources dépendantes du temps.
- Capacités de jonction et modèles AC non linéaires.
- Persistance de projet, chargement de schéma, PNG et curseurs de mesure.
- Factorisation creuse et profils de performance pour de grands schémas.
